//! Every knob this binary reads, from the environment alone, with NO
//! compiled-in default (ADR-0569 as amended by ADR-0705). The chart supplies
//! each one; an absent or empty value refuses the run, naming the variable and
//! the chart key that renders it.
//!
//! **THE ONLY UPSTREAMS ARE `project` AND `iam`.** There is no `PROJECT_DB_*`
//! or `IAM_DB_*` key and no other host key of any kind (ADR-0936, ADR-0938: no
//! module dials a `-db`). `iam` is dialled only when the declaration names
//! teams (`SyncTeams`, plans/team-access.md), so the `IAM_*` block is optional AS
//! A BLOCK: absent `IAM_HOST` means no iam, and once it is set every other
//! `IAM_*` key but the TLS domain is required. The set of keys read is
//! [`KEYS`], and `tests/config.rs` asserts it against the keys a real read
//! actually looks up, so a key added here and not there is red.
//!
//! **NO TLS SWITCH, deliberately.** `project` admits `SyncProjects` only from a
//! verified client leaf named `seeder-caller` (ADR-0936, ADR-0942), so a
//! cleartext seeder, or one presenting no client certificate, is refused on
//! every call. A `PROJECT_TLS_ENABLED=0` lever would be a configuration that can
//! only fail, so the CA bundle and the client identity are simply required.

use std::path::PathBuf;
use std::time::Duration;

/// The variables, in the order [`Config::from_lookup`] reads them.
pub const KEYS: &[&str] = &[
    REGISTRY_FILE,
    PROJECT_HOST,
    PROJECT_PORT,
    REQUEST_TIMEOUT,
    TLS_CA_FILE,
    TLS_CLIENT_CERT_FILE,
    TLS_CLIENT_KEY_FILE,
    TLS_DOMAIN,
    SYNC_ATTEMPTS,
    SYNC_BACKOFF,
    IAM_HOST,
    IAM_PORT,
    IAM_REQUEST_TIMEOUT,
    IAM_TLS_CA_FILE,
    IAM_TLS_DOMAIN,
];

pub const REGISTRY_FILE: &str = "SEEDER_REGISTRY_FILE";
pub const PROJECT_HOST: &str = "PROJECT_HOST";
pub const PROJECT_PORT: &str = "PROJECT_PORT";
pub const REQUEST_TIMEOUT: &str = "PROJECT_REQUEST_TIMEOUT_SECONDS";
pub const TLS_CA_FILE: &str = "PROJECT_TLS_CA_FILE";
pub const TLS_CLIENT_CERT_FILE: &str = "PROJECT_TLS_CLIENT_CERT_FILE";
pub const TLS_CLIENT_KEY_FILE: &str = "PROJECT_TLS_CLIENT_KEY_FILE";
/// OPTIONAL, and absence is not a default: it means "verify `project`'s
/// certificate against the host dialled", which is what `yadgar_dial` does with
/// no override. The same optional override `project` and `gateway` carry.
pub const TLS_DOMAIN: &str = "PROJECT_TLS_DOMAIN";
pub const SYNC_ATTEMPTS: &str = "SEEDER_SYNC_ATTEMPTS";
pub const SYNC_BACKOFF: &str = "SEEDER_SYNC_BACKOFF_SECONDS";
/// The `IAM_*` block, rendered only when `registry.teams` is declared. The
/// client leaf is the seeder's one (`PROJECT_TLS_CLIENT_*`): `seeder-client-tls`
/// is reused for iam, no second leaf.
pub const IAM_HOST: &str = "IAM_HOST";
pub const IAM_PORT: &str = "IAM_PORT";
pub const IAM_REQUEST_TIMEOUT: &str = "IAM_REQUEST_TIMEOUT_SECONDS";
pub const IAM_TLS_CA_FILE: &str = "IAM_TLS_CA_FILE";
/// OPTIONAL, like [`TLS_DOMAIN`].
pub const IAM_TLS_DOMAIN: &str = "IAM_TLS_DOMAIN";

/// The chart key each variable is rendered from, for the refusal.
fn chart_key(var: &str) -> &'static str {
    match var {
        REGISTRY_FILE => "the `registry` block (the chart mounts it)",
        PROJECT_HOST => "project.host",
        PROJECT_PORT => "project.port",
        REQUEST_TIMEOUT => REQUEST_TIMEOUT_CHART_KEY,
        TLS_CA_FILE => "project.tls.caSecret",
        TLS_CLIENT_CERT_FILE | TLS_CLIENT_KEY_FILE => "project.tls.clientCertSecret",
        IAM_HOST => "iam.host",
        IAM_PORT => "iam.port",
        IAM_REQUEST_TIMEOUT => IAM_REQUEST_TIMEOUT_CHART_KEY,
        IAM_TLS_CA_FILE => "iam.tls.caSecret",
        SYNC_ATTEMPTS => "sync.attempts",
        SYNC_BACKOFF => "sync.backoffSeconds",
        _ => "this chart's values.yaml",
    }
}

/// The chart key that renders [`REQUEST_TIMEOUT`].
pub const REQUEST_TIMEOUT_CHART_KEY: &str = "project.requestTimeoutSeconds";

/// The chart key that renders [`IAM_REQUEST_TIMEOUT`].
pub const IAM_REQUEST_TIMEOUT_CHART_KEY: &str = "iam.requestTimeoutSeconds";

/// The most attempts a run may make. A ceiling, so a typo cannot hold a Job
/// for hours; the Job's `activeDeadlineSeconds` is the outer bound.
pub const MAX_ATTEMPTS: u32 = 60; // ADR-0569-EXCEPTION(CC): a ceiling on a knob, not a knob.

/// The longest pause between attempts.
pub const MAX_BACKOFF: Duration = Duration::from_secs(300); // ADR-0569-EXCEPTION(CC): a ceiling on a knob, not a knob.

/// Everything one run needs, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub registry_file: PathBuf,
    pub project_host: String,
    pub project_port: u16,
    pub request_timeout: Duration,
    pub tls: Tls,
    pub retry: Retry,
    /// `Some` only when the chart rendered the `IAM_*` block, which it does
    /// only when `registry.teams` is declared.
    pub iam: Option<Iam>,
}

/// The transport to `iam`. The client leaf is [`Tls::client_cert_file`] and
/// [`Tls::client_key_file`]: the seeder presents one identity to both hops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iam {
    pub host: String,
    pub port: u16,
    pub request_timeout: Duration,
    /// The authority that signed iam's serving certificate (`iam-tls`).
    pub ca_file: PathBuf,
    pub domain: Option<String>,
}

/// The transport to `project`: the authority it is verified against, and the
/// seeder's own client leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls {
    pub ca_file: PathBuf,
    pub client_cert_file: PathBuf,
    pub client_key_file: PathBuf,
    pub domain: Option<String>,
}

/// How a `project` that answers `UNAVAILABLE` is retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Total attempts, the first included. At least 1.
    pub attempts: u32,
    /// The pause between two attempts. At least `yadgar_dial`'s re-resolve
    /// interval, so a retry can see an endpoint set the last one could not.
    pub backoff: Duration,
}

/// A knob that is missing or unusable. Every message names the variable and
/// the chart key.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "{var} is NOT SET. It has no compiled-in default (ADR-0569): the chart renders it from \
         {key}, and this run refuses rather than invent a value"
    )]
    Absent {
        var: &'static str,
        key: &'static str,
    },

    #[error(
        "{var} is set but EMPTY. It has no compiled-in default (ADR-0569); a values override \
         that nulls {key} produces exactly this"
    )]
    Empty {
        var: &'static str,
        key: &'static str,
    },

    #[error("{var} is {got:?}, which is not usable: {why}. It is rendered from {key}")]
    Unusable {
        var: &'static str,
        key: &'static str,
        got: String,
        why: String,
    },

    #[error("{0}")]
    Timeout(#[from] yadgar_dial::RequestTimeoutError),
}

impl Config {
    /// Read every knob from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// The same read over an injected lookup: environment variables are
    /// process-global, so a test that set them would steer every other test.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |var: &'static str| -> Result<String, ConfigError> {
            match lookup(var) {
                None => Err(ConfigError::Absent {
                    var,
                    key: chart_key(var),
                }),
                Some(v) if v.trim().is_empty() => Err(ConfigError::Empty {
                    var,
                    key: chart_key(var),
                }),
                Some(v) => Ok(v.trim().to_string()),
            }
        };
        let unusable = |var: &'static str, got: &str, why: String| ConfigError::Unusable {
            var,
            key: chart_key(var),
            got: got.to_string(),
            why,
        };

        let registry_file = PathBuf::from(required(REGISTRY_FILE)?);
        let project_host = required(PROJECT_HOST)?;
        let port = required(PROJECT_PORT)?;
        let project_port: u16 = port
            .parse()
            .map_err(|e| unusable(PROJECT_PORT, &port, format!("not a port number: {e}")))?;
        let request_timeout = yadgar_dial::request_timeout_from_lookup(
            &lookup,
            REQUEST_TIMEOUT,
            REQUEST_TIMEOUT_CHART_KEY,
        )?;
        let tls = Tls {
            ca_file: PathBuf::from(required(TLS_CA_FILE)?),
            client_cert_file: PathBuf::from(required(TLS_CLIENT_CERT_FILE)?),
            client_key_file: PathBuf::from(required(TLS_CLIENT_KEY_FILE)?),
            domain: lookup(TLS_DOMAIN)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
        };

        let raw = required(SYNC_ATTEMPTS)?;
        let attempts: u32 = raw
            .parse()
            .map_err(|e| unusable(SYNC_ATTEMPTS, &raw, format!("not a whole number: {e}")))?;
        if attempts == 0 || attempts > MAX_ATTEMPTS {
            return Err(unusable(
                SYNC_ATTEMPTS,
                &raw,
                format!("it must be 1 to {MAX_ATTEMPTS}; 1 means no retry"),
            ));
        }
        let raw = required(SYNC_BACKOFF)?;
        let secs: u64 = raw
            .parse()
            .map_err(|e| unusable(SYNC_BACKOFF, &raw, format!("not a whole number: {e}")))?;
        let floor = yadgar_dial::reresolve_interval();
        let backoff = Duration::from_secs(secs);
        if backoff < floor || backoff > MAX_BACKOFF {
            return Err(unusable(
                SYNC_BACKOFF,
                &raw,
                format!(
                    "it must be {} to {} seconds. The floor is yadgar-dial's re-resolve \
                     interval: a shorter pause retries against the same endpoint set that \
                     just failed",
                    floor.as_secs(),
                    MAX_BACKOFF.as_secs()
                ),
            ));
        }

        let iam = if lookup(IAM_HOST).is_some() {
            let host = required(IAM_HOST)?;
            let raw = required(IAM_PORT)?;
            let port: u16 = raw
                .parse()
                .map_err(|e| unusable(IAM_PORT, &raw, format!("not a port number: {e}")))?;
            Some(Iam {
                host,
                port,
                request_timeout: yadgar_dial::request_timeout_from_lookup(
                    &lookup,
                    IAM_REQUEST_TIMEOUT,
                    IAM_REQUEST_TIMEOUT_CHART_KEY,
                )?,
                ca_file: PathBuf::from(required(IAM_TLS_CA_FILE)?),
                domain: lookup(IAM_TLS_DOMAIN)
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty()),
            })
        } else {
            None
        };

        Ok(Self {
            registry_file,
            project_host,
            project_port,
            request_timeout,
            tls,
            retry: Retry { attempts, backoff },
            iam,
        })
    }
}
