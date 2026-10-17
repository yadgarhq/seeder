# Two shared bases (D63): build fat, ship distroless. `:latest` is deliberate:
# the bases are themselves built, scanned, signed and digest-pinned by the
# base-images workflow (D61, D63), and the published chart pins THIS image by
# digest (D65).
#
# The ignore must sit IMMEDIATELY before the instruction.
# hadolint ignore=DL3007
FROM ghcr.io/yadgarhq/rust-build:latest AS chef
WORKDIR /app

# cargo-chef splits dependency compilation from source compilation, so a
# source-only change does not rebuild the dependency graph.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json
COPY . .
# musl, so the runtime can be distroless/static (D63).
RUN cargo build --release --target x86_64-unknown-linux-musl

# hadolint ignore=DL3007
FROM ghcr.io/yadgarhq/runtime:latest
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/yadgar-seeder /yadgar-seeder

# The runtime base already declares this (D63); repeated so a static scanner
# reading THIS file sees the image does not run as root.
USER 65532:65532
# NO EXPOSE: the seeder serves nothing. It is a Job that dials `project` once.
ENTRYPOINT ["/yadgar-seeder"]
