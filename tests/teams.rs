//! Team access, stage 6 (plans/team-access.md): the real binary against a fake
//! `IamService` and a fake `ProjectService`, both over real mutual TLS. Red
//! numbers are the plan's.

mod support;

use support::iam::{answer, clean, serve_iam, FakeIam, Order};
use support::{counts_with_grants, env, env_iam, run, serve, text, Fake, Pki, Scratch};
use tonic::Status;
use yadgar_seeder::pb::yadgar::project::v1::GrantLevel;

const ADMIN: &str = "0192f3c4-aaaa-7bbb-8ccc-0123456789ab";
const RUNNER: &str = "0192f3c4-bbbb-7bbb-9ccc-0123456789ab";

/// Today's argocd values: no `teams`, no `grants`, no `allowMassRevoke`.
const TODAY: &str = r#"{
  "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd"}],
  "projects": [
    {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway"}
  ],
  "allowMassArchive": false
}"#;

/// Two teams, three grants: administrators readWrite on the root, estate-runners
/// read on the root and readWrite on one project.
const TEAMS: &str = r#"{
  "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd",
    "grants": [
      {"team": "administrators", "level": "readWrite"},
      {"team": "estate-runners", "level": "read"}
    ]}],
  "projects": [
    {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway",
     "grants": [{"team": "estate-runners", "level": "readWrite"}]}
  ],
  "teams": [
    {"name": "administrators", "members": ["0192f3c4-aaaa-7bbb-8ccc-0123456789ab"]},
    {"name": "estate-runners", "members": ["0192f3c4-bbbb-7bbb-9ccc-0123456789ab"]}
  ],
  "allowMassArchive": false,
  "allowMassRevoke": false
}"#;

/// The grants `TEAMS` puts on the wire.
const GRANTS: u32 = 3;

const SYNC: &[&str] = &["sync-projects"];

struct Rig {
    pki: Pki,
    order: Order,
    iam: FakeIam,
    project: Fake,
    scratch: Scratch,
    vars: Vec<(String, String)>,
}

/// Both fakes up, scripted, and the environment a teams-declaring chart renders.
async fn rig(
    registry: &str,
    iam: Vec<Result<yadgar_seeder::pb::yadgar::iam::v1::SyncTeamsResponse, Status>>,
    project: Vec<
        Result<yadgar_seeder::pb::yadgar::project::v1::ProjectServiceSyncProjectsResponse, Status>,
    >,
) -> Rig {
    let pki = Pki::new();
    let order = Order::default();
    let iam = FakeIam::answering(iam, &order);
    let project = Fake::answering(project).ordered(&order);
    let iam_port = serve_iam(&pki, iam.clone()).await;
    let port = serve(&pki, project.clone()).await;
    let scratch = Scratch::new();
    let vars = env_iam(&scratch, &pki, port, iam_port, registry);
    Rig {
        pki,
        order,
        iam,
        project,
        scratch,
        vars,
    }
}

/// The success path: teams applied, grants echoed.
async fn green(registry: &str) -> Rig {
    rig(
        registry,
        vec![Ok(clean(2, 2))],
        vec![Ok(counts_with_grants(GRANTS))],
    )
    .await
}

/// Red 5: `teams` absent and no grants: no `SyncTeams` call, `SyncProjects`
/// sent with empty grant sets and the revoke flag unset, exactly as before team
/// access. The iam fake is up and the `IAM_*` block is in the environment, so a
/// call that should not happen could happen.
#[tokio::test(flavor = "multi_thread")]
async fn red_05_teams_absent_makes_no_syncteams_call_and_sends_empty_grants() {
    let rig = rig(TODAY, vec![], vec![Ok(counts_with_grants(0))]).await;

    let out = run(rig.vars.clone(), SYNC).await;

    assert!(out.status.success(), "{}", text(&out));
    assert!(rig.iam.received().is_empty(), "no SyncTeams call");
    assert_eq!(rig.order.calls(), ["SyncProjects"]);
    let req = &rig.project.received()[0].request;
    assert!(req
        .roots
        .iter()
        .chain(&req.projects)
        .all(|p| p.grants.is_empty()));
    assert!(!req.allow_mass_revoke);
    assert!(
        !text(&out).contains("sending SyncTeams"),
        "no SyncTeams line in the log:\n{}",
        text(&out)
    );
}

/// Red 5 again, as the Job runs it before argocd declares teams: the chart
/// renders no `IAM_*` block at all, and the run still succeeds.
#[tokio::test(flavor = "multi_thread")]
async fn red_05_teams_absent_needs_no_iam_configuration() {
    let pki = Pki::new();
    let project = Fake::answering(vec![]);
    let port = serve(&pki, project.clone()).await;
    let scratch = Scratch::new();

    let out = run(env(&scratch, &pki, port, "seeder-caller", TODAY), SYNC).await;

    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(project.received().len(), 1);
}

/// The whole path: teams first, then projects, one request_id on both, both
/// from the seeder's leaf under the seeder's principal, and every declared
/// team, member and grant on the wire in its contract shape.
#[tokio::test(flavor = "multi_thread")]
async fn teams_go_to_iam_first_then_projects_under_one_request_id() {
    let rig = green(TEAMS).await;

    let out = run(rig.vars.clone(), SYNC).await;

    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(rig.order.calls(), ["SyncTeams", "SyncProjects"]);

    let teams = &rig.iam.received()[0];
    assert_eq!(teams.caller.as_deref(), Some("seeder-caller"));
    let scope = teams.request.scope.as_ref().expect("a Scope");
    assert_eq!(scope.user_id, "yadgar:system:seeder");
    let names: Vec<_> = teams
        .request
        .teams
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(names, ["administrators", "estate-runners"]);
    assert_eq!(
        teams.request.teams[0].member_user_ids,
        [format!("yadgar:user:{ADMIN}")]
    );
    assert_eq!(
        teams.request.teams[1].member_user_ids,
        [format!("yadgar:user:{RUNNER}")]
    );

    let projects = &rig.project.received()[0];
    assert_eq!(projects.caller.as_deref(), Some("seeder-caller"));
    let project_scope = projects.request.scope.as_ref().unwrap();
    assert_eq!(
        project_scope.request_id, scope.request_id,
        "one sync, one id"
    );

    let root = &projects.request.roots[0];
    assert_eq!(root.grants.len(), 2);
    assert_eq!(root.grants[0].team_id, "yadgar:team:administrators");
    assert_eq!(root.grants[0].level, GrantLevel::ReadWrite as i32);
    assert_eq!(root.grants[1].team_id, "yadgar:team:estate-runners");
    assert_eq!(root.grants[1].level, GrantLevel::Read as i32);
    assert_eq!(
        projects.request.projects[0].grants[0].level,
        GrantLevel::ReadWrite as i32
    );
    assert!(!projects.request.allow_mass_revoke);
    drop(rig.pki);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_mass_revoke_flag_reaches_the_wire_as_declared() {
    let rig =
        green(&TEAMS.replace(r#""allowMassRevoke": false"#, r#""allowMassRevoke": true"#)).await;

    let out = run(rig.vars.clone(), SYNC).await;

    assert!(out.status.success(), "{}", text(&out));
    assert!(rig.project.received()[0].request.allow_mass_revoke);
}

/// Red 6: a `project` that drops field 4 answers `grants_declared = 0`.
#[tokio::test(flavor = "multi_thread")]
async fn red_06_a_project_that_drops_grants_exits_non_zero_naming_the_mismatch() {
    let rig = rig(
        TEAMS,
        vec![Ok(clean(2, 2))],
        vec![Ok(counts_with_grants(0))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(log.contains("project predates grants"), "{log}");
    assert!(
        log.contains("parsed 0 grants and the seeder sent 3"),
        "{log}"
    );
}

/// Red 7: an iam that predates `SyncTeams` answers `UNIMPLEMENTED`.
#[tokio::test(flavor = "multi_thread")]
async fn red_07_an_iam_answering_unimplemented_fails_and_sends_no_syncprojects() {
    let rig = rig(
        TEAMS,
        vec![Err(Status::unimplemented("no such method"))],
        vec![],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(log.contains("Unimplemented"), "{log}");
    // SAID CLEARLY: which release is missing, and that no grant moved. iam
    // relays iam-db's code unchanged, so an old iam-db reads the same.
    assert!(log.contains("predates SyncTeams"), "{log}");
    assert!(log.contains("iam-db"), "{log}");
    assert!(log.contains("no grant was synced"), "{log}");
    assert_eq!(
        rig.order.calls(),
        ["SyncTeams"],
        "SyncProjects must not be sent"
    );
    assert!(rig.project.received().is_empty());
}

/// Red 8: one unknown member. Every other member and grant still converge, so
/// `SyncProjects` is sent, and the run then exits non-zero naming the uuid.
#[tokio::test(flavor = "multi_thread")]
async fn red_08_an_unknown_member_still_sends_syncprojects_then_fails_naming_it() {
    let ghost = format!("yadgar:user:{RUNNER}");
    let rig = rig(
        TEAMS,
        vec![Ok(answer(0, 0, &[&ghost], &[], 1))],
        vec![Ok(counts_with_grants(GRANTS))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert_eq!(rig.order.calls(), ["SyncTeams", "SyncProjects"]);
    assert!(log.contains(&ghost), "{log}");
}

/// Red 9: F2's expected first sync. A declared administrator with no user row is
/// pending while `administrators` has no live member: exit 0, and the log names
/// the uuid and says the bootstrap `create-user` is expected next.
#[tokio::test(flavor = "multi_thread")]
async fn red_09_a_pending_administrator_with_no_live_member_exits_zero_and_says_why() {
    let pending = format!("yadgar:user:{ADMIN}");
    let rig = rig(
        TEAMS,
        vec![Ok(answer(2, 1, &[], &[&pending], 0))],
        vec![Ok(counts_with_grants(GRANTS))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(out.status.success(), "{log}");
    assert!(log.contains(&pending), "{log}");
    assert!(
        log.contains("bootstrap create-user is expected next"),
        "{log}"
    );
}

/// Red 10: the same pending entry beside a live administrator is a store defect.
#[tokio::test(flavor = "multi_thread")]
async fn red_10_a_pending_administrator_beside_a_live_one_exits_non_zero_naming_it() {
    let pending = format!("yadgar:user:{ADMIN}");
    let rig = rig(
        TEAMS,
        vec![Ok(answer(0, 0, &[], &[&pending], 1))],
        vec![Ok(counts_with_grants(GRANTS))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(log.contains(&pending), "{log}");
    assert!(log.contains("store defect"), "{log}");
}

/// Red 14: the lockout guard. The fake project asserts zero `SyncProjects`
/// calls; the log names the refusal and says no grant was synced.
#[tokio::test(flavor = "multi_thread")]
async fn red_14_a_lockout_guard_refusal_sends_no_syncprojects_and_says_so() {
    let rig = rig(
        TEAMS,
        vec![Err(Status::failed_precondition(
            "the sync would leave administrators with no member",
        ))],
        vec![],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(rig.project.received().is_empty(), "zero SyncProjects calls");
    assert_eq!(rig.iam.received().len(), 1, "a refusal is not retried");
    assert!(log.contains("FailedPrecondition"), "{log}");
    assert!(
        log.contains("the sync would leave administrators with no member"),
        "{log}"
    );
    assert!(log.contains("no grant was synced"), "{log}");
}

/// `UNAVAILABLE` from iam is retried like `project`'s, with the same request.
#[tokio::test(flavor = "multi_thread")]
async fn an_unavailable_iam_is_retried_then_the_run_lands() {
    let rig = rig(
        TEAMS,
        vec![
            Err(Status::unavailable("iam-db is not ready")),
            Ok(clean(2, 2)),
        ],
        vec![Ok(counts_with_grants(GRANTS))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    assert!(out.status.success(), "{}", text(&out));
    let seen = rig.iam.received();
    assert_eq!(seen.len(), 2);
    let id = |i: usize| seen[i].request.scope.as_ref().unwrap().request_id.clone();
    assert_eq!(id(0), id(1));
    assert!(
        text(&out).contains("retrying the team sync"),
        "{}",
        text(&out)
    );
}

/// A `project` refusal after a green `SyncTeams` says the teams were applied, so
/// the operator knows the stores are out of step until the next green sync.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_refusal_after_green_teams_says_the_teams_were_applied() {
    let rig = rig(
        TEAMS,
        vec![Ok(clean(2, 2))],
        vec![Err(Status::failed_precondition(
            "would delete 9 of 10 grants",
        ))],
    )
    .await;

    let out = run(rig.vars.clone(), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(log.contains("SyncTeams was applied"), "{log}");
    assert!(log.contains("would delete 9 of 10 grants"), "{log}");
}

/// Teams declared and no `IAM_*` block: refused before any dial, naming the
/// chart keys. Skipping `SyncTeams` would sync grants for teams iam lacks.
#[tokio::test(flavor = "multi_thread")]
async fn teams_declared_without_iam_configuration_is_refused_before_any_dial() {
    let pki = Pki::new();
    let project = Fake::answering(vec![]);
    let port = serve(&pki, project.clone()).await;
    let scratch = Scratch::new();

    let out = run(env(&scratch, &pki, port, "seeder-caller", TEAMS), SYNC).await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(log.contains("IAM_HOST"), "{log}");
    assert!(log.contains("iam.tls.caSecret"), "{log}");
    assert!(project.received().is_empty());
}

/// Red 1 to 4, 12 and 13 at the binary: a refused declaration dials nothing, on
/// either upstream.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_team_declaration_dials_neither_upstream() {
    let poison = TEAMS.replace(r#""name": "administrators""#, r#""name": "admins""#);
    let rig = rig(&poison, vec![], vec![]).await;

    let out = run(rig.vars.clone(), SYNC).await;

    assert!(!out.status.success(), "{}", text(&out));
    assert!(text(&out).contains("registry.teams"), "{}", text(&out));
    assert!(rig.order.calls().is_empty(), "nothing dialled");
    drop(rig.scratch);
}
