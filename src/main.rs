//! `yadgar-seeder sync-projects`: one sync, then exit.
//!
//! **THE ORDER IS THE BEHAVIOUR.** Every input is read and checked before the
//! dial: the knobs, the client leaf's identity, and the whole declaration. A
//! malformed declaration therefore exits non-zero without a connection ever
//! being opened (plans/the-d35-seeder.md stage 5, test 1). Only then is anything
//! dialled, the request_id minted, and the calls sent.
//!
//! **TEAMS FIRST, THEN PROJECTS (plans/team-access.md stage 6).** When the
//! declaration names teams, `iam` gets `SyncTeams` and `project` gets
//! `SyncProjects` only after `SyncTeams` succeeded. A refused `SyncTeams` ends
//! the run: sending the new grants beside the old member set can admit a caller
//! neither declaration admits. When the declaration names no team (the
//! transition rule) there is no `SyncTeams`, and the run is what it was before
//! team access.
//!
//! **EXIT 0 ONLY WHEN THE STORES APPLIED THE DECLARATION AND THE COUNTS AGREE.**
//! Every refusal, an exhausted retry, every configuration error, and an applied
//! sync whose counts say something is wrong (`crate::outcome`) exit 1, so the
//! Job is `Failed`, the Application reports Degraded, and the ADR-0831
//! cold-install proof fails on it. Nothing is silent.

use std::process::ExitCode;

use yadgar_seeder::config::{Config, Iam};
use yadgar_seeder::pb::yadgar::iam::v1::SyncTeamsResponse;
use yadgar_seeder::registry::Declaration;
use yadgar_seeder::{identity, outcome, registry, sync, sync_teams, upstream};

/// The one subcommand. Named, so a later content-seeding arm (ledger 973) is a
/// second subcommand of the same binary rather than a flag on this one.
const SYNC_PROJECTS: &str = "sync-projects";

#[tokio::main]
async fn main() -> ExitCode {
    install_logging();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "the sync did not complete");
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn install_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info") // ADR-0569-EXCEPTION(LIB): the log level is observability, not behaviour; RUST_LOG unset must still produce visible logs rather than none
            }),
        )
        .init();
}

async fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 || args[0] != SYNC_PROJECTS {
        return Err(format!(
            "usage: yadgar-seeder {SYNC_PROJECTS}. Got {args:?}. The chart's Job passes the \
             subcommand; every knob comes from the environment"
        ));
    }

    let config = Config::from_env().map_err(|e| e.to_string())?;
    identity::check_client_leaf(&config.tls.client_cert_file).map_err(|e| e.to_string())?;
    // ADR-0523-UNWATCHED: the declaration is read once per run; a changed ConfigMap is a new Job (ADR-0940).
    let declaration = registry::read(&config.registry_file).map_err(|e| e.to_string())?;
    let iam = iam_for(&declaration, &config)?;
    let grants_sent = grants_sent(&declaration);
    tracing::info!(
        roots = declaration.roots.len(),
        projects = declaration.projects.len(),
        teams = declaration.teams.len(),
        grants = grants_sent,
        allow_mass_archive = declaration.allow_mass_archive,
        allow_mass_revoke = declaration.allow_mass_revoke,
        "declaration read and checked"
    );

    // ONE RUN IS ONE SYNC: one id on both calls (ADR-0942).
    let request_id = sync::mint_request_id();

    let teams = match iam {
        None => {
            tracing::info!("no team is declared; SyncTeams is not called (the transition rule)");
            None
        }
        Some(iam) => Some(sync_teams(&config, iam, &declaration, &request_id).await?),
    };

    let channel = upstream::connect(&config)
        .await
        .map_err(|e| yadgar_telemetry::diagnose::chain(&e))?;
    let request = sync::request(&declaration, &request_id);
    tracing::info!(
        %request_id,
        host = %config.project_host,
        port = config.project_port,
        request_timeout_secs = config.request_timeout.as_secs(),
        attempts = config.retry.attempts,
        "sending SyncProjects to project"
    );
    let counts = match sync::send(channel, request, config.retry).await {
        Ok(counts) => counts,
        Err(e) => {
            tracing::error!(
                %request_id,
                code = ?e.code(),
                message = e.message(),
                teams_applied = teams.is_some(),
                "project did not apply the sync"
            );
            return Err(match teams {
                None => e.to_string(),
                Some(_) => format!(
                    "{e}. SyncTeams was applied, so the new members now pair with the old \
                     grants until the next green sync"
                ),
            });
        }
    };
    tracing::info!(
        %request_id,
        inserted = counts.inserted,
        updated = counts.updated,
        archived = counts.archived,
        reactivated = counts.reactivated,
        unchanged = counts.unchanged,
        grants_declared = counts.grants_declared,
        grants_inserted = counts.grants_inserted,
        grants_deleted = counts.grants_deleted,
        grants_unchanged = counts.grants_unchanged,
        "sync applied"
    );

    let judgement = outcome::judge(grants_sent, teams.as_ref(), &counts);
    for id in &judgement.pending {
        tracing::info!(
            %request_id,
            pending_administrator = %id,
            "the declared administrator has no user yet: the bootstrap create-user is expected \
             next (ADR-0963 (5), flow F2)"
        );
    }
    if judgement.failures.is_empty() {
        Ok(())
    } else {
        Err(judgement.failures.join("; "))
    }
}

/// The iam transport the declaration needs: `None` under the transition rule, a
/// refusal naming the `iam.*` chart keys when teams are declared and the chart
/// rendered no `IAM_*` block.
fn iam_for<'a>(declaration: &Declaration, config: &'a Config) -> Result<Option<&'a Iam>, String> {
    if declaration.teams.is_empty() {
        return Ok(None);
    }
    config.iam.as_ref().map(Some).ok_or_else(|| {
        "registry.teams is declared, and IAM_HOST is NOT SET. The chart renders the IAM_* \
         variables from iam.host, iam.port, iam.requestTimeoutSeconds and iam.tls.caSecret when \
         teams are declared; this run refuses rather than skip SyncTeams, which would sync \
         grants for teams iam does not hold"
            .to_string()
    })
}

/// The number of grants on the wire, the figure `project` must echo back.
fn grants_sent(declaration: &Declaration) -> u32 {
    let n: usize = declaration
        .roots
        .iter()
        .chain(&declaration.projects)
        .map(|p| p.grants.len())
        .sum();
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The first call of a run that declares teams.
async fn sync_teams(
    config: &Config,
    iam: &Iam,
    declaration: &Declaration,
    request_id: &str,
) -> Result<SyncTeamsResponse, String> {
    let channel = upstream::connect_iam(config, iam)
        .await
        .map_err(|e| yadgar_telemetry::diagnose::chain(&e))?;
    tracing::info!(
        %request_id,
        host = %iam.host,
        port = iam.port,
        request_timeout_secs = iam.request_timeout.as_secs(),
        attempts = config.retry.attempts,
        teams = declaration.teams.len(),
        "sending SyncTeams to iam"
    );
    let request = sync_teams::request(declaration, request_id);
    match sync_teams::send(channel, request, config.retry).await {
        Ok(counts) => {
            tracing::info!(
                %request_id,
                teams_inserted = counts.teams_inserted,
                teams_archived = counts.teams_archived,
                teams_reactivated = counts.teams_reactivated,
                members_added = counts.members_added,
                members_removed = counts.members_removed,
                unknown_member_user_ids = ?counts.unknown_member_user_ids,
                pending_administrator_user_ids = ?counts.pending_administrator_user_ids,
                administrators_live_members = counts.administrators_live_members,
                "teams applied"
            );
            Ok(counts)
        }
        Err(e) => {
            tracing::error!(
                %request_id,
                code = ?e.code(),
                message = e.message(),
                "iam did not apply the team sync; SyncProjects is not sent, so no grant was synced"
            );
            Err(e.to_string())
        }
    }
}
