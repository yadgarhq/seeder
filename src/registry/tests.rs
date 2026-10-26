use super::{parse, Declaration, RegistryError, MAX_BYTES};

/// A well-formed declaration: one root, two projects, the guard on.
const GOOD: &str = r#"{
  "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd", "grants": [{"team": "administrators", "level": "readWrite"}]}],
  "projects": [
    {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway"},
    {"path": "yadgarhq/project-db", "displayName": "", "sourceRepo": "yadgarhq/project-db"}
  ],
  "teams": [{"name": "administrators", "members": ["0192f3c4-aaaa-7bbb-8ccc-0123456789ab"]}], "allowMassRevoke": false,
  "allowMassArchive": false
}"#;

fn ok(text: &str) -> Declaration {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("expected a valid declaration, got: {e}"))
}

/// Every refusal names what was wrong. `needle` must appear in the message, so
/// a refusal for the WRONG reason does not pass the test.
fn refused(text: &str, needle: &str) {
    match parse(text.as_bytes()) {
        Ok(d) => panic!("expected a refusal naming {needle:?}, got a declaration: {d:?}"),
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains(needle),
                "the refusal must name {needle:?}; got: {msg}"
            );
        }
    }
}

/// The declaration with `roots` and `projects` replaced.
fn with(roots: &str, projects: &str) -> String {
    format!(r#"{{"roots": {roots}, "projects": {projects}, {TEAMS}, "allowMassArchive": false}}"#)
}

/// The `teams` and `allowMassRevoke` keys every declaration carries (stage 12).
const TEAMS: &str = r#""teams": [{"name": "administrators", "members": ["0192f3c4-aaaa-7bbb-8ccc-0123456789ab"]}], "allowMassRevoke": false"#;

fn entry(path: &str) -> String {
    format!(
        r#"{{"path": "{path}", "displayName": "", "sourceRepo": "yadgarhq/argocd", "grants": [{{"team": "administrators", "level": "readWrite"}}]}}"#
    )
}

#[test]
fn a_good_declaration_is_carried_exactly() {
    let d = ok(GOOD);
    assert_eq!(d.roots.len(), 1);
    assert_eq!(d.roots[0].path, "yadgarhq");
    assert_eq!(d.roots[0].source_repo, "yadgarhq/argocd");
    assert_eq!(d.projects.len(), 2);
    assert_eq!(d.projects[0].path, "yadgarhq/gateway");
    assert_eq!(d.projects[0].display_name, "gateway");
    assert_eq!(d.projects[1].path, "yadgarhq/project-db");
    assert_eq!(d.projects[1].display_name, "");
    assert!(!d.allow_mass_archive);
}

#[test]
fn the_mass_archive_flag_is_read_as_written() {
    let text = GOOD.replace(
        r#""allowMassArchive": false"#,
        r#""allowMassArchive": true"#,
    );
    assert!(ok(&text).allow_mass_archive);
}

/// `projects: []` is a legal, explicit statement that the organisation has
/// only its anchors.
#[test]
fn an_empty_project_list_is_legal() {
    let d = ok(&with(&format!("[{}]", entry("yadgarhq")), "[]"));
    assert!(d.projects.is_empty());
}

/// LEDGER 1244 OPTION B. A single-segment root is a namespace anchor and never
/// a landing site (ADR-0673), so the estate's smoke runs need a TWO-SEGMENT
/// registered path to land on. The declaration expresses that as a second
/// root `estate` with the project `estate/smoke` beneath it. A multi-segment
/// ROOT is refused (see below), because the store refuses one.
#[test]
fn ledger_1244_option_b_is_expressible() {
    let d = ok(&with(
        &format!("[{}, {}]", entry("yadgarhq"), entry("estate")),
        &format!("[{}, {}]", entry("yadgarhq/gateway"), entry("estate/smoke")),
    ));
    assert_eq!(
        d.roots.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        ["yadgarhq", "estate"]
    );
    assert_eq!(d.projects[1].path, "estate/smoke");
}

#[test]
fn malformed_json_is_refused() {
    refused("{ this is not json", "not a valid registry document");
}

#[test]
fn an_unknown_key_is_refused() {
    let text = GOOD.replace("\"roots\"", "\"rootz\"");
    refused(&text, "rootz");
}

#[test]
fn an_absent_mass_archive_flag_is_refused() {
    refused(
        &format!(
            r#"{{"roots": [{}], "projects": [], {TEAMS}}}"#,
            entry("yadgarhq")
        ),
        "allowMassArchive",
    );
}

#[test]
fn an_absent_source_repo_key_is_refused() {
    refused(
        &with(r#"[{"path": "yadgarhq", "displayName": ""}]"#, "[]"),
        "sourceRepo",
    );
}

#[test]
fn no_root_is_refused() {
    refused(&with("[]", "[]"), "at least one root");
}

#[test]
fn a_multi_segment_root_is_refused() {
    refused(
        &with(&format!("[{}]", entry("estate/smoke")), "[]"),
        "\"estate/smoke\"",
    );
}

#[test]
fn a_project_under_no_declared_root_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("other/thing")),
        ),
        "\"other/thing\" is not under a declared root",
    );
}

/// A single-segment project is not STRICTLY beneath a root, even when it
/// equals one: that would be the root itself.
#[test]
fn a_single_segment_project_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("gateway")),
        ),
        "\"gateway\" is not under a declared root",
    );
}

#[test]
fn a_local_path_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("local/max/thing")),
        ),
        "local/max/thing",
    );
}

/// The store folds ASCII case on `local`, so every spelling is the reserved
/// segment.
#[test]
fn a_local_root_in_any_case_is_refused() {
    refused(&with(&format!("[{}]", entry("LOCAL")), "[]"), "\"LOCAL\"");
}

/// Compared case-folded, because the store's walk folds ASCII case: two
/// spellings of one path are one row.
#[test]
fn a_duplicate_compared_case_folded_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!(
                "[{}, {}]",
                entry("yadgarhq/Gateway"),
                entry("yadgarhq/gateway")
            ),
        ),
        "declared twice",
    );
}

#[test]
fn a_root_declared_again_as_a_project_spelling_is_a_duplicate() {
    refused(
        &with(
            &format!("[{}, {}]", entry("yadgarhq"), entry("YadgarHQ")),
            "[]",
        ),
        "declared twice",
    );
}

/// A project under a root spelled in another case IS under that root: the store
/// compares the first segment case-folded.
#[test]
fn a_project_under_a_root_spelled_in_another_case_is_accepted() {
    ok(&with(
        &format!("[{}]", entry("YadgarHQ")),
        &format!("[{}]", entry("yadgarhq/gateway")),
    ));
}

#[test]
fn an_empty_source_repo_is_refused() {
    refused(
        &with(
            r#"[{"path": "yadgarhq", "displayName": "", "sourceRepo": ""}]"#,
            "[]",
        ),
        "no sourceRepo",
    );
}

#[test]
fn a_bad_path_segment_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("yadgarhq/has space")),
        ),
        "\"has space\"",
    );
}

#[test]
fn an_empty_segment_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("yadgarhq//x")),
        ),
        "yadgarhq//x",
    );
}

#[test]
fn a_trailing_slash_is_refused() {
    refused(&with(&format!("[{}]", entry("yadgarhq/")), "[]"), "slash");
}

#[test]
fn a_dot_segment_is_refused() {
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry("yadgarhq/..")),
        ),
        "\"..\"",
    );
}

#[test]
fn an_empty_path_is_refused() {
    refused(&with(&format!("[{}]", entry("")), "[]"), "path is empty");
}

#[test]
fn a_display_name_wider_than_the_column_is_refused() {
    let wide = "x".repeat(256);
    refused(
        &with(
            &format!(
                r#"[{{"path": "yadgarhq", "displayName": "{wide}", "sourceRepo": "yadgarhq/argocd"}}]"#
            ),
            "[]",
        ),
        "displayName",
    );
}

#[test]
fn a_display_name_exactly_at_the_column_width_is_accepted() {
    let wide = "x".repeat(255);
    ok(&with(
        &format!(
            r#"[{{"path": "yadgarhq", "displayName": "{wide}", "sourceRepo": "yadgarhq/argocd", "grants": [{{"team": "administrators", "level": "readWrite"}}]}}]"#
        ),
        "[]",
    ));
}

#[test]
fn a_path_longer_than_the_store_allows_is_refused() {
    let long = format!("yadgarhq/{}", "x".repeat(250));
    refused(
        &with(
            &format!("[{}]", entry("yadgarhq")),
            &format!("[{}]", entry(&long)),
        ),
        "characters",
    );
}

#[test]
fn a_document_over_the_size_guard_is_refused_before_it_is_parsed() {
    // Not JSON at all: the guard must refuse on size alone, before the parser
    // runs, or this would be reported as malformed instead.
    let big = vec![b' '; MAX_BYTES + 1];
    match parse(&big) {
        Err(RegistryError::TooLarge { bytes, .. }) => assert_eq!(bytes, MAX_BYTES + 1),
        other => panic!("expected TooLarge, got {other:?}"),
    }
}
