export DOCKER_UID := $(shell id -u)
export DOCKER_GID := $(shell id -g)

CARGO := docker compose run --rm dev cargo
WIN := x86_64-pc-windows-gnu

.PHONY: build cli-win package dll fixture wine-smoke wine-ui test lint fmt shell

build:
	mkdir -p dist
	$(CARGO) build --release --locked
	docker compose run --rm dev cp /cargo-target/release/sigstore-shell-cli /app/dist/sigstore-shell-cli

cli-win:
	mkdir -p dist
	$(CARGO) build --release --locked --target $(WIN) -p sigstore-shell-cli
	docker compose run --rm dev cp /cargo-target/$(WIN)/release/sigstore-shell-cli.exe /app/dist/sigstore-shell-cli.exe

# Needs dist/ from build, cli-win and dll; VERSION names the zip. --build picks up image changes.
package:
	docker compose run --rm --build dev ./scripts/package.sh $(VERSION)

dll:
	mkdir -p dist
	$(CARGO) build --release --locked --target $(WIN) -p sigstore-shell-ext
	docker compose run --rm dev cp /cargo-target/$(WIN)/release/sigstore_shell_ext.dll /app/dist/sigstore_shell_ext.dll

fixture:
	mkdir -p dist
	$(CARGO) build --release --locked -p provenance-core --features test-fixtures --example fixture_exe
	docker compose run --rm dev /cargo-target/release/examples/fixture_exe /app/dist/fixture.exe cli/cli

# Headless: registers, loads and unregisters the DLL under Wine and walks the COM entry points.
wine-smoke: dll fixture
	$(CARGO) build --release --locked --target $(WIN) -p sigstore-shell-ext --examples
	docker compose run --rm wine ./scripts/wine-smoke.sh

# Same, plus creating the page under a virtual X display and pressing Verify (needs network).
wine-ui: dll fixture
	$(CARGO) build --release --locked --target $(WIN) -p sigstore-shell-ext --examples
	docker compose run --rm wine ./scripts/wine-smoke.sh --ui

test:
	$(CARGO) test --locked --workspace

lint:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --locked --workspace --all-targets -- -D warnings
	$(CARGO) clippy --locked --target $(WIN) -p sigstore-shell-ext --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

shell:
	docker compose run --rm dev bash
