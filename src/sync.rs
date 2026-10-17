//! The one call: the whole declaration, sent to `project` as ONE
//! `ProjectService.SyncProjects` (ADR-0936), retried only while `project`
//! answers `UNAVAILABLE`.
//!
//! **ONE RUN IS ONE SYNC, AND IT IS IDEMPOTENT BY CONVERGENCE.** The store
//! applies the declaration in one transaction and a second identical sync
//! changes nothing, so a retry after an attempt whose outcome is unknown (a
//! timeout after the commit) is safe: it reports every row `unchanged`.
//!
//! **ANY REFUSAL ENDS THE RUN.** `INVALID_ARGUMENT`, `ALREADY_EXISTS`,
//! `FAILED_PRECONDITION` (the mass-archive guard, ADR-0939), `PERMISSION_DENIED`
//! and every other code except `UNAVAILABLE` would get the same answer again:
//! the declaration or the deployment has to change. The run logs the code and
//! the message `project` relays and exits non-zero, which leaves the Job
//! `Failed` and the Application Degraded.

use tonic::transport::Channel;
use tonic::{Code, Status};

use crate::config::Retry;
use crate::identity::PRINCIPAL;
use crate::pb::yadgar::common::v1::Scope;
use crate::pb::yadgar::project::v1::project_service_client::ProjectServiceClient;
use crate::pb::yadgar::project::v1::{
    ProjectServiceSyncProjectsRequest, ProjectServiceSyncProjectsResponse,
};
use crate::registry::Declaration;

/// The request for one sync.
///
/// **EVERY FIELD IS SET BY NAME AND NONE BY `..Default::default()`.** The
/// seeder vendors the contract at the SAME tag `project` vendors
/// (`PROTO_VERSION`). A field the seeder set that `project` did not vendor
/// would be dropped by `project`'s prost decoder SILENTLY, as an unknown field,
/// so the seeder sets nothing `project` does not know: no struct update syntax
/// here, so a field added to the contract fails to compile until someone
/// decides it, and the vendored tag moves only together with `project`'s.
pub fn request(declaration: &Declaration, request_id: &str) -> ProjectServiceSyncProjectsRequest {
    ProjectServiceSyncProjectsRequest {
        scope: Some(scope(request_id)),
        roots: declaration.roots.clone(),
        projects: declaration.projects.clone(),
        allow_mass_archive: declaration.allow_mass_archive,
    }
}

/// The seeder's own Scope (ADR-0942). The ONE path in the estate where a Scope
/// is built outside the gateway's `attest()`.
///
/// - `user_id` is the fixed system principal; `project` refuses anything else.
/// - `request_id` is minted by the seeder once per sync and relayed unchanged
///   by `project`, so D67 records join across both hops.
/// - `project_id`, `team_ids` and `instance_id` are empty and ignored. They are
///   still named, so a reader sees they were decided rather than forgotten.
/// - `owner_reads_own_record` (the last field `common.proto` declares at the
///   vendored tag) is absent: it is ADR-0522's record-read policy, and a sync
///   reads no record. The store's sync never consults it.
fn scope(request_id: &str) -> Scope {
    Scope {
        user_id: PRINCIPAL.to_string(),
        project_id: String::new(),
        team_ids: Vec::new(),
        instance_id: String::new(),
        request_id: request_id.to_string(),
        owner_reads_own_record: None,
    }
}

/// A fresh `request_id` for one sync: a UUIDv7, as the gateway mints.
pub fn mint_request_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Why the sync did not complete.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(
        "project refused the sync with {code:?}: {message}. A refusal is not retried: the same \
         declaration gets the same answer"
    )]
    Refused { code: Code, message: String },

    #[error(
        "project stayed UNAVAILABLE for all {attempts} attempts (sync.attempts); the last \
         answer was: {message}"
    )]
    Unavailable { attempts: u32, message: String },
}

impl SyncError {
    /// The gRPC code of the last answer, for the log line.
    pub fn code(&self) -> Code {
        match self {
            Self::Refused { code, .. } => *code,
            Self::Unavailable { .. } => Code::Unavailable,
        }
    }

    /// The message `project` relayed with it.
    pub fn message(&self) -> &str {
        match self {
            Self::Refused { message, .. } | Self::Unavailable { message, .. } => message,
        }
    }
}

/// Send `request`, retrying only `UNAVAILABLE`, at most `retry.attempts` times.
///
/// The SAME request goes on every attempt, `request_id` included: one sync is
/// one id (ADR-0942), however many attempts it takes to land.
pub async fn send(
    channel: Channel,
    request: ProjectServiceSyncProjectsRequest,
    retry: Retry,
) -> Result<ProjectServiceSyncProjectsResponse, SyncError> {
    let mut client = ProjectServiceClient::new(channel);
    let mut attempt = 1;
    loop {
        match client.sync_projects(request.clone()).await {
            Ok(answer) => return Ok(answer.into_inner()),
            Err(status) => match next(&status, attempt, retry) {
                Next::Retry => {
                    tracing::warn!(
                        attempt,
                        attempts = retry.attempts,
                        backoff_secs = retry.backoff.as_secs(),
                        code = ?status.code(),
                        message = status.message(),
                        "project is unavailable; retrying the sync"
                    );
                    tokio::time::sleep(retry.backoff).await;
                    attempt += 1;
                }
                Next::Stop(e) => return Err(e),
            },
        }
    }
}

enum Next {
    Retry,
    Stop(SyncError),
}

/// The retry decision, apart from the network so it can be read in one place.
fn next(status: &Status, attempt: u32, retry: Retry) -> Next {
    let message = status.message().to_string();
    match status.code() {
        Code::Unavailable if attempt < retry.attempts => Next::Retry,
        Code::Unavailable => Next::Stop(SyncError::Unavailable {
            attempts: retry.attempts,
            message,
        }),
        code => Next::Stop(SyncError::Refused { code, message }),
    }
}
