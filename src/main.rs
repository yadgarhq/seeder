//! `yadgar-seeder sync-projects`: one sync, then exit.
//!
//! **THE ORDER IS THE BEHAVIOUR.** Every input is read and checked before the
//! dial: the knobs, the client leaf's identity, and the whole declaration. A
//! malformed declaration therefore exits non-zero without a connection ever
//! being opened (plans/the-d35-seeder.md stage 5, test 1). Only then is
//! `project` dialled, the request_id minted, and the one call sent.
//!
//! **EXIT 0 ONLY WHEN THE STORE APPLIED THE DECLARATION.** Every refusal, an
//! exhausted retry, and every configuration error exits 1, so the Job is
//! `Failed`, the Application reports Degraded, and the ADR-0831 cold-install
//! proof fails on it. Nothing is silent.

use std::process::ExitCode;

use yadgar_seeder::config::Config;
use yadgar_seeder::{identity, registry, sync, upstream};

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
    tracing::info!(
        roots = declaration.roots.len(),
        projects = declaration.projects.len(),
        allow_mass_archive = declaration.allow_mass_archive,
        "declaration read and checked"
    );

    let channel = upstream::connect(&config)
        .await
        .map_err(|e| yadgar_telemetry::diagnose::chain(&e))?;

    let request_id = sync::mint_request_id();
    let request = sync::request(&declaration, &request_id);
    tracing::info!(
        %request_id,
        host = %config.project_host,
        port = config.project_port,
        request_timeout_secs = config.request_timeout.as_secs(),
        attempts = config.retry.attempts,
        "sending SyncProjects to project"
    );
    match sync::send(channel, request, config.retry).await {
        Ok(counts) => {
            tracing::info!(
                %request_id,
                inserted = counts.inserted,
                updated = counts.updated,
                archived = counts.archived,
                reactivated = counts.reactivated,
                unchanged = counts.unchanged,
                "sync applied"
            );
            Ok(())
        }
        Err(e) => {
            tracing::error!(
                %request_id,
                code = ?e.code(),
                message = e.message(),
                "project did not apply the sync"
            );
            Err(e.to_string())
        }
    }
}
