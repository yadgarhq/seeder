//! The exit rule over both answers (plans/team-access.md, stage 6).
//!
//! **THE ANSWERS ARE JUDGED TOGETHER, AFTER BOTH CALLS.** A refused call never
//! reaches here: it ends the run in `main`. What is judged is an APPLIED sync
//! whose counts say something is wrong, and every such finding is reported, so
//! one run names everything the operator has to fix.

use crate::pb::yadgar::iam::v1::SyncTeamsResponse;
use crate::pb::yadgar::project::v1::ProjectServiceSyncProjectsResponse;

/// What the two answers mean for the exit code.
#[derive(Debug, Default)]
pub struct Judgement {
    /// Each one makes the run exit non-zero. Empty means exit 0.
    pub failures: Vec<String>,
    /// F2's expected first sync (ADR-0963 (5)): declared administrators with no
    /// user row yet, while `administrators` has no live member. Not a failure.
    pub pending: Vec<String>,
}

/// Judge the answers. `grants_sent` is the number of grants the seeder put on
/// the wire; `teams` is `None` when `SyncTeams` was not called (the transition
/// rule).
pub fn judge(
    grants_sent: u32,
    teams: Option<&SyncTeamsResponse>,
    projects: &ProjectServiceSyncProjectsResponse,
) -> Judgement {
    let mut j = Judgement::default();
    if projects.grants_declared != grants_sent {
        j.failures.push(format!(
            "project parsed {} grants and the seeder sent {grants_sent}: project predates grants. \
             It dropped `DeclaredProject.grants` as an unknown field, so no grant was stored. \
             Release `project` with the team-access contract before declaring grants",
            projects.grants_declared
        ));
    }
    let Some(teams) = teams else {
        return j;
    };
    if !teams.unknown_member_user_ids.is_empty() {
        j.failures.push(format!(
            "iam skipped {} declared member(s) that have no user: {}. Every other member and \
             every grant converged. Correct the uuid in registry.teams[].members, or enrol the \
             user, then sync again",
            teams.unknown_member_user_ids.len(),
            teams.unknown_member_user_ids.join(", ")
        ));
    }
    if !teams.pending_administrator_user_ids.is_empty() {
        if teams.administrators_live_members == 0 {
            j.pending = teams.pending_administrator_user_ids.clone();
        } else {
            j.failures.push(format!(
                "iam reports pending administrator(s) {} while administrators_live_members is {}. \
                 A pending row exists only while `administrators` has no live member: this is a \
                 store defect",
                teams.pending_administrator_user_ids.join(", "),
                teams.administrators_live_members
            ));
        }
    }
    j
}

#[cfg(test)]
mod tests;
