//! Stage 6 reds 1 to 4, 11 to 13 (plans/team-access.md), at the declaration
//! parser: every refusal names the key or the path, and nothing is dialled
//! because the parser runs before the dial.

use super::super::{parse, Declaration, RegistryError};
use crate::pb::yadgar::project::v1::GrantLevel;

const ADMIN: &str = "0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
const RUNNER: &str = "0192f3c4-bbbb-7bbb-9ccc-0123456789ab";

fn ok(text: &str) -> Declaration {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("expected a valid declaration, got: {e}"))
}

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

fn admin_grant() -> &'static str {
    r#"{"team": "administrators", "level": "readWrite"}"#
}

/// A declaration with `teams` and one root carrying `grants`.
fn doc(teams: &str, root_grants: &str, project_grants: &str) -> String {
    format!(
        r#"{{
  "roots": [{{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd", "grants": [{root_grants}]}}],
  "projects": [{{"path": "yadgarhq/gateway", "displayName": "g", "sourceRepo": "yadgarhq/gateway", "grants": [{project_grants}]}}],
  "teams": [{teams}],
  "allowMassArchive": false,
  "allowMassRevoke": false
}}"#
    )
}

fn team(name: &str, members: &[&str]) -> String {
    let members: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
    format!(
        r#"{{"name": "{name}", "members": [{}]}}"#,
        members.join(",")
    )
}

fn good_teams() -> String {
    format!(
        "{},{}",
        team("administrators", &[ADMIN]),
        team("estate-runners", &[RUNNER])
    )
}

fn good() -> String {
    doc(&good_teams(), admin_grant(), "")
}

#[test]
fn a_good_team_declaration_is_carried_in_the_wire_shape() {
    let d = ok(&doc(
        &good_teams(),
        &format!(
            r#"{admin}, {{"team": "estate-runners", "level": "read"}}"#,
            admin = admin_grant()
        ),
        "",
    ));
    assert_eq!(d.teams.len(), 2);
    assert_eq!(d.teams[0].name, "administrators");
    assert_eq!(d.teams[0].member_user_ids, [format!("yadgar:user:{ADMIN}")]);
    let grants = &d.roots[0].grants;
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0].team_id, "yadgar:team:administrators");
    assert_eq!(grants[0].level, GrantLevel::ReadWrite as i32);
    assert_eq!(grants[1].team_id, "yadgar:team:estate-runners");
    assert_eq!(grants[1].level, GrantLevel::Read as i32);
    assert!(d.projects[0].grants.is_empty());
    assert!(!d.allow_mass_revoke);
}

#[test]
fn the_mass_revoke_flag_is_read_as_written() {
    let text = good().replace(r#""allowMassRevoke": false"#, r#""allowMassRevoke": true"#);
    assert!(ok(&text).allow_mass_revoke);
}

/// Red 1.
#[test]
fn red_01_teams_without_administrators_is_refused_naming_the_key() {
    let text = doc(&team("estate-runners", &[RUNNER]), "", "");
    refused(&text, "registry.teams");
    refused(&text, "administrators");
}

/// Red 2.
#[test]
fn red_02_a_root_without_an_administrators_read_write_grant_is_refused_naming_the_root() {
    refused(&doc(&good_teams(), "", ""), "\"yadgarhq\"");
    // A read grant is not enough: the rule is readWrite.
    refused(
        &doc(
            &good_teams(),
            r#"{"team": "administrators", "level": "read"}"#,
            "",
        ),
        "\"yadgarhq\"",
    );
    // Another team's readWrite grant is not enough either.
    refused(
        &doc(
            &good_teams(),
            r#"{"team": "estate-runners", "level": "readWrite"}"#,
            "",
        ),
        "administrators",
    );
}

/// Red 3.
#[test]
fn red_03_a_grant_naming_an_undeclared_team_is_refused_naming_the_path_and_the_team() {
    let text = doc(
        &good_teams(),
        admin_grant(),
        r#"{"team": "ghosts", "level": "read"}"#,
    );
    refused(&text, "yadgarhq/gateway");
    refused(&text, "ghosts");
}

/// A needle unique to the stage 12 refusal. Main's older refusals for the same
/// inputs also say "registry.teams", so that key alone proves nothing.
const NEW: &str = "A declaration names its teams";

/// Stage 12, the CLEAN empty case: `teams: []` with no grants and no
/// `allowMassRevoke`. Main accepted it (the transition rule); no other check
/// refuses it, so only the new refusal can make this pass.
#[test]
fn stage_12_a_clean_empty_teams_list_is_refused() {
    let clean = r#"{
      "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}],
      "projects": [],
      "teams": [],
      "allowMassArchive": false
    }"#;
    refused(clean, NEW);
    refused(clean, "absent or empty");
}

/// Stage 12: the transition rule is gone. A declaration without `teams` is
/// refused, and the refusal names the key `teams`. Nothing is dialled because
/// the parser runs before the dial.
#[test]
fn stage_12_teams_absent_is_refused_naming_the_key() {
    let without_teams = r#"{
      "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}],
      "projects": [],
      "allowMassArchive": false
    }"#;
    refused(without_teams, NEW);
    refused(without_teams, "absent");
}

/// Stage 12: an empty list is the same declaration as an absent one.
#[test]
fn stage_12_teams_empty_is_refused_naming_the_key() {
    refused(&doc("", admin_grant(), ""), NEW);
    let empty = r#"{
      "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}],
      "projects": [],
      "teams": [],
      "allowMassArchive": false,
      "allowMassRevoke": false
    }"#;
    refused(empty, NEW);
}

/// Red 11.
#[test]
fn red_11_an_unknown_key_under_teams_or_grants_is_refused_by_name() {
    let team_typo = good().replace(r#""members""#, r#""memberz""#);
    refused(&team_typo, "memberz");
    let grant_typo = doc(
        &good_teams(),
        r#"{"team": "administrators", "level": "readWrite", "lvl": "x"}"#,
        "",
    );
    refused(&grant_typo, "lvl");
}

/// Red 12, the seeder half.
#[test]
fn red_12_administrators_with_no_members_is_refused_naming_the_key() {
    let text = doc(&team("administrators", &[]), admin_grant(), "");
    refused(&text, "teams[administrators].members");
}

/// Red 13: each class of stage 2 red 13, in bare form.
#[test]
fn red_13_a_member_that_is_not_a_lowercase_canonical_uuidv7_is_refused_by_name() {
    let bad = [
        ("uppercase hex", "0192F3C4-AAAA-7BBB-8CCC-0123456789AB"),
        ("uuidv4", "0192f3c4-aaaa-4bbb-8ccc-0123456789ab"),
        ("variant nibble c", "0192f3c4-aaaa-7bbb-cccc-0123456789ab"),
        ("35 characters", "0192f3c4-aaaa-7bbb-8ccc-0123456789a"),
        ("the nil uuid", "00000000-0000-0000-0000-000000000000"),
        (
            "the wire prefix",
            "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab",
        ),
        ("no hyphens", "0192f3c4aaaa7bbb8ccc0123456789ab"),
    ];
    for (class, member) in bad {
        let text = doc(&team("administrators", &[member]), admin_grant(), "");
        match parse(text.as_bytes()) {
            Ok(_) => panic!("{class}: {member:?} must be refused"),
            Err(RegistryError::Invalid(msg)) => {
                assert!(msg.contains(member), "{class}: must name the member: {msg}");
                assert!(
                    msg.contains("teams[administrators].members"),
                    "{class}: must name the key: {msg}"
                );
            }
            Err(other) => panic!("{class}: wrong refusal: {other}"),
        }
    }
}

#[test]
fn a_duplicate_member_in_one_team_is_refused() {
    refused(
        &doc(&team("administrators", &[ADMIN, ADMIN]), admin_grant(), ""),
        "more than once",
    );
}

#[test]
fn one_user_in_two_teams_is_legal() {
    let teams = format!(
        "{},{}",
        team("administrators", &[ADMIN]),
        team("estate-runners", &[ADMIN])
    );
    ok(&doc(&teams, admin_grant(), ""));
}

#[test]
fn a_team_name_outside_the_grammar_is_refused() {
    for name in ["Administrators", "1abc", "has space", "", &"a".repeat(64)] {
        let teams = format!(
            "{},{}",
            team("administrators", &[ADMIN]),
            team(name, &[RUNNER])
        );
        refused(&doc(&teams, admin_grant(), ""), "teams[");
    }
}

#[test]
fn a_duplicate_team_name_is_refused() {
    let teams = format!(
        "{},{}",
        team("administrators", &[ADMIN]),
        team("administrators", &[RUNNER])
    );
    refused(&doc(&teams, admin_grant(), ""), "declared twice");
}

#[test]
fn a_team_twice_on_one_row_is_refused() {
    let grants = format!("{}, {}", admin_grant(), admin_grant());
    refused(&doc(&good_teams(), &grants, ""), "more than once");
}

#[test]
fn an_unknown_level_is_refused_by_name() {
    let text = doc(
        &good_teams(),
        r#"{"team": "administrators", "level": "write"}"#,
        "",
    );
    refused(&text, "write");
}

/// ADR-0939's pattern for the new flag: with teams declared the flag has no
/// default.
#[test]
fn allow_mass_revoke_is_required_once_teams_are_declared() {
    let text = good().replace(",\n  \"allowMassRevoke\": false", "");
    refused(&text, "allowMassRevoke");
}

#[test]
fn a_local_path_still_cannot_carry_a_grant() {
    let text = good().replace("yadgarhq/gateway", "local/max/x");
    refused(&text, "local");
}

/// THE MASS-REVOKE HOLE, CLOSED (now doubly: `teams` is required). A declaration
/// without `teams` would send empty grant sets, the shape that deletes every stored grant, so the
/// flag that lifts the store's guard must not ride along. The chart renders no
/// `allowMassRevoke` without teams (it refuses the render); a hand-edited ConfigMap carrying one is
/// refused, `true` and `false` alike, naming `registry.teams` (the stage 12 refusal) and
/// `registry.allowMassRevoke`.
#[test]
fn allow_mass_revoke_without_teams_is_refused_by_name() {
    for flag in ["true", "false"] {
        let text = format!(
            r#"{{
      "roots": [{{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}}],
      "projects": [],
      "allowMassArchive": false,
      "allowMassRevoke": {flag}
    }}"#
        );
        refused(&text, "registry.allowMassRevoke");
        refused(&text, "registry.teams");
    }
}
