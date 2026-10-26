//! The organisation's declared registry: the document the chart renders into
//! `ConfigMap/seeder-registry` from the adopter's `registry` values block.
//!
//! **CHECKED HERE AS WELL AS IN THE CHART**, so a hand-edited ConfigMap is
//! refused too (plans/the-d35-seeder.md, "The module"). The rules mirror
//! `project-db`'s own `SyncProjects` validation (`src/write/sync.rs`,
//! `src/path.rs` at v0.7.0), which stays the authority: a declaration the store
//! would refuse is refused here before anything is dialled, with the path named.
//!
//! **THE SHAPE IS CLOSED.** Unknown keys are refused, so a typo in a key name is
//! a refusal naming the key rather than a field silently read as absent. Every
//! key is required: `displayName` may be `""` (a project whose name is its path
//! is an ordinary declaration), but the chart always renders it, so absence
//! means the document was not written by the chart.

use std::collections::HashSet;

use serde::Deserialize;

use crate::pb::yadgar::iam::v1::DeclaredTeam;
use crate::pb::yadgar::project::v1::DeclaredProject;

mod teams;

/// The size guard on the rendered document, below the 1 MiB a ConfigMap can
/// hold (ADR-0705's guard). The chart's `templates/configmap.yaml` refuses at
/// the same number; `scripts/tests/` pins the two equal.
pub const MAX_BYTES: usize = 786_432; // ADR-0569-EXCEPTION(CC): a ceiling derived from the ConfigMap limit, not a knob.

/// The widest path the store accepts (`project-db/src/path.rs`, `MAX_LEN`).
const MAX_PATH_LEN: usize = 255; // ADR-0569-EXCEPTION(CC): the store's column width, not a knob.

/// The widest `display_name` and `source_repo` the store accepts, in characters
/// (`project-db/src/write/sync.rs`, `MAX_CHARS`).
const MAX_CHARS: usize = 255; // ADR-0569-EXCEPTION(CC): the store's column width, not a knob.

/// The reserved first segment of the private class (`project-db/src/path.rs`).
const RESERVED_ROOT: &str = "local";

/// One row as the document states it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Entry {
    path: String,
    display_name: String,
    source_repo: String,
    /// Absent is empty: a row that grants nothing may leave the key out.
    #[serde(default)]
    grants: Vec<teams::Grant>,
}

/// The document as the chart renders it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Document {
    roots: Vec<Entry>,
    projects: Vec<Entry>,
    allow_mass_archive: bool,
    /// REQUIRED and non-empty (team access stage 12). `Option` only so that an
    /// absent key is refused by `validate` with a sentence naming it, not by
    /// serde's "missing field".
    teams: Option<Vec<teams::Team>>,
    /// REQUIRED, with no default (ADR-0963 (6)): checked in `validate`.
    allow_mass_revoke: Option<bool>,
}

/// A checked declaration, already in the contract's element type.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub roots: Vec<DeclaredProject>,
    pub projects: Vec<DeclaredProject>,
    /// ADR-0939's flag. `false` is the guard ON. Read only from the explicit
    /// `registry.allowMassArchive` value; there is no other source.
    pub allow_mass_archive: bool,
    /// The declared teams, in the iam contract's element type. Never empty:
    /// a declaration without teams is refused (plans/team-access.md stage 12).
    pub teams: Vec<DeclaredTeam>,
    /// ADR-0963 (6)'s flag. `false` is the guard ON.
    pub allow_mass_revoke: bool,
}

/// Why the declaration was refused. Every message names the path or the key.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(
        "the registry file {path} could not be read: {source}. The chart mounts it from \
         ConfigMap/seeder-registry; SEEDER_REGISTRY_FILE names where"
    )]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "the registry document is {bytes} bytes and the guard is {limit} bytes, below the 1 MiB \
         a ConfigMap holds (ADR-0705). Split nothing: a declaration this large is a mistake to \
         look at before it archives anything"
    )]
    TooLarge { bytes: usize, limit: usize },

    #[error(
        "the registry file is not a valid registry document: {0}. It is rendered by the chart \
         from the `registry` values block; every key is required and unknown keys are refused"
    )]
    Malformed(#[source] serde_json::Error),

    #[error("{0}")]
    Invalid(String),
}

/// Read and check the document at `path`.
pub fn read(path: &std::path::Path) -> Result<Declaration, RegistryError> {
    let bytes = std::fs::read(path).map_err(|source| RegistryError::Read {
        path: path.display().to_string(),
        source,
    })?;
    parse(&bytes)
}

/// Check a document's bytes. The size guard runs FIRST, before the parser.
pub fn parse(bytes: &[u8]) -> Result<Declaration, RegistryError> {
    if bytes.len() > MAX_BYTES {
        return Err(RegistryError::TooLarge {
            bytes: bytes.len(),
            limit: MAX_BYTES,
        });
    }
    let doc: Document = serde_json::from_slice(bytes).map_err(RegistryError::Malformed)?;
    validate(doc)
}

/// The whole declaration, or nothing — the order the store checks in.
fn validate(doc: Document) -> Result<Declaration, RegistryError> {
    let declared_teams = match doc.teams.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err(invalid(
                "registry.teams is absent or empty. A declaration names its teams (ADR-0962 \
                 ruling 5): declare registry.teams with the team \"administrators\", every \
                 root granting it readWrite, and registry.allowMassRevoke. The transition rule \
                 that let a declaration omit `teams` is removed (plans/team-access.md stage 12)"
                    .to_string(),
            ))
        }
    };
    if doc.roots.is_empty() {
        return Err(invalid(
            "a declaration names at least one root: the namespace anchors every project sits \
             under (ADR-0673). `registry.roots` is empty"
                .to_string(),
        ));
    }
    let mut keys = Vec::with_capacity(doc.roots.len() + doc.projects.len());
    let mut roots = HashSet::new();
    for root in &doc.roots {
        row("registry.roots[].path", root)?;
        if root.path.contains('/') {
            return Err(invalid(format!(
                "the root {:?} has more than one segment. A root is a namespace anchor, a single \
                 segment such as \"acme\"; declare a deeper path as a project under one",
                root.path
            )));
        }
        let key = root.path.to_ascii_lowercase();
        roots.insert(key.clone());
        keys.push((key, root.path.as_str()));
    }
    for project in &doc.projects {
        row("registry.projects[].path", project)?;
        let key = project.path.to_ascii_lowercase();
        let first = key.split('/').next().unwrap_or_default();
        if !key.contains('/') || !roots.contains(first) {
            return Err(invalid(format!(
                "the project {:?} is not under a declared root. Every project sits strictly \
                 beneath one of `registry.roots`",
                project.path
            )));
        }
        keys.push((key, project.path.as_str()));
    }
    let mut seen = HashSet::new();
    for (key, path) in &keys {
        if !seen.insert(key.as_str()) {
            return Err(invalid(format!(
                "{path:?} is declared twice (paths are compared with ASCII case folded, as the \
                 store compares them)"
            )));
        }
    }
    let wire_teams = teams::check(declared_teams, &doc.roots, &doc.projects)?;
    let Some(allow_mass_revoke) = doc.allow_mass_revoke else {
        return Err(invalid(
            "registry.allowMassRevoke must be set to true or false. It has no default (ADR-0963 \
             (6)): false keeps the store's mass-revoke guard on, and true lets ONE sync delete \
             more than half of the stored grants"
                .to_string(),
        ));
    };
    Ok(Declaration {
        roots: doc.roots.into_iter().map(declared).collect(),
        projects: doc.projects.into_iter().map(declared).collect(),
        allow_mass_archive: doc.allow_mass_archive,
        teams: wire_teams,
        allow_mass_revoke,
    })
}

/// One row's own checks: grammar, class, and the two column widths.
fn row(what: &str, e: &Entry) -> Result<(), RegistryError> {
    grammar(what, &e.path)?;
    if e.path.eq_ignore_ascii_case(RESERVED_ROOT) {
        return Err(invalid(format!(
            "{what} {:?} is the reserved segment {RESERVED_ROOT:?}, the first segment of every \
             private project path; it is not an organisation",
            e.path
        )));
    }
    let private = e
        .path
        .split_once('/')
        .is_some_and(|(first, _)| first.eq_ignore_ascii_case(RESERVED_ROOT));
    if private {
        return Err(invalid(format!(
            "{what} {:?} is a private `local/...` path. A sync declares organisation projects \
             only; personal projects are registered by their own account (ledger 641)",
            e.path
        )));
    }
    if e.source_repo.is_empty() {
        return Err(invalid(format!(
            "{:?} has no sourceRepo. Every declared row is org class, and an org row names the \
             repository whose pull request flow governs it",
            e.path
        )));
    }
    for (field, value) in [
        ("sourceRepo", &e.source_repo),
        ("displayName", &e.display_name),
    ] {
        let chars = value.chars().count();
        if chars > MAX_CHARS {
            return Err(invalid(format!(
                "the {field} of {:?} is {chars} characters and the store's limit is {MAX_CHARS}",
                e.path
            )));
        }
    }
    Ok(())
}

/// The store's path grammar (`project-db/src/path.rs::validate`): non-empty, at
/// most 255 bytes, no leading or trailing slash, and every segment one or more
/// of `A-Z a-z 0-9 . _ -`, never `.` or `..`.
fn grammar(what: &str, path: &str) -> Result<(), RegistryError> {
    if path.is_empty() {
        return Err(invalid(format!("{what}: a path is empty")));
    }
    if path.len() > MAX_PATH_LEN {
        return Err(invalid(format!(
            "{what} is {} characters and the store's limit is {MAX_PATH_LEN}",
            path.len()
        )));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(invalid(format!(
            "{what} {path:?} begins or ends with a slash, which is an empty segment"
        )));
    }
    for segment in path.split('/') {
        let legal = !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !legal {
            return Err(invalid(format!(
                "{what} {path:?} contains the segment {segment:?}. A segment is one or more of \
                 A-Z a-z 0-9 . _ - and is neither \".\" nor \"..\""
            )));
        }
    }
    Ok(())
}

fn invalid(message: String) -> RegistryError {
    RegistryError::Invalid(message)
}

/// The contract's element, built field for field. EXHAUSTIVE on purpose: a
/// field added to `DeclaredProject` fails to compile here rather than being
/// sent at its zero value.
fn declared(e: Entry) -> DeclaredProject {
    let Entry {
        path,
        display_name,
        source_repo,
        grants,
    } = e;
    DeclaredProject {
        path,
        display_name,
        source_repo,
        grants: grants.iter().map(teams::wire_grant).collect(),
    }
}

#[cfg(test)]
mod tests;
