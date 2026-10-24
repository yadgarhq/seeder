//! The first call of a run that declares teams: the whole team declaration, sent
//! to `iam` as ONE `IamService.SyncTeams` (ADR-0963 (1)), retried only while
//! `iam` answers `UNAVAILABLE`.
//!
//! **IT RUNS BEFORE `SyncProjects`, AND A REFUSAL ENDS THE RUN.** Sending the
//! grants after a refused team sync would pair the new grants with the old
//! member set, which can admit a caller neither declaration admits
//! (plans/team-access.md §1). So any answer but `OK` ends the run non-zero with
//! no `SyncProjects` sent, the lockout guard's `FAILED_PRECONDITION` included,
//! and `UNAVAILABLE` is the one answer retried, as for `project`.
//!
//! **THE SAME `request_id`, THE SAME SCOPE.** One run is one sync (ADR-0942):
//! the Scope is the one `crate::sync` builds, and `iam` relays the id to
//! `iam-db` so D67 records join across the hops.

use tonic::transport::Channel;
use tonic::Code;

use crate::config::Retry;
use crate::pb::yadgar::iam::v1::iam_service_client::IamServiceClient;
use crate::pb::yadgar::iam::v1::{SyncTeamsRequest, SyncTeamsResponse};
use crate::registry::Declaration;
use crate::sync::{next, scope, Next, SyncError};

/// The request for one team sync. Every field is set by name and none by
/// `..Default::default()`, for the reason `crate::sync::request` gives: a field
/// `iam` did not vendor would be dropped by its decoder silently.
pub fn request(declaration: &Declaration, request_id: &str) -> SyncTeamsRequest {
    SyncTeamsRequest {
        scope: Some(scope(request_id)),
        teams: declaration.teams.clone(),
    }
}

/// Why the team sync did not complete.
#[derive(Debug, thiserror::Error)]
pub enum TeamsError {
    #[error(
        "iam refused SyncTeams with {code:?}: {message}. A refusal is not retried: the same \
         declaration gets the same answer. SyncProjects was NOT sent, so no grant was synced and \
         the old teams and the old grants stay together"
    )]
    Refused { code: Code, message: String },

    #[error(
        "iam answered UNIMPLEMENTED to SyncTeams: {message}. This iam, or the iam-db it relays \
         to, predates SyncTeams (team access stage 3 for iam, stage 2 for iam-db; iam relays \
         iam-db's code unchanged). Release both before declaring registry.teams. SyncProjects \
         was NOT sent, so no grant was synced and the old teams and the old grants stay together"
    )]
    Predates { message: String },

    #[error(
        "iam stayed UNAVAILABLE for all {attempts} attempts (sync.attempts); the last answer \
         was: {message}. SyncProjects was NOT sent, so no grant was synced"
    )]
    Unavailable { attempts: u32, message: String },
}

impl TeamsError {
    /// The gRPC code of the last answer, for the log line.
    pub fn code(&self) -> Code {
        match self {
            Self::Refused { code, .. } => *code,
            Self::Predates { .. } => Code::Unimplemented,
            Self::Unavailable { .. } => Code::Unavailable,
        }
    }

    /// The message `iam` relayed with it.
    pub fn message(&self) -> &str {
        match self {
            Self::Refused { message, .. }
            | Self::Predates { message }
            | Self::Unavailable { message, .. } => message,
        }
    }
}

/// Send `request`, retrying only `UNAVAILABLE`, at most `retry.attempts` times.
/// The SAME request goes on every attempt, `request_id` included.
pub async fn send(
    channel: Channel,
    request: SyncTeamsRequest,
    retry: Retry,
) -> Result<SyncTeamsResponse, TeamsError> {
    let mut client = IamServiceClient::new(channel);
    let mut attempt = 1;
    loop {
        match client.sync_teams(request.clone()).await {
            Ok(answer) => return Ok(answer.into_inner()),
            Err(status) => match next(&status, attempt, retry) {
                Next::Retry => {
                    tracing::warn!(
                        attempt,
                        attempts = retry.attempts,
                        backoff_secs = retry.backoff.as_secs(),
                        code = ?status.code(),
                        message = status.message(),
                        "iam is unavailable; retrying the team sync"
                    );
                    tokio::time::sleep(retry.backoff).await;
                    attempt += 1;
                }
                Next::Stop(e) => return Err(into_teams_error(e)),
            },
        }
    }
}

fn into_teams_error(e: SyncError) -> TeamsError {
    match e {
        SyncError::Refused {
            code: Code::Unimplemented,
            message,
        } => TeamsError::Predates { message },
        SyncError::Refused { code, message } => TeamsError::Refused { code, message },
        SyncError::Unavailable { attempts, message } => {
            TeamsError::Unavailable { attempts, message }
        }
    }
}
