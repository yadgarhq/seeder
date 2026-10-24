//! The exit rule over both answers (plans/team-access.md stage 6, reds 6, 8, 9,
//! 10), apart from any network.

use super::judge;
use crate::pb::yadgar::iam::v1::SyncTeamsResponse;
use crate::pb::yadgar::project::v1::ProjectServiceSyncProjectsResponse;

fn projects(grants_declared: u32) -> ProjectServiceSyncProjectsResponse {
    ProjectServiceSyncProjectsResponse {
        inserted: 0,
        updated: 0,
        archived: 0,
        reactivated: 0,
        unchanged: 0,
        grants_declared,
        grants_inserted: 0,
        grants_deleted: 0,
        grants_unchanged: 0,
    }
}

fn teams(unknown: &[&str], pending: &[&str], live: u32) -> SyncTeamsResponse {
    SyncTeamsResponse {
        teams_inserted: 0,
        teams_archived: 0,
        teams_reactivated: 0,
        members_added: 0,
        members_removed: 0,
        unknown_member_user_ids: unknown.iter().map(|s| s.to_string()).collect(),
        pending_administrator_user_ids: pending.iter().map(|s| s.to_string()).collect(),
        administrators_live_members: live,
    }
}

#[test]
fn teams_absent_and_no_grants_is_clean() {
    let j = judge(0, None, &projects(0));
    assert!(j.failures.is_empty(), "{:?}", j.failures);
    assert!(j.pending.is_empty());
}

/// Red 6.
#[test]
fn red_06_a_project_that_dropped_the_grants_is_named_as_predating_grants() {
    let j = judge(3, Some(&teams(&[], &[], 1)), &projects(0));
    assert_eq!(j.failures.len(), 1, "{:?}", j.failures);
    assert!(
        j.failures[0].contains("project predates grants"),
        "{:?}",
        j.failures
    );
    assert!(
        j.failures[0].contains('3') && j.failures[0].contains('0'),
        "{:?}",
        j.failures
    );
}

#[test]
fn matching_grant_counts_are_clean() {
    let j = judge(3, Some(&teams(&[], &[], 1)), &projects(3));
    assert!(j.failures.is_empty(), "{:?}", j.failures);
}

/// Red 8.
#[test]
fn red_08_an_unknown_member_fails_naming_the_uuid() {
    let id = "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
    let j = judge(0, Some(&teams(&[id], &[], 1)), &projects(0));
    assert_eq!(j.failures.len(), 1);
    assert!(j.failures[0].contains(id), "{:?}", j.failures);
}

/// Red 9.
#[test]
fn red_09_a_pending_administrator_with_no_live_member_is_not_a_failure() {
    let id = "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
    let j = judge(1, Some(&teams(&[], &[id], 0)), &projects(1));
    assert!(j.failures.is_empty(), "{:?}", j.failures);
    assert_eq!(j.pending, [id]);
}

/// Red 10.
#[test]
fn red_10_a_pending_administrator_beside_a_live_one_is_a_failure_naming_it() {
    let id = "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
    let j = judge(1, Some(&teams(&[], &[id], 1)), &projects(1));
    assert_eq!(j.failures.len(), 1);
    assert!(j.failures[0].contains(id), "{:?}", j.failures);
}

#[test]
fn every_failure_is_reported_not_only_the_first() {
    let a = "yadgar:user:0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
    let b = "yadgar:user:0192f3c4-bbbb-7bbb-8ccc-0123456789ab";
    let j = judge(2, Some(&teams(&[a], &[b], 1)), &projects(0));
    assert_eq!(j.failures.len(), 3, "{:?}", j.failures);
}
