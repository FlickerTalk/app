# ft-support image: the FlickerTalk support contact (crates/ft-support), the Rust binary on a
# minimal base with no shell, as a non-root user. Built by CI (.github/workflows/support.yml) and
# published to GHCR; the cluster pulls it from there. Build from the repository's root:
#
#   docker build -f crates/ft-support/Dockerfile -t ft-support .
FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY . .
# The toolchain file pins the same Rust as this image and adds the Android targets the app needs;
# this image only builds for Linux.
RUN rm -f rust-toolchain.toml
# Thin LTO and parallel code generation: the workspace's release profile (fat LTO, one codegen
# unit) is tuned for the size of the phones' library, not for a server binary, and builds several
# times slower.
ENV CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
RUN cargo build --release --locked -p ft-support \
    && mkdir -p /out/data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/ft-support /usr/local/bin/ft-support
# Everything it keeps lives here: key, database, identity bundle, card link, files. Owned by the
# image's user, so a new named volume mounted on it is too.
COPY --from=build --chown=65532:65532 /out/data /data
VOLUME /data
ENTRYPOINT ["/usr/local/bin/ft-support"]
