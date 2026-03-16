# Contributing to Linkup

## Table of contents

- [Prerequisites](#prerequisites)
- [Building from source](#building-from-source)
- [Installing locally (replacing Homebrew)](#installing-locally-replacing-homebrew)
- [Running tests](#running-tests)
- [Multi-instance development](#multi-instance-development)
- [Linting and formatting](#linting-and-formatting)
- [Project structure](#project-structure)

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain — managed automatically via `rust-toolchain.toml`)
- [Wrangler](https://developers.cloudflare.com/workers/wrangler/install-and-update/) (only needed for running the full test suite)

## Building from source

```sh
cargo build
```

To build only the CLI in release mode:

```sh
cargo build --release -p linkup-cli
```

The binary will be at `target/release/linkup`.

## Installing locally (replacing Homebrew)

If you have linkup installed via Homebrew and want to use your local build instead:

```sh
# Remove the Homebrew symlink from PATH (does not uninstall the formula)
brew unlink linkup

# Install from the local checkout into ~/.cargo/bin/
cargo install --path linkup-cli
```

Verify which binary is active:

```sh
which linkup
# Should show: /Users/<you>/.cargo/bin/linkup
linkup --version
```

To go back to the Homebrew version:

```sh
# Remove the cargo-installed binary
cargo uninstall linkup

# Restore the Homebrew symlink
brew link linkup
```

## Running tests

Unit tests (no external dependencies):

```sh
cargo test
```

Full test suite (requires a local Wrangler worker):

```sh
cd worker
cp wrangler.toml.sample wrangler.toml
npx wrangler dev &
# Wait for "Ready on http://localhost:8787"
cd ..
cargo test --all-features
```

## Multi-instance development

Linkup supports running multiple isolated instances for parallel worktree development. Key mechanisms:

- **`LINKUP_HOME` env var** — overrides the default `~/.linkup` state directory. Each instance gets its own state, logs, PID files, and cloudflared config.
- **`local_server_port` config field** — set in `linkup-config.yaml` under `linkup:` to bind the local server to a custom port (HTTPS port = HTTP port + 363).

To test a second instance locally:

```sh
export LINKUP_HOME="$HOME/.linkup/instances/1"
mkdir -p "$LINKUP_HOME"
cp -r ~/.linkup/certs "$LINKUP_HOME/certs"
linkup start -c /path/to/worktree/linkup-config.yaml
```

When an instance is removed with `linkup instance-remove` or `linkup instance-remove-all`, the CLI also performs best-effort cleanup of the instance's Cloudflare tunnel and DNS record via the worker API. If the instance was never started or the worker is unreachable, the cleanup is skipped gracefully — local resources are still cleaned up.

## Linting and formatting

```sh
cargo fmt --all --check   # Check formatting
cargo fmt --all           # Auto-format
cargo clippy              # Lint
```

CI runs these with `RUSTFLAGS="-D warnings"`, so clippy warnings are treated as errors.

## Project structure

| Crate            | Description                              |
| ---------------- | ---------------------------------------- |
| `linkup`         | Core library (routing, session logic)    |
| `linkup-cli`     | The `linkup` CLI binary                  |
| `local-server`   | Local development server (Axum-based)    |
| `worker`         | Cloudflare Worker (compiled to WASM)     |
| `cloudflare`     | Cloudflare API client (fork of cloudflare-rs) |
| `server-tests`   | Integration tests                        |
| `docs/`          | Documentation site (Astro/Starlight)     |
