//! ZIP inspection, extraction, and deterministic parser for Data Export.

use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

/// Inspects ZIP metadata before any parser reads archive content.
///
/// # Errors
///
/// Returns a typed refusal for malformed or resource-unsafe archives.
pub fn inspect_archive(
    bytes: &[u8],
    limits: ExportLimits,
) -> Result<InspectedArchive, ExportError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(ExportError::InvalidZip)?;
    if archive.len() > limits.max_entries {
        return Err(ExportError::Limit {
            limit: "entry_count",
            detail: archive.len().to_string(),
        });
    }
    let mut compressed_bytes = 0_u64;
    let mut decompressed_bytes = 0_u64;
    let mut entry_names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(ExportError::InvalidZip)?;
        compressed_bytes =
            checked_total(compressed_bytes, file.compressed_size(), "compressed_bytes")?;
        decompressed_bytes = checked_total(decompressed_bytes, file.size(), "decompressed_bytes")?;
        if compressed_bytes > limits.max_compressed_bytes {
            return Err(size_error("compressed_bytes", compressed_bytes));
        }
        if decompressed_bytes > limits.max_decompressed_bytes {
            return Err(size_error("decompressed_bytes", decompressed_bytes));
        }
        if exceeds_ratio(
            file.size(),
            file.compressed_size(),
            limits.max_compression_ratio,
        ) {
            return Err(ExportError::Limit {
                limit: "compression_ratio",
                detail: file.name().to_owned(),
            });
        }
        entry_names.push(validate_entry_name(file.name(), limits.max_path_depth)?);
    }
    Ok(InspectedArchive { entry_names })
}

/// Extracts already-inspected ZIP entries only beneath `root`.
///
/// # Errors
///
/// Returns [`ExportError`] when inspection rejects the archive or a private
/// extraction operation fails. No caller-supplied archive path is used as an
/// extraction destination.
/// Extracts inspected entries only beneath a caller-owned private root.
///
/// # Errors
///
/// Returns a typed refusal or private extraction error.
pub fn extract_archive(
    bytes: &[u8],
    limits: ExportLimits,
    root: &Path,
) -> Result<ExtractedArchive, ExportError> {
    let inspected = inspect_archive(bytes, limits)?;
    fs::create_dir_all(root).map_err(|source| extraction_error("creating root", source))?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(ExportError::InvalidZip)?;
    let mut extracted_bytes = 0_u64;
    for (index, name) in inspected.entry_names.iter().enumerate() {
        let mut file = archive.by_index(index).map_err(ExportError::InvalidZip)?;
        let written = write_entry(&mut file, root, name, limits.max_decompressed_bytes)?;
        extracted_bytes = checked_total(extracted_bytes, written, "decompressed_bytes")?;
        if extracted_bytes > limits.max_decompressed_bytes {
            return Err(size_error("decompressed_bytes", extracted_bytes));
        }
    }
    Ok(ExtractedArchive {
        entry_names: inspected.entry_names,
    })
}

fn write_entry<R: Read>(
    file: &mut R,
    root: &Path,
    name: &str,
    max_bytes: u64,
) -> Result<u64, ExportError> {
    let path = root.join(name);
    let parent = path.parent().ok_or_else(|| path_error(name))?;
    fs::create_dir_all(parent).map_err(|source| extraction_error("creating parent", source))?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| extraction_error("creating entry", source))?;
    let written = copy_bounded(file, &mut output, max_bytes)?;
    output
        .sync_all()
        .map_err(|source| extraction_error("syncing entry", source))?;
    Ok(written)
}

fn copy_bounded<R: Read, W: std::io::Write>(
    input: &mut R,
    output: &mut W,
    maximum: u64,
) -> Result<u64, ExportError> {
    let mut total = 0_u64;
    let mut buffer = [0_u8; 8_192];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|source| extraction_error("reading entry", source))?;
        if read == 0 {
            return Ok(total);
        }
        total = checked_total(total, read as u64, "decompressed_bytes")?;
        if total > maximum {
            return Err(size_error("decompressed_bytes", total));
        }
        let written = buffer
            .get(..read)
            .ok_or_else(|| size_error("entry_read", read as u64))?;
        output
            .write_all(written)
            .map_err(|source| extraction_error("writing entry", source))?;
    }
}

fn extraction_error(operation: &'static str, source: std::io::Error) -> ExportError {
    ExportError::Extraction { operation, source }
}

fn exceeds_ratio(uncompressed: u64, compressed: u64, maximum: u64) -> bool {
    if uncompressed == 0 {
        return false;
    }
    compressed == 0 || uncompressed > compressed.saturating_mul(maximum)
}

fn checked_total(total: u64, value: u64, limit: &'static str) -> Result<u64, ExportError> {
    total
        .checked_add(value)
        .ok_or_else(|| size_error(limit, u64::MAX))
}

fn size_error(limit: &'static str, value: u64) -> ExportError {
    ExportError::Limit {
        limit,
        detail: value.to_string(),
    }
}

fn validate_entry_name(name: &str, max_depth: usize) -> Result<String, ExportError> {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') {
        return Err(path_error(name));
    }
    let path = Path::new(name);
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || name.split('/').any(str::is_empty)
    {
        return Err(path_error(name));
    }
    if name.split('/').count() > max_depth {
        return Err(ExportError::Limit {
            limit: "path_depth",
            detail: name.to_owned(),
        });
    }
    Ok(name.to_owned())
}

fn path_error(name: &str) -> ExportError {
    ExportError::Limit {
        limit: "path",
        detail: name.to_owned(),
    }
}

/// Parses the one supported Threads export layout after safe inspection.
///
/// # Errors
///
/// Returns an [`ExportError`] when inspection, layout selection, version
/// detection, or JSON parsing fails. Unknown archive entries are retained in
/// [`ParsedExport::unknown_entries`] rather than causing a schema guess.
/// Parses the supported export layout after safe inspection.
///
/// # Errors
///
/// Returns a typed error for unsafe archives, unsupported layouts, or invalid manifests.
pub fn parse_export(bytes: &[u8], limits: ExportLimits) -> Result<ParsedExport, ExportError> {
    let inspected = inspect_archive(bytes, limits)?;
    if !inspected
        .entry_names
        .iter()
        .any(|name| name == "threads_export.json")
    {
        return Err(ExportError::UnsupportedLayout);
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(ExportError::InvalidZip)?;
    let mut manifest = String::new();
    archive
        .by_name("threads_export.json")
        .map_err(ExportError::InvalidZip)?
        .read_to_string(&mut manifest)
        .map_err(|error| manifest_read_error(&error))?;
    let manifest = serde_json::from_str(&manifest).map_err(ExportError::InvalidManifest)?;
    normalize_manifest(manifest, inspected.entry_names)
}

fn manifest_read_error(error: &std::io::Error) -> ExportError {
    ExportError::Limit {
        limit: "manifest_read",
        detail: error.kind().to_string(),
    }
}

fn normalize_manifest(
    manifest: ExportManifest,
    entry_names: Vec<String>,
) -> Result<ParsedExport, ExportError> {
    if manifest.version != SUPPORTED_EXPORT_VERSION {
        return Err(ExportError::UnsupportedVersion(manifest.version));
    }
    let mut posts = manifest
        .posts
        .into_iter()
        .map(|post| ExportPost {
            provider_post_id: post.id,
            permalink: post.permalink,
            text: post.text,
        })
        .collect::<Vec<_>>();
    posts.sort_by(|left, right| left.provider_post_id.cmp(&right.provider_post_id));
    let mut relations = manifest
        .relations
        .into_iter()
        .map(|relation| ExportRelation {
            referencing_provider_post_id: relation.from,
            relation_kind: relation.kind,
            target_provider_post_id: relation.to,
        })
        .collect::<Vec<_>>();
    relations.sort();
    let mut unknown_entries = entry_names
        .into_iter()
        .filter(|name| name != "threads_export.json")
        .collect::<Vec<_>>();
    unknown_entries.sort();
    Ok(ParsedExport {
        detected_version: SUPPORTED_EXPORT_VERSION.to_owned(),
        parser_version: PARSER_VERSION,
        posts,
        relations,
        unknown_entries,
    })
}

#[derive(Debug, serde::Deserialize)]
struct ExportManifest {
    version: String,
    #[serde(default)]
    posts: Vec<ExportManifestPost>,
    #[serde(default)]
    relations: Vec<ExportManifestRelation>,
}

#[derive(Debug, serde::Deserialize)]
struct ExportManifestPost {
    id: String,
    permalink: String,
    text: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ExportManifestRelation {
    from: String,
    kind: String,
    to: String,
}
