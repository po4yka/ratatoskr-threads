//! Capability-model contract: what the matrix answers for each acquisition
//! mode, that authority ceilings hold, that local constants align with the
//! published social-contract grammars plus the documented Telegram extension,
//! and that upstream availability never touches local preservation.
//!
//! The contract vocabularies pinned here are copied from
//! `ratatoskr-contracts` `crates/social-contracts/src/vocabulary.rs` at
//! revision `fb88f94` (2026-08-25), recorded in `docs/CAPABILITY_MATRIX.md`.

use ratatoskr_threads_archive::capability::{
    AcquisitionMode, NativeSavedSupport, PreservationState, SavedAuthority, SupportStatus,
    UpstreamAvailability, retention_after_observation,
};

/// The `AcquisitionMethod` vocabulary of `ratatoskr-social-contracts@fb88f94`.
const CONTRACT_ACQUISITION_METHODS: [&str; 6] = [
    "official_api",
    "share_extension",
    "browser_extension",
    "public_resolution",
    "data_export",
    "legacy_import",
];

/// The `SavedAuthority` vocabulary of `ratatoskr-social-contracts@fb88f94`.
const CONTRACT_SAVED_AUTHORITIES: [&str; 4] = [
    "explicit_user_capture",
    "export_observation",
    "authoritative_platform_state",
    "legacy_observation",
];

/// The single documented Threads extension of the contract method grammar:
/// Telegram is a first-class capture client lane in this bounded context.
const LOCAL_EXTENSION_METHODS: [&str; 1] = ["telegram_capture"];

/// The documented matrix row for one mode: wire methods plus authority ceiling.
#[derive(Debug)]
struct Expected {
    mode: AcquisitionMode,
    wire_methods: &'static [&'static str],
    ceiling: SavedAuthority,
}

fn documented_matrix() -> [Expected; 5] {
    use AcquisitionMode::{
        DataExport, ExplicitCapture, LegacyImport, OwnAccountSync, PublicResolution,
    };
    use SavedAuthority::{
        AuthoritativePlatformState as Authoritative, ExplicitUserCapture as Explicit,
        ExportObservation as Export, LegacyObservation as Legacy,
    };
    [
        Expected {
            mode: ExplicitCapture,
            wire_methods: &["share_extension", "browser_extension", "telegram_capture"],
            ceiling: Explicit,
        },
        Expected {
            mode: PublicResolution,
            wire_methods: &["public_resolution"],
            ceiling: Explicit,
        },
        Expected {
            mode: OwnAccountSync,
            wire_methods: &["official_api"],
            ceiling: Authoritative,
        },
        Expected {
            mode: DataExport,
            wire_methods: &["data_export"],
            ceiling: Export,
        },
        Expected {
            mode: LegacyImport,
            wire_methods: &["legacy_import"],
            ceiling: Legacy,
        },
    ]
}

#[test]
fn each_mode_resolves_to_its_documented_capability() {
    let mut seen: Vec<AcquisitionMode> = Vec::new();
    for expected in documented_matrix() {
        let capability = expected.mode.capability();
        assert_eq!(capability.authority_ceiling, expected.ceiling);

        let mut produced = capability.wire_methods().to_vec();
        produced.sort_unstable();
        let mut documented = expected.wire_methods.to_vec();
        documented.sort_unstable();
        assert_eq!(
            produced, documented,
            "wire vocabulary mismatch for {expected:?}"
        );

        seen.push(expected.mode);
    }
    let mut inventory = AcquisitionMode::ALL.to_vec();
    inventory.sort_unstable();
    seen.sort_unstable();
    assert_eq!(
        inventory, seen,
        "the mode inventory is exactly the five documented modes"
    );
}

#[test]
fn only_implemented_lanes_claim_support() {
    for mode in AcquisitionMode::ALL {
        let status = mode.capability().status;
        if matches!(
            mode,
            AcquisitionMode::ExplicitCapture | AcquisitionMode::PublicResolution
        ) {
            assert_eq!(
                status,
                SupportStatus::Supported,
                "implemented capture and public-resolution lanes must report support"
            );
        } else {
            assert_eq!(
                status,
                SupportStatus::Planned,
                "{mode:?} must not claim support before its plan item lands"
            );
        }
    }
    let supported = AcquisitionMode::ALL
        .iter()
        .filter(|mode| mode.capability().status == SupportStatus::Supported)
        .count();
    assert_eq!(
        supported, 2,
        "exactly explicit capture and public resolution may claim support today"
    );
}

#[test]
fn native_saved_list_synchronization_reports_not_supported_with_reason() {
    let saved: NativeSavedSupport = native_saved_support();
    assert_eq!(
        saved.status,
        SupportStatus::NotSupported,
        "no supported provider surface exposes the personal Saved list"
    );
    assert!(
        saved.reason.contains("no supported"),
        "the reason must state the provider limitation: {:?}",
        saved.reason
    );
}

/// Lookup helper so the test above reads against the constant's type.
fn native_saved_support() -> NativeSavedSupport {
    ratatoskr_threads_archive::capability::NATIVE_SAVED_LIST_SYNC
}

#[test]
fn only_own_account_sync_reaches_authoritative_platform_state() {
    for expected in documented_matrix() {
        let ceiling = expected.mode.capability().authority_ceiling;
        if expected.mode == AcquisitionMode::OwnAccountSync {
            assert_eq!(ceiling, SavedAuthority::AuthoritativePlatformState);
        } else {
            assert_ne!(
                ceiling,
                SavedAuthority::AuthoritativePlatformState,
                "{expected:?} must never reach authoritative platform state"
            );
        }
    }
}

#[test]
fn local_method_set_equals_contract_vocabulary_plus_the_telegram_extension() {
    let mut owned: Vec<(&str, usize)> = Vec::new();
    for mode in AcquisitionMode::ALL {
        for method in mode.capability().wire_methods() {
            match owned.iter_mut().find(|(name, _)| *name == *method) {
                Some((_, count)) => *count += 1,
                None => owned.push((method, 1)),
            }
        }
    }

    let mut local = owned.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    local.sort_unstable();

    let mut expected = CONTRACT_ACQUISITION_METHODS.to_vec();
    expected.extend_from_slice(&LOCAL_EXTENSION_METHODS);
    expected.sort_unstable();

    assert_eq!(
        local, expected,
        "the local method set must be exactly the contract vocabulary plus telegram_capture"
    );
    for (name, count) in owned {
        assert_eq!(
            count, 1,
            "{name} must be produced by exactly one acquisition mode"
        );
    }
}

#[test]
fn reachable_saved_authority_set_equals_the_contract_vocabulary() {
    let mut authorities = AcquisitionMode::ALL
        .iter()
        .map(|mode| mode.capability().authority_ceiling.wire_value())
        .collect::<Vec<_>>();
    authorities.sort_unstable();
    authorities.dedup();

    let mut contract_authorities = CONTRACT_SAVED_AUTHORITIES.to_vec();
    contract_authorities.sort_unstable();

    assert_eq!(
        authorities, contract_authorities,
        "the reachable authority set must equal the contract vocabulary"
    );
}

#[test]
fn upstream_availability_wire_values_match_the_documented_schema_vocabulary() {
    let wire: Vec<&str> = UpstreamAvailability::ALL
        .iter()
        .map(|value| value.wire_value())
        .collect();
    assert_eq!(
        wire,
        [
            "active",
            "deleted",
            "private_or_inaccessible",
            "author_unavailable",
            "temporarily_unavailable",
            "unknown",
        ],
        "the availability vocabulary mirrors the posts.upstream_status and \
         tombstones.availability CHECK values value for value"
    );
}

#[test]
fn applying_any_observation_changes_no_preservation_state() {
    for current in PreservationState::ALL {
        for observed in UpstreamAvailability::ALL {
            assert_eq!(
                retention_after_observation(current, observed),
                current,
                "observing {observed:?} must not change {current:?}"
            );
        }
    }
}
