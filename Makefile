export DOCKER_UID := $(shell id -u)
export DOCKER_GID := $(shell id -g)

CARGO := docker compose run --rm dev cargo

.PHONY: build test lint fmt shell

build:
	mkdir -p dist
	$(CARGO) build --release --locked
	docker compose run --rm dev cp /cargo-target/release/sigstore-shell-cli /app/dist/sigstore-shell-cli

test:
	$(CARGO) test --locked

lint:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --locked --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

shell:
	docker compose run --rm dev bash
