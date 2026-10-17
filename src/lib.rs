//! `seeder` — the D35 seeder: the one writer of the organisation's declared
//! project registry (ADR-0705 as amended by ADR-0713 and ADR-0936, ADR-0934).
//!
//! **A JOB, NOT A SERVICE (ADR-0940).** It serves no RPC. One run is one sync:
//! read the declaration the chart rendered, check it, send it to `project` in
//! ONE `ProjectService.SyncProjects` call, log what the store did, exit. A
//! commit to the organisation's GitOps repository re-renders the Job, and that
//! is the only trigger. There is no CronJob.
//!
//! **IT DIALS `project` AND NOTHING ELSE.** No module talks to a `-db` or its
//! engine directly (ADR-0936, ADR-0938): `project` checks the caller and relays
//! to `project-db`. This crate has no configuration key that could name
//! `project-db`, and `tests/config.rs` pins that.

#![forbid(unsafe_code)]

pub mod config;
pub mod identity;
pub mod registry;
pub mod sync;
pub mod upstream;

/// Generated from the vendored contract (D16, D70), at the SAME tag `project`
/// vendors (`PROTO_VERSION`). The module tree mirrors the protobuf package path,
/// because generated cross-package references are emitted as
/// `super::super::common::v1::Scope`.
pub mod pb {
    pub mod yadgar {
        pub mod common {
            pub mod v1 {
                tonic::include_proto!("yadgar.common.v1");
            }
        }
        pub mod project {
            pub mod v1 {
                // OVER GENERATED CODE THIS REPOSITORY CANNOT EDIT, scoped to the
                // generated module only. `project.proto`'s comment above
                // `service ProjectService` is a bullet list whose continuation
                // lines are flush with the marker; prost copies it verbatim as a
                // `///` doc comment and `clippy::doc_lazy_continuation` (in
                // `clippy::all`, which this crate denies) reads it as a lost
                // indent. `project` carries the same suppression for the same
                // file (D16: generated output is never hand-edited).
                #![allow(clippy::doc_lazy_continuation)]

                tonic::include_proto!("yadgar.project.v1");
            }
        }
    }
}
