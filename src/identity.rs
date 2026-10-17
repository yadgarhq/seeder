//! Who the seeder is, on both of the checks `project` makes (ADR-0936,
//! ADR-0942), and the boot check that the mounted client leaf is the seeder's.
//!
//! **THE TWO LITERALS ARE SHARED WITH `project` BY VALUE, NOT BY CRATE.** The
//! repositories share no crate, so `project`'s `src/service/sync.rs` declares
//! `SYNC_CALLER` and `SEEDER_PRINCIPAL` and this file declares the same two
//! strings. Each suite pins its copy by literal (`tests/sync.rs` here), so a
//! change on one side alone is red on that side.

use std::path::Path;

use rustls_pki_types::pem::PemObject as _;
use rustls_pki_types::CertificateDer;

/// The common name on the seeder's client leaf, as `platform` issues it
/// (`seeder-client-tls`). `project` admits `SyncProjects` from this name only.
pub const CALLER: &str = "seeder-caller";

/// The seeder's fixed system principal (ADR-0942): the `scope.user_id` every
/// sync carries, stamped by the store as `created_by` and `updated_by`.
pub const PRINCIPAL: &str = "yadgar:system:seeder";

/// The mounted client leaf is not the seeder's.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("the client certificate {path} could not be read as PEM: {why}")]
    Unreadable { path: String, why: String },

    #[error(
        "the client certificate {path} names {found}, and `project` admits SyncProjects only \
         from a leaf naming {CALLER:?} (ADR-0936). The chart mounts the Secret named by \
         project.tls.clientCertSecret; it must be `seeder-client-tls`, the leaf `platform` \
         issues for the seeder. Every module's leaf verifies against the same authority, so \
         another module's leaf would handshake and then be refused PERMISSION_DENIED"
    )]
    WrongLeaf { path: String, found: String },
}

/// Refuse a client leaf whose single common name is not [`CALLER`].
///
/// **BEFORE ANY DIAL.** A wrong Secret (say `project-client-tls`) handshakes
/// fine, because every leaf verifies against the same authority, and is then
/// refused by `project` on every call. Checking here turns that into one boot
/// refusal naming the Secret, instead of a retry loop ending in a refusal. The
/// common name is read with `yadgar_lifecycle::serve_tls::leaf_common_name`,
/// the parser `project`'s own caller check uses.
pub fn check_client_leaf(path: &Path) -> Result<(), IdentityError> {
    // ADR-0523-UNWATCHED: a Job process lives for one sync; a rotated leaf is read by the next run.
    let leaf = CertificateDer::from_pem_file(path).map_err(|e| IdentityError::Unreadable {
        path: path.display().to_string(),
        why: e.to_string(),
    })?;
    match yadgar_lifecycle::serve_tls::leaf_common_name(&leaf) {
        Some(cn) if cn == CALLER => Ok(()),
        found => Err(IdentityError::WrongLeaf {
            path: path.display().to_string(),
            found: found.map_or_else(
                || "no single common name".to_string(),
                |cn| format!("{cn:?}"),
            ),
        }),
    }
}
