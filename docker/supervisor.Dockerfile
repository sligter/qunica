# syntax=docker/dockerfile:1.7
ARG RUST_VERSION=1.97.1
FROM rust:${RUST_VERSION}-bookworm AS build
WORKDIR /src
COPY rust-toolchain.toml ./
COPY backend-rs/ backend-rs/
RUN --mount=type=cache,id=qunica-supervisor-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=qunica-supervisor-target,target=/build-target \
    CARGO_TARGET_DIR=/build-target cargo build --locked --release --manifest-path backend-rs/Cargo.toml --package qunica-computer-supervisor \
    && cp /build-target/release/qunica-computer-supervisor /usr/local/bin/

FROM debian:bookworm-slim
COPY --from=build /usr/local/bin/qunica-computer-supervisor /usr/local/bin/
# Only this control service receives the Docker socket; it runs no agents and
# has no application database, signing key, workspace or provider credentials.
ENV QUNICA_SUPERVISOR_LISTEN=0.0.0.0:9000 QUNICA_SUPERVISOR_STATE=/state
EXPOSE 9000
ENTRYPOINT ["/usr/local/bin/qunica-computer-supervisor"]
