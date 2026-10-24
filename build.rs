// Types are GENERATED from the vendored contract, never hand-written (D16). The
// protos come from proto/, vendored at the tag in PROTO_VERSION (D70), so this
// build reaches no network and needs no credentials.
use std::path::Path;

/// Where `google/protobuf/*.proto` lives: `PROTOC_INCLUDE` when set, otherwise
/// `/usr/include` if it actually holds them, otherwise nothing and protoc uses
/// its own. Same rule as `project`'s build script.
fn well_known_include() -> Option<String> {
    if let Ok(dir) = std::env::var("PROTOC_INCLUDE") {
        return Some(dir);
    }
    Path::new("/usr/include/google/protobuf/timestamp.proto")
        .exists()
        .then(|| "/usr/include".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-env-changed=PROTOC_INCLUDE");

    let mut includes = vec!["proto".to_string()];
    includes.extend(well_known_include());
    let includes: Vec<&str> = includes.iter().map(String::as_str).collect();

    tonic_prost_build::configure()
        // THE CLIENT HALF for the binary. The server half is generated too,
        // and only the test suite uses it: `tests/` serves a fake
        // ProjectService so the seeder's real client is exercised over a real
        // handshake. Generated code costs nothing it does not link.
        .build_client(true)
        .build_server(true)
        .compile_protos(
            &[
                "proto/yadgar/common/v1/common.proto",
                "proto/yadgar/project/v1/project.proto",
                "proto/yadgar/iam/v1/iam.proto",
            ],
            &includes[..],
        )?;
    Ok(())
}
