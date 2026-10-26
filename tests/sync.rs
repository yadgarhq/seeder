//! The real binary against a fake `ProjectService` over real mutual TLS
//! (plans/the-d35-seeder.md stage 5, tests 1 to 4, and the identity check).

mod support;

use support::iam::{serve_iam, FakeIam, Order};
use support::{
    counts_with_grants, env_iam_as, free_port, run, serve, serve_at, text, Fake, Pki, Scratch,
};
use tonic::Status;

/// What `project` answers once it parses the one grant `REGISTRY` carries.
fn counts(
    inserted: u32,
    updated: u32,
    archived: u32,
    reactivated: u32,
    unchanged: u32,
) -> yadgar_seeder::pb::yadgar::project::v1::ProjectServiceSyncProjectsResponse {
    yadgar_seeder::pb::yadgar::project::v1::ProjectServiceSyncProjectsResponse {
        inserted,
        updated,
        archived,
        reactivated,
        unchanged,
        ..counts_with_grants(1)
    }
}

/// The environment the chart renders: every declaration names teams (stage 12),
/// so the iam fake is up beside `project`, answering cleanly.
async fn env(
    scratch: &Scratch,
    pki: &Pki,
    port: u16,
    common_name: &str,
    registry: &str,
) -> Vec<(String, String)> {
    let iam = FakeIam::answering(vec![], &Order::default());
    let iam_port = serve_iam(pki, iam).await;
    env_iam_as(scratch, pki, port, iam_port, common_name, registry)
}

/// One root, two projects, the guard on.
const REGISTRY: &str = r#"{
  "roots": [{"path": "yadgarhq", "displayName": "", "sourceRepo": "yadgarhq/argocd",
    "grants": [{"team": "administrators", "level": "readWrite"}]}],
  "projects": [
    {"path": "yadgarhq/gateway", "displayName": "gateway", "sourceRepo": "yadgarhq/gateway"},
    {"path": "yadgarhq/project-db", "displayName": "", "sourceRepo": "yadgarhq/project-db"}
  ],
  "teams": [
    {"name": "administrators", "members": ["0192f3c4-aaaa-7bbb-8ccc-0123456789ab"]}
  ],
  "allowMassArchive": false,
  "allowMassRevoke": false
}"#;

const SYNC: &[&str] = &["sync-projects"];

/// Stage 5, test 2: ONE `SyncProjects`, carrying the file's roots and projects
/// exactly, from the seeder's leaf, under the seeder's principal.
///
/// **BOTH IDENTITY STRINGS ARE PINNED BY LITERAL HERE**, not through the
/// crate's constants: `project` declares the same two strings in its own
/// repository, and a constant changed on this side alone must turn this red.
#[tokio::test(flavor = "multi_thread")]
async fn one_sync_carries_the_declaration_exactly_under_the_seeder_s_identity() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![Ok(counts(3, 0, 0, 0, 0))]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();

    let out = run(
        env(&scratch, &pki, port, "seeder-caller", REGISTRY).await,
        SYNC,
    )
    .await;

    assert!(
        out.status.success(),
        "the run must succeed:\n{}",
        text(&out)
    );
    let received = fake.received();
    assert_eq!(received.len(), 1, "exactly one call");
    let call = &received[0];
    assert_eq!(call.caller.as_deref(), Some("seeder-caller"));

    let req = &call.request;
    let scope = req
        .scope
        .as_ref()
        .expect("the seeder sends a Scope (ADR-0942)");
    assert_eq!(scope.user_id, "yadgar:system:seeder");
    assert!(
        uuid_v7(&scope.request_id),
        "request_id must be a minted UUIDv7, got {:?}",
        scope.request_id
    );
    assert_eq!(scope.project_id, "");
    assert!(scope.team_ids.is_empty());
    assert_eq!(scope.instance_id, "");
    assert!(scope.owner_reads_own_record.is_none());

    let rows = |list: &[yadgar_seeder::pb::yadgar::project::v1::DeclaredProject]| {
        list.iter()
            .map(|d| {
                (
                    d.path.clone(),
                    d.display_name.clone(),
                    d.source_repo.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&req.roots),
        [("yadgarhq".into(), "".into(), "yadgarhq/argocd".into())]
    );
    assert_eq!(
        rows(&req.projects),
        [
            (
                "yadgarhq/gateway".into(),
                "gateway".into(),
                "yadgarhq/gateway".into()
            ),
            (
                "yadgarhq/project-db".into(),
                "".into(),
                "yadgarhq/project-db".into()
            ),
        ]
    );
    assert!(!req.allow_mass_archive);

    // The five counts reach the Job log, with the request_id that joins them.
    let log = text(&out);
    assert!(log.contains("\"sync applied\""), "log:\n{log}");
    assert!(log.contains("\"inserted\":3"), "log:\n{log}");
    assert!(log.contains(&scope.request_id), "log:\n{log}");
}

/// ADR-0939: the flag reaches the wire exactly as the declaration states it.
#[tokio::test(flavor = "multi_thread")]
async fn the_mass_archive_flag_reaches_the_wire_as_declared() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();
    let registry = REGISTRY.replace(
        r#""allowMassArchive": false"#,
        r#""allowMassArchive": true"#,
    );

    let out = run(
        env(&scratch, &pki, port, "seeder-caller", &registry).await,
        SYNC,
    )
    .await;

    assert!(out.status.success(), "{}", text(&out));
    assert!(fake.received()[0].request.allow_mass_archive);
}

/// Stage 5, test 1: a malformed registry exits non-zero WITHOUT DIALLING.
#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_registry_exits_non_zero_without_dialling() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();
    let poison = REGISTRY.replace("\"path\": \"yadgarhq\"", "\"path\": \"yadgarhq/two\"");

    let out = run(
        env(&scratch, &pki, port, "seeder-caller", &poison).await,
        SYNC,
    )
    .await;

    assert!(!out.status.success(), "must refuse:\n{}", text(&out));
    assert!(text(&out).contains("\"yadgarhq/two\""), "{}", text(&out));
    assert!(fake.received().is_empty(), "no call may be made");
}

/// Not JSON at all: the same outcome.
#[tokio::test(flavor = "multi_thread")]
async fn a_registry_that_is_not_json_exits_non_zero_without_dialling() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();

    let out = run(
        env(&scratch, &pki, port, "seeder-caller", "{ roots: [").await,
        SYNC,
    )
    .await;

    assert!(!out.status.success());
    assert!(
        text(&out).contains("not a valid registry document"),
        "{}",
        text(&out)
    );
    assert!(fake.received().is_empty());
}

/// Stage 5, test 3: a refusal relayed by `project` exits non-zero, logs the
/// code and the message, and is NOT retried — one call per refusal code.
#[tokio::test(flavor = "multi_thread")]
async fn every_refusal_exits_non_zero_after_one_call_and_logs_code_and_message() {
    let cases = [
        (
            Status::invalid_argument("the root \"a/b\" has more than one segment"),
            "InvalidArgument",
        ),
        (
            Status::already_exists("\"yadgarhq/old\" is an alias of another project"),
            "AlreadyExists",
        ),
        (
            Status::failed_precondition("would archive 3 of 4 active org rows; allow_mass_archive"),
            "FailedPrecondition",
        ),
        (
            Status::permission_denied("scope.user_id is not the seeder principal"),
            "PermissionDenied",
        ),
    ];
    for (status, code) in cases {
        let message = status.message().to_string();
        let pki = Pki::new();
        let fake = Fake::answering(vec![Err(status)]);
        let port = serve(&pki, fake.clone()).await;
        let scratch = Scratch::new();

        let out = run(
            env(&scratch, &pki, port, "seeder-caller", REGISTRY).await,
            SYNC,
        )
        .await;

        let log = text(&out);
        assert!(!out.status.success(), "{code} must fail the run:\n{log}");
        assert_eq!(fake.received().len(), 1, "{code} must not be retried");
        assert!(log.contains(code), "the log must carry {code}:\n{log}");
        let escaped = message.replace('"', "\\\"");
        assert!(
            log.contains(&message) || log.contains(&escaped),
            "the log must carry the relayed message {message:?}:\n{log}"
        );
    }
}

/// Stage 5, test 4: `UNAVAILABLE` is retried, then the sync lands — with the
/// SAME request_id on every attempt (ADR-0942: one sync, one id).
#[tokio::test(flavor = "multi_thread")]
async fn unavailable_is_retried_with_the_same_request_id_then_succeeds() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![
        Err(Status::unavailable("project-db is not ready")),
        Ok(counts(0, 0, 0, 0, 3)),
    ]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();

    let out = run(
        env(&scratch, &pki, port, "seeder-caller", REGISTRY).await,
        SYNC,
    )
    .await;

    assert!(out.status.success(), "{}", text(&out));
    let received = fake.received();
    assert_eq!(received.len(), 2, "one retry");
    let id = |i: usize| {
        received[i]
            .request
            .scope
            .as_ref()
            .unwrap()
            .request_id
            .clone()
    };
    assert_eq!(id(0), id(1), "one sync carries one request_id");
    assert!(text(&out).contains("retrying the sync"), "{}", text(&out));
}

/// Stage 5, test 4 AS THE PLAN MEANS IT: `project` is not serving when the
/// run starts — nothing listens on its port — and comes up a few seconds later.
/// The first attempt fails on the transport, the run retries, and the sync
/// lands. What code the transport failure surfaces as is printed in the log.
#[tokio::test(flavor = "multi_thread")]
async fn a_project_that_is_not_serving_yet_is_retried_until_it_is() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![Ok(counts(1, 0, 0, 0, 0))]);
    let port = free_port().await;
    let scratch = Scratch::new();
    let vars = env(&scratch, &pki, port, "seeder-caller", REGISTRY).await;

    let running = tokio::spawn(run(vars, SYNC));
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    serve_at(&pki, fake.clone(), port).await;
    let out = running.await.unwrap();

    let log = text(&out);
    assert!(
        out.status.success(),
        "the run must land once project serves:\n{log}"
    );
    assert!(
        log.contains("retrying the sync"),
        "the first attempt must have failed:\n{log}"
    );
    assert_eq!(fake.received().len(), 1, "one call reached project");
}

/// The retry is bounded: `UNAVAILABLE` on every attempt exits non-zero after
/// exactly `SEEDER_SYNC_ATTEMPTS` calls.
#[tokio::test(flavor = "multi_thread")]
async fn unavailable_on_every_attempt_exits_non_zero_after_the_bound() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![
        Err(Status::unavailable("down")),
        Err(Status::unavailable("still down")),
    ]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();
    let mut vars = env(&scratch, &pki, port, "seeder-caller", REGISTRY).await;
    for (k, v) in vars.iter_mut() {
        if k == "SEEDER_SYNC_ATTEMPTS" {
            *v = "2".to_string();
        }
    }

    let out = run(vars, SYNC).await;

    assert!(!out.status.success(), "{}", text(&out));
    assert_eq!(fake.received().len(), 2);
    assert!(text(&out).contains("still down"), "{}", text(&out));
}

/// A client leaf that is not the seeder's (here `project`'s own) is refused at
/// boot, before any dial, naming the Secret to fix. Over the wire it would
/// handshake and then be refused PERMISSION_DENIED.
#[tokio::test(flavor = "multi_thread")]
async fn another_module_s_leaf_is_refused_before_any_dial() {
    let pki = Pki::new();
    let fake = Fake::answering(vec![]);
    let port = serve(&pki, fake.clone()).await;
    let scratch = Scratch::new();

    let out = run(
        env(&scratch, &pki, port, "project-caller", REGISTRY).await,
        SYNC,
    )
    .await;

    let log = text(&out);
    assert!(!out.status.success(), "{log}");
    assert!(
        log.contains("\\\"project-caller\\\"") || log.contains("\"project-caller\""),
        "{log}"
    );
    assert!(log.contains("seeder-client-tls"), "{log}");
    assert!(fake.received().is_empty());
}

/// No subcommand, or any other, is a usage refusal: the Job names the
/// subcommand, so a bare run does nothing rather than guessing one.
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_or_unknown_subcommand_is_refused_without_dialling() {
    for args in [&[][..], &["sync"][..], &["sync-projects", "extra"][..]] {
        let pki = Pki::new();
        let fake = Fake::answering(vec![]);
        let port = serve(&pki, fake.clone()).await;
        let scratch = Scratch::new();

        let out = run(
            env(&scratch, &pki, port, "seeder-caller", REGISTRY).await,
            args,
        )
        .await;

        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(text(&out).contains("usage"), "{}", text(&out));
        assert!(fake.received().is_empty());
    }
}

/// A canonical UUIDv7 string: 8-4-4-4-12 hex with version nibble 7.
fn uuid_v7(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
        && parts[2].starts_with('7')
}
