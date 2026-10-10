//! The provider-failure classification table, as pure data.

use reqwest::StatusCode;

use ratatoskr_threads_archive::capture_resolution::{Resolution, classify};
use ratatoskr_threads_archive::public_resolution::PublicResolutionError;

#[test]
fn deleted_statuses_are_permanent_deletions() {
    for status in [StatusCode::NOT_FOUND, StatusCode::GONE] {
        assert_eq!(
            classify(&PublicResolutionError::ProviderStatus(status)),
            Resolution::Deleted,
            "{status}"
        );
    }
}

#[test]
fn forbidden_and_unusable_responses_are_permanently_inaccessible() {
    assert_eq!(
        classify(&PublicResolutionError::ProviderStatus(
            StatusCode::FORBIDDEN
        )),
        Resolution::Inaccessible
    );
    for error in [
        PublicResolutionError::InvalidJson,
        PublicResolutionError::MissingField("html"),
        PublicResolutionError::WrongProvider,
        PublicResolutionError::PermalinkMismatch,
    ] {
        assert_eq!(classify(&error), Resolution::Inaccessible, "{error}");
    }
}

#[test]
fn throttling_server_errors_and_transport_failures_are_transient() {
    for status in [
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::SERVICE_UNAVAILABLE,
    ] {
        assert_eq!(
            classify(&PublicResolutionError::ProviderStatus(status)),
            Resolution::Transient,
            "{status}"
        );
    }
    for error in [
        PublicResolutionError::Network,
        PublicResolutionError::ResponseTooLarge,
    ] {
        assert_eq!(classify(&error), Resolution::Transient, "{error}");
    }
}
