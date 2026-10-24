//! The chart's numbers are the binary's numbers. Each pair is stated twice —
//! once in `chart/`, once in this crate or the dial it pins — and this is where
//! the two stop being a comment. Every number is also pinned by a LITERAL
//! (ADR-0599), so moving both sides together is still a deliberate edit here.

use std::path::PathBuf;

use serde_json::Value;
use yadgar_seeder::config::{MAX_ATTEMPTS, MAX_BACKOFF};
use yadgar_seeder::registry::MAX_BYTES;

fn read(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|why| panic!("{} must be readable: {why}", path.display()))
}

fn schema() -> Value {
    serde_json::from_str(&read("chart/values.schema.json"))
        .expect("chart/values.schema.json must be valid JSON")
}

fn range(block: &str, key: &str) -> (u64, u64) {
    let s = schema();
    let property = &s["properties"][block]["properties"][key];
    assert_eq!(property["type"], "integer", "{block}.{key}");
    (
        property["minimum"].as_u64().expect("minimum"),
        property["maximum"].as_u64().expect("maximum"),
    )
}

#[test]
fn the_request_timeout_range_is_the_dial_s() {
    let (min, max) = range("project", "requestTimeoutSeconds");
    assert_eq!(min, yadgar_dial::min_request_timeout().as_secs());
    assert_eq!(max, yadgar_dial::max_request_timeout().as_secs());
    assert_eq!((min, max), (14, 59));
}

/// iam's timeout is bounded by the same dial as project's.
#[test]
fn the_iam_request_timeout_range_is_the_dial_s() {
    let (min, max) = range("iam", "requestTimeoutSeconds");
    assert_eq!(min, yadgar_dial::min_request_timeout().as_secs());
    assert_eq!(max, yadgar_dial::max_request_timeout().as_secs());
    assert_eq!((min, max), (14, 59));
}

#[test]
fn the_attempts_range_is_the_binary_s() {
    let (min, max) = range("sync", "attempts");
    assert_eq!((min, max), (1, u64::from(MAX_ATTEMPTS)));
    assert_eq!(max, 60);
}

#[test]
fn the_backoff_range_is_the_binary_s() {
    let (min, max) = range("sync", "backoffSeconds");
    assert_eq!(min, yadgar_dial::reresolve_interval().as_secs());
    assert_eq!(max, MAX_BACKOFF.as_secs());
    assert_eq!((min, max), (5, 300));
}

/// The size guard is one number in two places: the render check refuses at it,
/// and the binary refuses at it. Both under the 1 MiB a ConfigMap holds.
#[test]
fn the_size_guard_is_one_number() {
    let checks = read("chart/templates/render-checks.yaml");
    let literal = format!("gt $bytes {MAX_BYTES}");
    assert!(
        checks.contains(&literal),
        "render-checks.yaml must refuse at `{literal}`"
    );
    assert_eq!(MAX_BYTES, 786_432);
    const { assert!(MAX_BYTES < 1024 * 1024) };
}

/// The chart default for the seeder's hop sits ABOVE `project`'s own hop to
/// `project-db` (30 s in `project`'s chart), so the seeder receives the store's
/// answer rather than racing it. 30 is pinned by literal: `project`'s chart is
/// another repository, and moving it past this value must be noticed here.
#[test]
fn the_default_timeout_outlasts_project_s_store_hop() {
    let values = read("chart/values.yaml");
    let line = values
        .lines()
        .find(|l| l.trim_start().starts_with("requestTimeoutSeconds:"))
        .expect("values.yaml sets project.requestTimeoutSeconds");
    let secs: u64 = line.split(':').nth(1).unwrap().trim().parse().unwrap();
    const PROJECT_DB_HOP_SECS: u64 = 30;
    assert!(
        secs > PROJECT_DB_HOP_SECS,
        "{secs} must exceed {PROJECT_DB_HOP_SECS}"
    );
}
