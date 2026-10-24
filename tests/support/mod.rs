//! A fake `ProjectService` over REAL loopback mutual TLS, an authority minted
//! per run, and a way to run the real binary against them.
//!
//! **THE FAKE ADMITS THE WAY `project` DOES.** It reads the caller off the
//! verified client leaf with `yadgar_lifecycle::serve_tls::caller_common_name`,
//! the accessor `project`'s own caller check uses, and refuses
//! `PERMISSION_DENIED` unless it is the literal `"seeder-caller"`. So a test
//! that passes proves the binary presented the seeder's leaf over a handshake,
//! not that a stub was called.

#![allow(dead_code)]

pub mod iam;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status};

use yadgar_seeder::pb::yadgar::project::v1 as pb;
use yadgar_seeder::pb::yadgar::project::v1::project_service_server::{
    ProjectService, ProjectServiceServer,
};

pub const BIN: &str = env!("CARGO_BIN_EXE_yadgar-seeder");

/// The name `project`'s serving leaf carries; the binary verifies against it
/// through `PROJECT_TLS_DOMAIN`, because it dials 127.0.0.1.
pub const SERVED_NAME: &str = "project";

/// A fresh directory under the system temp dir, removed on drop. The name
/// carries the pid and a counter, so parallel cases never share one.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "yadgar-seeder-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One authority, `project`'s serving leaf, and named client leaves.
pub struct Pki {
    issuer: CertifiedIssuer<'static, KeyPair>,
    server_cert: String,
    server_key: String,
}

impl Pki {
    pub fn new() -> Self {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params
            .distinguished_name
            .push(DnType::CommonName, "seeder test authority");
        let issuer = CertifiedIssuer::self_signed(params, key).unwrap();

        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![SERVED_NAME.to_string()]).unwrap();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params
            .distinguished_name
            .push(DnType::CommonName, SERVED_NAME);
        let cert = params.signed_by(&key, &issuer).unwrap();
        Self {
            issuer,
            server_cert: cert.pem(),
            server_key: key.serialize_pem(),
        }
    }

    pub fn ca_pem(&self) -> String {
        self.issuer.pem()
    }

    /// A client leaf as `platform` issues one: `commonName: <module>-caller`,
    /// a dNSName SAN equal to it, `client auth`. Returns (cert PEM, key PEM).
    pub fn client(&self, common_name: &str) -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec![common_name.to_string()]).unwrap();
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params
            .distinguished_name
            .push(DnType::CommonName, common_name);
        let cert = params.signed_by(&key, &self.issuer).unwrap();
        (cert.pem(), key.serialize_pem())
    }
}

/// One call the fake received: who called, and what they sent.
#[derive(Clone, Debug)]
pub struct Received {
    pub caller: Option<String>,
    pub request: pb::ProjectServiceSyncProjectsRequest,
}

/// The fake's script and its record.
#[derive(Clone, Default)]
pub struct Fake {
    /// Answers handed out in order, one per ADMITTED call. When it runs out,
    /// every further call gets [`counts`]`(0, 0, 0, 0, 0)`.
    script: Arc<Mutex<Vec<Result<pb::ProjectServiceSyncProjectsResponse, Status>>>>,
    received: Arc<Mutex<Vec<Received>>>,
    /// Where this fake logs each call it gets, to be compared with the iam fake's.
    order: iam::Order,
}

impl Fake {
    pub fn answering(script: Vec<Result<pb::ProjectServiceSyncProjectsResponse, Status>>) -> Self {
        Self {
            script: Arc::new(Mutex::new(script)),
            received: Arc::default(),
            order: iam::Order::default(),
        }
    }

    /// The same fake, logging its calls into `order` beside the iam fake's.
    pub fn ordered(mut self, order: &iam::Order) -> Self {
        self.order = order.clone();
        self
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}

/// [`counts`] with the grant echo `project` returns once it vendors grants.
pub fn counts_with_grants(grants_declared: u32) -> pb::ProjectServiceSyncProjectsResponse {
    pb::ProjectServiceSyncProjectsResponse {
        grants_declared,
        ..counts(0, 0, 0, 0, 0)
    }
}

pub fn counts(
    inserted: u32,
    updated: u32,
    archived: u32,
    reactivated: u32,
    unchanged: u32,
) -> pb::ProjectServiceSyncProjectsResponse {
    pb::ProjectServiceSyncProjectsResponse {
        inserted,
        updated,
        archived,
        reactivated,
        unchanged,
        grants_declared: 0,
        grants_inserted: 0,
        grants_deleted: 0,
        grants_unchanged: 0,
    }
}

#[tonic::async_trait]
impl ProjectService for Fake {
    async fn list_projects(
        &self,
        _: Request<pb::ProjectServiceListProjectsRequest>,
    ) -> Result<Response<pb::ProjectServiceListProjectsResponse>, Status> {
        Err(Status::unimplemented("the seeder never lists"))
    }

    async fn resolve_project(
        &self,
        _: Request<pb::ProjectServiceResolveProjectRequest>,
    ) -> Result<Response<pb::ProjectServiceResolveProjectResponse>, Status> {
        Err(Status::unimplemented("the seeder never resolves"))
    }

    async fn sync_projects(
        &self,
        request: Request<pb::ProjectServiceSyncProjectsRequest>,
    ) -> Result<Response<pb::ProjectServiceSyncProjectsResponse>, Status> {
        self.order.push("SyncProjects");
        let caller = yadgar_lifecycle::serve_tls::caller_common_name(&request);
        self.received.lock().unwrap().push(Received {
            caller: caller.clone(),
            request: request.into_inner(),
        });
        // `project`'s admission, by literal (ADR-0936).
        if caller.as_deref() != Some("seeder-caller") {
            return Err(Status::permission_denied(format!(
                "fake project: caller {caller:?} is not the seeder"
            )));
        }
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            return Ok(Response::new(counts(0, 0, 0, 0, 0)));
        }
        script.remove(0).map(Response::new)
    }
}

/// Serve `fake` under REQUIRED client authentication, the mode every ADR-0852
/// hop runs. Returns the port.
pub async fn serve(pki: &Pki, fake: Fake) -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    serve_on(pki, fake, listener);
    port
}

/// A loopback port nothing listens on: bound, read, released. A later
/// [`serve_at`] binds it again, which is how a `project` that is "not serving
/// yet" is staged.
pub async fn free_port() -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    listener.local_addr().unwrap().port()
}

/// Serve `fake` on a port chosen earlier by [`free_port`].
pub async fn serve_at(pki: &Pki, fake: Fake, port: u16) {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
        .expect("the reserved port is free again");
    serve_on(pki, fake, listener);
}

fn serve_on(pki: &Pki, fake: Fake, listener: TcpListener) {
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(&pki.server_cert, &pki.server_key))
        .client_ca_root(Certificate::from_pem(pki.ca_pem()));
    let router = Server::builder()
        .tls_config(tls)
        .unwrap()
        .add_service(ProjectServiceServer::new(fake));
    tokio::spawn(async move {
        let _ = router
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await;
    });
}

/// The environment a deployment renders, pointed at the fake on `port`, with
/// the client leaf named `common_name` and the registry document `registry`.
pub fn env(
    scratch: &Scratch,
    pki: &Pki,
    port: u16,
    common_name: &str,
    registry: &str,
) -> Vec<(String, String)> {
    let ca = scratch.write("ca.pem", &pki.ca_pem());
    let (cert, key) = pki.client(common_name);
    let cert = scratch.write("client.crt", &cert);
    let key = scratch.write("client.key", &key);
    let registry = scratch.write("registry.json", registry);
    [
        ("SEEDER_REGISTRY_FILE", registry.display().to_string()),
        ("PROJECT_HOST", "127.0.0.1".to_string()),
        ("PROJECT_PORT", port.to_string()),
        ("PROJECT_REQUEST_TIMEOUT_SECONDS", "20".to_string()),
        ("PROJECT_TLS_CA_FILE", ca.display().to_string()),
        ("PROJECT_TLS_CLIENT_CERT_FILE", cert.display().to_string()),
        ("PROJECT_TLS_CLIENT_KEY_FILE", key.display().to_string()),
        ("PROJECT_TLS_DOMAIN", SERVED_NAME.to_string()),
        ("SEEDER_SYNC_ATTEMPTS", "3".to_string()),
        ("SEEDER_SYNC_BACKOFF_SECONDS", "5".to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// [`env`], plus the `IAM_*` block the chart renders when teams are declared,
/// pointed at the iam fake on `iam_port`. The same authority signs both fakes,
/// and the client leaf is the one `env` mints: the seeder presents one identity.
pub fn env_iam(
    scratch: &Scratch,
    pki: &Pki,
    port: u16,
    iam_port: u16,
    registry: &str,
) -> Vec<(String, String)> {
    let mut vars = env(scratch, pki, port, "seeder-caller", registry);
    let ca = scratch.path().join("ca.pem").display().to_string();
    vars.extend(
        [
            ("IAM_HOST", "127.0.0.1".to_string()),
            ("IAM_PORT", iam_port.to_string()),
            ("IAM_REQUEST_TIMEOUT_SECONDS", "20".to_string()),
            ("IAM_TLS_CA_FILE", ca),
            ("IAM_TLS_DOMAIN", SERVED_NAME.to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v)),
    );
    vars
}

/// Run the real binary with exactly `vars` (the environment is cleared first)
/// and `args`, off the async runtime so the fake keeps serving.
pub async fn run(vars: Vec<(String, String)>, args: &[&str]) -> Output {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    tokio::task::spawn_blocking(move || {
        Command::new(BIN)
            .args(&args)
            .env_clear()
            .envs(vars)
            .output()
            .expect("the binary must start")
    })
    .await
    .unwrap()
}

/// Everything the binary wrote, both streams.
pub fn text(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
