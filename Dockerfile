FROM rust:1-bookworm

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/cargo-target \
    HOME=/tmp

RUN rustup component add clippy rustfmt

# Mode 1777 so an arbitrary non-root uid:gid can write the named cache volumes.
RUN mkdir -p /cargo /cargo-target /app && chmod 1777 /cargo /cargo-target /app

WORKDIR /app
