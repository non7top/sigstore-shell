FROM rust:1-bookworm AS dev

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/cargo-target \
    HOME=/tmp

# The posix thread model is what Rust's windows-gnu std expects.
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64-posix binutils-mingw-w64-x86-64 cmake nasm zip \
    && rm -rf /var/lib/apt/lists/* \
    && update-alternatives --set x86_64-w64-mingw32-gcc /usr/bin/x86_64-w64-mingw32-gcc-posix \
    && rustup target add x86_64-pc-windows-gnu \
    && rustup component add clippy rustfmt

# Mode 1777 so an arbitrary non-root uid:gid can write the named cache volumes.
RUN mkdir -p /cargo /cargo-target /app && chmod 1777 /cargo /cargo-target /app

WORKDIR /app

# Debian 13 (trixie) for Wine 10: Wine 8 in bookworm lacks bcryptprimitives.dll, which Rust's std imports.
FROM debian:trixie-slim AS wine

RUN apt-get update \
    && apt-get install -y --no-install-recommends wine wine64 xvfb xauth ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ENV WINEPREFIX=/wine/prefix \
    WINEDEBUG=-all \
    WINEARCH=win64 \
    HOME=/tmp \
    PATH=/usr/lib/wine:$PATH

RUN mkdir -p /wine /app && chmod 1777 /wine /app

WORKDIR /app
