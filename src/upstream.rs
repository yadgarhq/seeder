//! Connecting to `project`, the seeder's main upstream (ADR-0936), and to `iam`
//! when the declaration names teams (`SyncTeams`, plans/team-access.md).
//!
//! Through `yadgar_dial`, because `project`'s Service is headless (D23) and the
//! balancer re-resolves it. **THE DIAL IS LAZY**: `connect` returns a channel
//! for a name that does not resolve yet, and the failure moves to the call,
//! where it is `UNAVAILABLE` and [`crate::sync`] retries it. So a `project` that
//! is not serving yet does not fail the Job at once.
//!
//! **ALWAYS MUTUAL TLS.** The CA bundle authenticates `project` to the seeder;
//! the client leaf authenticates the seeder to `project`, whose caller check
//! reads its common name. There is no cleartext path (see `crate::config`).

use tonic::transport::Channel;
use yadgar_dial::{BalanceError, TlsOptions};

use crate::config::{Config, Iam};

/// The transport, as `yadgar_dial` takes it, identity included.
pub fn options(config: &Config) -> TlsOptions {
    let tls = &config.tls;
    let options =
        TlsOptions::new(&tls.ca_file).identity(&tls.client_cert_file, &tls.client_key_file);
    match &tls.domain {
        None => options,
        Some(domain) => options.domain_name(domain),
    }
}

/// A balanced, mutually authenticated channel to `project`.
pub async fn connect(config: &Config) -> Result<Channel, BalanceError> {
    // ADR-0523-UNWATCHED: yadgar_dial reads the CA bundle and client leaf once; a Job process lives for one sync.
    yadgar_dial::connect_tls_with_request_timeout(
        &config.project_host,
        config.project_port,
        &options(config),
        config.request_timeout,
    )
    .await
}

/// The transport to `iam`: its own authority, the seeder's own client leaf
/// (`seeder-client-tls` is reused, no second leaf).
pub fn iam_options(config: &Config, iam: &Iam) -> TlsOptions {
    let tls = &config.tls;
    let options =
        TlsOptions::new(&iam.ca_file).identity(&tls.client_cert_file, &tls.client_key_file);
    match &iam.domain {
        None => options,
        Some(domain) => options.domain_name(domain),
    }
}

/// A balanced, mutually authenticated channel to `iam`. Lazy, like [`connect`].
pub async fn connect_iam(config: &Config, iam: &Iam) -> Result<Channel, BalanceError> {
    // ADR-0523-UNWATCHED: yadgar_dial reads the CA bundle and client leaf once; a Job process lives for one sync.
    yadgar_dial::connect_tls_with_request_timeout(
        &iam.host,
        iam.port,
        &iam_options(config, iam),
        iam.request_timeout,
    )
    .await
}
