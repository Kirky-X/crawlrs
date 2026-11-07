use super::*;
use crate::infrastructure::security::ssrf::SsrfError;

#[test]
fn test_robots_checker_error_cache_lock_display() {
    let err = RobotsCheckerError::CacheLockError("lock poisoned".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("Cache lock acquisition failed"));
    assert!(msg.contains("lock poisoned"));
}

#[test]
fn test_robots_checker_error_url_parse_display() {
    let err = RobotsCheckerError::UrlParseError("bad url".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("URL parse failed"));
    assert!(msg.contains("bad url"));
}

#[test]
fn test_robots_checker_error_validation_display() {
    let err = RobotsCheckerError::ValidationError("invalid input".to_string());
    let msg = format!("{}", err);
    assert!(msg.contains("Validation failed"));
    assert!(msg.contains("invalid input"));
}

// From<SsrfError> tests

#[test]
fn test_from_ssrf_error_converts_to_validation_error() {
    let ssrf_err = SsrfError::InvalidScheme {
        scheme: "ftp".to_string(),
    };
    let robots_err: RobotsCheckerError = ssrf_err.into();
    match robots_err {
        RobotsCheckerError::ValidationError(msg) => {
            assert!(
                msg.contains("ftp"),
                "error message should contain the scheme, got: {}",
                msg
            );
        }
        other => panic!("expected ValidationError variant, got {:?}", other),
    }
}

#[test]
fn test_from_ssrf_error_blocked_hostname() {
    let ssrf_err = SsrfError::BlockedHostname {
        hostname: "localhost".to_string(),
    };
    let robots_err: RobotsCheckerError = ssrf_err.into();
    match robots_err {
        RobotsCheckerError::ValidationError(msg) => {
            assert!(msg.contains("localhost"));
        }
        other => panic!("expected ValidationError variant, got {:?}", other),
    }
}

// impl_basic_error_conversions tests

#[test]
fn test_from_string_creates_validation_error() {
    let err: RobotsCheckerError = "something went wrong".to_string().into();
    match err {
        RobotsCheckerError::ValidationError(msg) => {
            assert_eq!(msg, "something went wrong");
        }
        other => panic!("expected ValidationError variant, got {:?}", other),
    }
}

#[test]
fn test_from_str_creates_validation_error() {
    let err: RobotsCheckerError = "bad input".into();
    match err {
        RobotsCheckerError::ValidationError(msg) => {
            assert_eq!(msg, "bad input");
        }
        other => panic!("expected ValidationError variant, got {:?}", other),
    }
}

#[test]
fn test_from_anyhow_error_creates_validation_error() {
    let anyhow_err = anyhow::anyhow!("anyhow failure");
    let err: RobotsCheckerError = anyhow_err.into();
    match err {
        RobotsCheckerError::ValidationError(msg) => {
            assert!(msg.contains("anyhow failure"));
        }
        other => panic!("expected ValidationError variant, got {:?}", other),
    }
}
