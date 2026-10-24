//! Teams, members and grants of the declaration (plans/team-access.md §2).
//!
//! **THE TRANSITION RULE LIVES HERE.** With `teams` absent or empty no `grants`
//! key may appear anywhere, the result declares no team, and the caller makes no
//! `SyncTeams` call. An ADR-0946 auto-pin can bring this seeder to a cluster
//! whose GitOps repository declares no team yet, and that run must behave like
//! the seeder before team access.
//!
//! **THE ADMINISTRATORS RULE, MADE MECHANICAL (ADR-0962 ruling 5).** With teams
//! declared, a team named `administrators` exists with at least one member, and
//! every root carries a `readWrite` grant for it. The chart schema refuses the
//! first half too (`if`/`then` + `contains`, a named exception in its
//! `$comment`); this is the same rule again for a hand-edited ConfigMap.
//!
//! The team and member grammar mirrors iam-db's `SyncTeams` (`DeclaredTeam` in
//! `iam.proto`), which stays the authority.

use std::collections::HashSet;

use serde::Deserialize;

use super::{invalid, Entry, RegistryError};
use crate::pb::yadgar::iam::v1::DeclaredTeam;
use crate::pb::yadgar::project::v1::{DeclaredGrant, GrantLevel};

/// The team every declaration with teams must hold (ADR-0962 ruling 5).
pub const ADMINISTRATORS: &str = "administrators";

/// The wire prefix of a team id: `yadgar:team:<name>` (`project.proto`,
/// `DeclaredGrant.team_id`).
const TEAM_PREFIX: &str = "yadgar:team:";

/// The wire prefix of a member: `yadgar:user:<uuid>` (`iam.proto`,
/// `DeclaredTeam.member_user_ids`).
const USER_PREFIX: &str = "yadgar:user:";

/// The widest team name: the grammar `^[a-z][a-z0-9-]{0,62}$` allows 63.
const MAX_NAME_LEN: usize = 63; // ADR-0569-EXCEPTION(CC): the store's grammar, not a knob.

/// One team as the document states it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Team {
    name: String,
    members: Vec<String>,
}

/// One grant as the document states it: a declared team and a level.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Grant {
    team: String,
    level: Level,
}

/// The two levels the document spells (`read | readWrite`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(super) enum Level {
    #[serde(rename = "read")]
    Read,
    #[serde(rename = "readWrite")]
    ReadWrite,
}

/// A grant in the contract's element type. EXHAUSTIVE on purpose, like
/// `declared` in the parent: a field added to `DeclaredGrant` fails to compile
/// here rather than being sent at its zero value.
pub(super) fn wire_grant(g: &Grant) -> DeclaredGrant {
    let level = match g.level {
        Level::Read => GrantLevel::Read,
        Level::ReadWrite => GrantLevel::ReadWrite,
    };
    DeclaredGrant {
        team_id: format!("{TEAM_PREFIX}{}", g.team),
        level: level as i32,
    }
}

/// Check every team, member and grant, and return the teams in the iam
/// contract's element type. `Ok(vec![])` is the transition rule.
pub(super) fn check(
    teams: &[Team],
    roots: &[Entry],
    projects: &[Entry],
) -> Result<Vec<DeclaredTeam>, RegistryError> {
    if teams.is_empty() {
        return no_grants_without_teams(roots, projects).map(|()| Vec::new());
    }
    let names = team_names(teams)?;
    administrators(teams)?;
    for (list, entries) in [("roots", roots), ("projects", projects)] {
        for e in entries {
            row_grants(list, e, &names)?;
        }
    }
    for root in roots {
        let held = root
            .grants
            .iter()
            .any(|g| g.team == ADMINISTRATORS && g.level == Level::ReadWrite);
        if !held {
            return Err(invalid(format!(
                "the root {:?} carries no `readWrite` grant for the team {ADMINISTRATORS:?}. With \
                 `registry.teams` declared, every root grants it (ADR-0962 ruling 5): \
                 registry.roots[].grants needs {{team: {ADMINISTRATORS}, level: readWrite}}",
                root.path
            )));
        }
    }
    Ok(teams.iter().map(wire_team).collect())
}

fn no_grants_without_teams(roots: &[Entry], projects: &[Entry]) -> Result<(), RegistryError> {
    for (list, entries) in [("roots", roots), ("projects", projects)] {
        if let Some(e) = entries.iter().find(|e| !e.grants.is_empty()) {
            return Err(invalid(format!(
                "registry.{list}[].grants is set on {:?} while `registry.teams` is absent or \
                 empty. A grant names a declared team: declare the teams, or remove every \
                 `grants` key (the transition rule, plans/team-access.md §2)",
                e.path
            )));
        }
    }
    Ok(())
}

/// Each name's grammar and uniqueness. Returns the set of names.
fn team_names(teams: &[Team]) -> Result<HashSet<&str>, RegistryError> {
    let mut names = HashSet::new();
    for t in teams {
        let mut chars = t.name.chars();
        let legal = t.name.len() <= MAX_NAME_LEN
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !legal {
            return Err(invalid(format!(
                "registry.teams[{}].name {:?} is not a team name. A name matches \
                 ^[a-z][a-z0-9-]{{0,62}}$",
                t.name, t.name
            )));
        }
        if !names.insert(t.name.as_str()) {
            return Err(invalid(format!(
                "registry.teams[{}].name {:?} is declared twice. Names are unique; a rename is \
                 an archive plus a new team",
                t.name, t.name
            )));
        }
    }
    Ok(names)
}

/// The `administrators` team exists and has members; every member of every team
/// is a lowercase canonical uuidv7, once per team.
fn administrators(teams: &[Team]) -> Result<(), RegistryError> {
    let Some(admin) = teams.iter().find(|t| t.name == ADMINISTRATORS) else {
        return Err(invalid(format!(
            "registry.teams is declared and holds no team named {ADMINISTRATORS:?}. With teams, \
             the team {ADMINISTRATORS:?} exists with at least one member (ADR-0962 ruling 5)"
        )));
    };
    if admin.members.is_empty() {
        return Err(invalid(format!(
            "registry.teams[{ADMINISTRATORS}].members is empty. On a cold install it would leave \
             the bootstrap create-user nothing to create; on a live install the lockout guard \
             would refuse it"
        )));
    }
    for t in teams {
        let mut seen = HashSet::new();
        for m in &t.members {
            if !canonical_uuid_v7(m) {
                return Err(invalid(format!(
                    "registry.teams[{}].members has {m:?}, which is not a lowercase canonical \
                     uuidv7: 36 characters, 8-4-4-4-12 lowercase hex, version 7, variant 8, 9, a \
                     or b. Declare the bare uuid `GET /auth/whoami` answers as account_segment",
                    t.name
                )));
            }
            if !seen.insert(m.as_str()) {
                return Err(invalid(format!(
                    "registry.teams[{}].members lists {m:?} more than once",
                    t.name
                )));
            }
        }
    }
    Ok(())
}

/// `8-4-4-4-12` lowercase hex, version nibble `7`, variant nibble `8 9 a b`
/// (RFC 4122), the form ADR-0954 gives `account_segment`.
fn canonical_uuid_v7(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(c),
        })
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

/// One row's grants name declared teams, each at most once.
fn row_grants(list: &str, e: &Entry, names: &HashSet<&str>) -> Result<(), RegistryError> {
    let mut seen = HashSet::new();
    for g in &e.grants {
        if !names.contains(g.team.as_str()) {
            return Err(invalid(format!(
                "registry.{list}[].grants on {:?} names the team {:?}, which `registry.teams` does \
                 not declare",
                e.path, g.team
            )));
        }
        if !seen.insert(g.team.as_str()) {
            return Err(invalid(format!(
                "registry.{list}[].grants on {:?} names the team {:?} more than once",
                e.path, g.team
            )));
        }
    }
    Ok(())
}

/// A team in the contract's element type, members on the wire as
/// `yadgar:user:<uuid>`.
fn wire_team(t: &Team) -> DeclaredTeam {
    DeclaredTeam {
        name: t.name.clone(),
        member_user_ids: t
            .members
            .iter()
            .map(|m| format!("{USER_PREFIX}{m}"))
            .collect(),
    }
}

#[cfg(test)]
mod tests;
