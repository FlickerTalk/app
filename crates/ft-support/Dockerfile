# Build from the repository root: docker build -f crates/ft-support/Dockerfile -t ft-support .
FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY . .
# rust-toolchain.toml adds the Android targets the app needs, this image only builds for Linux
RUN rm -f rust-toolchain.toml
# the workspace release profile (fat LTO, one codegen unit) is tuned for the phones' library size
# and builds several times slower
ENV CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
RUN cargo build --release --locked -p ft-support \
    && mkdir -p /out/data

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /src/target/release/ft-support /usr/local/bin/ft-support
# owned by the image's user, so a new named volume mounted here is too
COPY --from=build --chown=65532:65532 /out/data /data
VOLUME /data
ENTRYPOINT ["/usr/local/bin/ft-support"]
