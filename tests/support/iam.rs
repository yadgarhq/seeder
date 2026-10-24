//! A fake `IamService` over the same real loopback mutual TLS as the fake
//! `ProjectService` (`super`), and the shared call log that lets a test assert
//! WHICH call came first.
//!
//! **THE FAKE ADMITS THE WAY `iam` DOES FOR `SyncTeams`**: the verified client
//! leaf must name `seeder-caller`, or the call is refused `PERMISSION_DENIED`.

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status};

use super::Pki;
use yadgar_seeder::pb::yadgar::iam::v1 as pb;
use yadgar_seeder::pb::yadgar::iam::v1::iam_service_server::{IamService, IamServiceServer};

/// The order calls arrived in, across both fakes: `"SyncTeams"` and
/// `"SyncProjects"`, one entry per ADMITTED or refused call.
#[derive(Clone, Default)]
pub struct Order(Arc<Mutex<Vec<&'static str>>>);

impl Order {
    pub fn push(&self, call: &'static str) {
        self.0.lock().unwrap().push(call);
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}

/// One `SyncTeams` the fake received.
#[derive(Clone, Debug)]
pub struct ReceivedTeams {
    pub caller: Option<String>,
    pub request: pb::SyncTeamsRequest,
}

/// The fake's script and its record.
#[derive(Clone, Default)]
pub struct FakeIam {
    script: Arc<Mutex<Vec<Result<pb::SyncTeamsResponse, Status>>>>,
    received: Arc<Mutex<Vec<ReceivedTeams>>>,
    order: Order,
}

impl FakeIam {
    pub fn answering(script: Vec<Result<pb::SyncTeamsResponse, Status>>, order: &Order) -> Self {
        Self {
            script: Arc::new(Mutex::new(script)),
            received: Arc::default(),
            order: order.clone(),
        }
    }

    pub fn received(&self) -> Vec<ReceivedTeams> {
        self.received.lock().unwrap().clone()
    }
}

/// A `SyncTeamsResponse` that reports nothing wrong: no unknown member, no
/// pending administrator, one live administrator.
pub fn clean(teams_inserted: u32, members_added: u32) -> pb::SyncTeamsResponse {
    answer(teams_inserted, members_added, &[], &[], 1)
}

pub fn answer(
    teams_inserted: u32,
    members_added: u32,
    unknown: &[&str],
    pending: &[&str],
    administrators_live_members: u32,
) -> pb::SyncTeamsResponse {
    pb::SyncTeamsResponse {
        teams_inserted,
        teams_archived: 0,
        teams_reactivated: 0,
        members_added,
        members_removed: 0,
        unknown_member_user_ids: unknown.iter().map(|s| s.to_string()).collect(),
        pending_administrator_user_ids: pending.iter().map(|s| s.to_string()).collect(),
        administrators_live_members,
    }
}

#[tonic::async_trait]
impl IamService for FakeIam {
    async fn sync_teams(
        &self,
        request: Request<pb::SyncTeamsRequest>,
    ) -> Result<Response<pb::SyncTeamsResponse>, Status> {
        self.order.push("SyncTeams");
        let caller = yadgar_lifecycle::serve_tls::caller_common_name(&request);
        self.received.lock().unwrap().push(ReceivedTeams {
            caller: caller.clone(),
            request: request.into_inner(),
        });
        if caller.as_deref() != Some("seeder-caller") {
            return Err(Status::permission_denied(format!(
                "fake iam: caller {caller:?} is not the seeder"
            )));
        }
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            return Ok(Response::new(clean(0, 0)));
        }
        script.remove(0).map(Response::new)
    }

    async fn resolve_credential(
        &self,
        _: Request<pb::ResolveCredentialRequest>,
    ) -> Result<Response<pb::ResolveCredentialResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn login(
        &self,
        _: Request<pb::LoginRequest>,
    ) -> Result<Response<pb::LoginResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn redeem_enrolment(
        &self,
        _: Request<pb::RedeemEnrolmentRequest>,
    ) -> Result<Response<pb::RedeemEnrolmentResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn issue_credential(
        &self,
        _: Request<pb::IssueCredentialRequest>,
    ) -> Result<Response<pb::IssueCredentialResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn revoke_credential(
        &self,
        _: Request<pb::RevokeCredentialRequest>,
    ) -> Result<Response<pb::RevokeCredentialResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn revoke_all_credentials(
        &self,
        _: Request<pb::RevokeAllCredentialsRequest>,
    ) -> Result<Response<pb::RevokeAllCredentialsResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn list_credentials(
        &self,
        _: Request<pb::ListCredentialsRequest>,
    ) -> Result<Response<pb::ListCredentialsResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn issue_enrolment(
        &self,
        _: Request<pb::IssueEnrolmentRequest>,
    ) -> Result<Response<pb::IssueEnrolmentResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn create_user(
        &self,
        _: Request<pb::CreateUserRequest>,
    ) -> Result<Response<pb::CreateUserResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn set_user_admin(
        &self,
        _: Request<pb::SetUserAdminRequest>,
    ) -> Result<Response<pb::SetUserAdminResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn set_rate_limit_override(
        &self,
        _: Request<pb::SetRateLimitOverrideRequest>,
    ) -> Result<Response<pb::SetRateLimitOverrideResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn add_team_member(
        &self,
        _: Request<pb::AddTeamMemberRequest>,
    ) -> Result<Response<pb::AddTeamMemberResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn remove_team_member(
        &self,
        _: Request<pb::RemoveTeamMemberRequest>,
    ) -> Result<Response<pb::RemoveTeamMemberResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }

    async fn set_inherited_setting(
        &self,
        _: Request<pb::SetInheritedSettingRequest>,
    ) -> Result<Response<pb::SetInheritedSettingResponse>, Status> {
        Err(Status::unimplemented(
            "the seeder only ever calls SyncTeams",
        ))
    }
}

/// Serve `fake` under REQUIRED client authentication. Returns the port.
pub async fn serve_iam(pki: &Pki, fake: FakeIam) -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(&pki.server_cert, &pki.server_key))
        .client_ca_root(Certificate::from_pem(pki.ca_pem()));
    let router = Server::builder()
        .tls_config(tls)
        .unwrap()
        .add_service(IamServiceServer::new(fake));
    tokio::spawn(async move {
        let _ = router
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await;
    });
    port
}
