//! The knobs: every one required, none defaulted, and no key that could name
//! anything but `project` (plans/the-d35-seeder.md stage 5, test 5).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::time::Duration;

use yadgar_seeder::config::{Config, ConfigError, KEYS};

fn complete() -> Vec<(&'static str, &'static str)> {
    vec![
        ("SEEDER_REGISTRY_FILE", "/etc/seeder/registry.json"),
        ("PROJECT_HOST", "project"),
        ("PROJECT_PORT", "50052"),
        ("PROJECT_REQUEST_TIMEOUT_SECONDS", "45"),
        ("PROJECT_TLS_CA_FILE", "/var/run/config/project-ca/ca.pem"),
        (
            "PROJECT_TLS_CLIENT_CERT_FILE",
            "/var/run/secrets/client-cert/client.crt",
        ),
        (
            "PROJECT_TLS_CLIENT_KEY_FILE",
            "/var/run/secrets/client-cert/client.key",
        ),
        ("SEEDER_SYNC_ATTEMPTS", "10"),
        ("SEEDER_SYNC_BACKOFF_SECONDS", "6"),
    ]
}

fn read(vars: &[(&str, &str)]) -> Result<Config, ConfigError> {
    Config::from_lookup(|k| {
        vars.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    })
}

fn without(key: &str) -> Vec<(&'static str, &'static str)> {
    complete().into_iter().filter(|(k, _)| *k != key).collect()
}

fn with(key: &'static str, value: &'static str) -> Vec<(&'static str, &'static str)> {
    let mut vars = without(key);
    vars.push((key, value));
    vars
}

#[test]
fn a_complete_environment_is_read_verbatim() {
    let c = read(&complete()).expect("complete");
    assert_eq!(c.registry_file.to_str(), Some("/etc/seeder/registry.json"));
    assert_eq!(c.project_host, "project");
    assert_eq!(c.project_port, 50052);
    assert_eq!(c.request_timeout, Duration::from_secs(45));
    assert_eq!(
        c.tls.client_cert_file.to_str(),
        Some("/var/run/secrets/client-cert/client.crt")
    );
    assert_eq!(c.tls.domain, None);
    assert_eq!(c.retry.attempts, 10);
    assert_eq!(c.retry.backoff, Duration::from_secs(6));
}

/// Stage 5, test 5. THE KEYS A REAL READ LOOKS UP, recorded rather than
/// declared, equal [`KEYS`] — and none of them can name `project-db` or any
/// other host. A key added to the read and not to `KEYS` is red here.
#[test]
fn the_only_upstream_the_configuration_can_name_is_project() {
    let asked = RefCell::new(BTreeSet::new());
    let vars = complete();
    Config::from_lookup(|k| {
        asked.borrow_mut().insert(k.to_string());
        vars.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    })
    .expect("complete");
    let asked = asked.into_inner();
    let declared: BTreeSet<String> = KEYS.iter().map(|k| k.to_string()).collect();
    assert_eq!(asked, declared);
    for key in &asked {
        assert!(!key.contains("DB"), "{key} could name a -db (ADR-0936)");
        assert!(
            !key.ends_with("_HOST") || key == "PROJECT_HOST",
            "{key} is a second upstream host"
        );
    }
}

#[test]
fn every_required_knob_refuses_when_absent_and_names_itself() {
    for (key, _) in complete() {
        let err = read(&without(key)).expect_err(key).to_string();
        assert!(err.contains(key), "the refusal must name {key}: {err}");
    }
}

/// Helm renders an unset value as `""`, so set-but-empty is what a nulled chart
/// value produces. It refuses with its OWN message.
#[test]
fn an_empty_knob_refuses_differently_from_an_absent_one() {
    let empty = read(&with("PROJECT_HOST", "")).unwrap_err().to_string();
    let absent = read(&without("PROJECT_HOST")).unwrap_err().to_string();
    assert!(empty.contains("EMPTY"), "{empty}");
    assert!(absent.contains("NOT SET"), "{absent}");
}

#[test]
fn the_tls_domain_is_an_optional_override() {
    let c = read(&with("PROJECT_TLS_DOMAIN", "project.yadgar.svc")).unwrap();
    assert_eq!(c.tls.domain.as_deref(), Some("project.yadgar.svc"));
}

#[test]
fn a_port_that_is_not_a_number_is_refused() {
    let err = read(&with("PROJECT_PORT", "fifty"))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("PROJECT_PORT") && err.contains("project.port"),
        "{err}"
    );
}

/// yadgar-dial's own range: 14..=59 seconds.
#[test]
fn a_request_timeout_outside_dial_s_range_is_refused() {
    for bad in ["0", "13", "60"] {
        let err = read(&with("PROJECT_REQUEST_TIMEOUT_SECONDS", bad))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("PROJECT_REQUEST_TIMEOUT_SECONDS"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn zero_attempts_is_refused() {
    let err = read(&with("SEEDER_SYNC_ATTEMPTS", "0"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SEEDER_SYNC_ATTEMPTS"), "{err}");
}

#[test]
fn attempts_above_the_ceiling_are_refused() {
    let err = read(&with("SEEDER_SYNC_ATTEMPTS", "61"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("SEEDER_SYNC_ATTEMPTS"), "{err}");
}

/// The floor is dial's re-resolve interval (5 s): a shorter pause retries the
/// endpoint set that just failed.
#[test]
fn a_backoff_below_the_re_resolve_interval_is_refused() {
    let err = read(&with("SEEDER_SYNC_BACKOFF_SECONDS", "4"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("re-resolve"), "{err}");
    read(&with("SEEDER_SYNC_BACKOFF_SECONDS", "5")).expect("the floor itself is legal");
}
