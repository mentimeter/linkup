# Agents

This is a Rust workspace. The toolchain is pinned to `stable` via `rust-toolchain.toml`.

## Table of contents

- [Build & test](#build--test)
- [Installing locally (replacing Homebrew)](#installing-locally-replacing-homebrew)
- [Workspace crates](#workspace-crates)
- [Key patterns](#key-patterns)
- [Multi-instance support](#multi-instance-support)
- [Gotchas](#gotchas)
- [Known issues](#known-issues)
- [Testing notes](#testing-notes)

## Build & test

```sh
cargo build                  # Build all crates
cargo build -p linkup-cli    # Build only the CLI
cargo test                   # Run unit tests
cargo test --all-features    # Full test suite (needs a local Wrangler worker)
cargo clippy                 # Lint (CI treats warnings as errors)
cargo fmt --all --check      # Check formatting
```

The worker crate compiles to WASM. CI creates stub build artifacts before `cargo check --all`:

```sh
mkdir -p worker/build/worker
touch worker/build/worker/shim.mjs
touch worker/build/worker/index.wasm
```

## Installing locally (replacing Homebrew)

```sh
# Remove the Homebrew symlink from PATH (does not uninstall the formula)
brew unlink linkup

# Install from the local checkout into ~/.cargo/bin/
cargo install --path linkup-cli
```

This places the binary in `~/.cargo/bin/linkup`. The CLI will detect itself as a Cargo install (path contains `.cargo`).

To revert to the Homebrew version:

```sh
# Remove the cargo-installed binary
cargo uninstall linkup

# Restore the Homebrew symlink
brew link linkup
```

## Workspace crates

- **`linkup`** — Core library. No binary, no external services.
- **`linkup-cli`** — CLI binary (`linkup`). Entry point: `linkup-cli/src/main.rs`.
- **`local-server`** — Axum-based local dev server, used by the CLI.
- **`worker`** — Cloudflare Worker. Compiled to WASM via `worker-build`. Not a regular Rust binary.
- **`cloudflare`** — Cloudflare API client (vendored fork of `cloudflare-rs`).
- **`server-tests`** — Integration tests that require a running worker.

## Key patterns

- Error handling uses `anyhow` in the CLI and `thiserror` for typed errors in libraries.
- The CLI detects its installation method (Brew/Cargo/Manual) by inspecting its own binary path.
- The worker crate uses `worker-rs` bindings — it cannot use `tokio` or standard async runtimes. It uses `worker::wasm_bindgen_futures::spawn_local` for async work.
- CI runs on both `ubuntu-latest` and `macos-latest`.

## Multi-instance support

Linkup supports running multiple isolated instances simultaneously (e.g., one per git worktree). This is controlled by environment variables and config fields.

### `LINKUP_HOME` resolution

`linkup_dir_path()` in `linkup-cli/src/main.rs` resolves the state directory with this precedence:

1. **`LINKUP_HOME` env var** — explicit override (e.g., `LINKUP_HOME=~/.linkup/instances/1 linkup start`)
2. **`.env` file walk** — walks up from `cwd` looking for a `.env` file containing `LINKUP_HOME=...`. This lets linkup auto-detect the correct instance when run from inside a worktree without any explicit env var.
3. **`~/.linkup/active-instance`** — set by `linkup instance-use`
4. **`~/.linkup`** — default

### `LINKUP_CONFIG` resolution from `.env`

In **all** of the above cases (including when `LINKUP_HOME` is set via env var), linkup walks up from `cwd` looking for `LINKUP_CONFIG=...` in a `.env` file and **overrides** the process env var with it. This is critical for worktrees: the parent shell often inherits `LINKUP_CONFIG` pointing to the main worktree's `linkup-config.yaml`, but a worktree needs its own `linkup-config.worktree.yaml` with offset ports and a unique session name.

The implementation lives in three functions in `main.rs`:
- `read_dotenv_var(key)` — walks up from cwd, reads `.env` files, returns the first non-empty value for `key`
- `set_linkup_config_from_dotenv()` — calls `read_dotenv_var("LINKUP_CONFIG")` and overrides the env var
- `linkup_home_from_dotenv()` — calls `read_dotenv_var("LINKUP_HOME")` + `set_linkup_config_from_dotenv()`

A companion `default_linkup_dir_path()` always returns `~/.linkup` regardless of `LINKUP_HOME` — use it when you need the canonical path (e.g., scanning all instances).

### `local_server_port` config field

The `linkup:` section of `linkup-config.yaml` accepts an optional `local_server_port` field:

```yaml
linkup:
  worker_url: https://...
  worker_token: ...
  local_server_port: 9080
```

This port is used for the HTTP local server. The HTTPS port is derived as `http_port + 363` (so 80 -> 443, 9080 -> 9443). The derivation lives in `LocalServer::https_port()`.

When omitted, defaults to port 80 (original behavior).

### `session_name` config field

The `linkup:` section of `linkup-config.yaml` accepts an optional `session_name` field:

```yaml
linkup:
  worker_url: https://...
  worker_token: ...
  session_name: rafael2
```

When present, this name is used as `desired_name` when registering with the server. The server will assign this name if available (if not, it assigns a random one).

When omitted, the server assigns a random name and the CLI reuses it on subsequent starts. A config-specified `session_name` always takes priority over the previous state's name.

Note: the mm-js worktree setup generates a `session_name` with a random 3-hex suffix derived from the default session name. This avoids conflicts with Cloudflare-persisted names from destroyed instances while keeping domains recognizable.

### API changes from multi-instance

- `LocalServer::url(port: u16)` — takes a port parameter. Callers must pass the port, typically from `state.linkup.local_server_port.unwrap_or(80)`.
- `LocalServer::new(http_port: u16)` — constructor requires the HTTP port.
- The hidden `Server` subcommand (`LocalWorker` variant) accepts `--http-port` and `--https-port` flags (both default to 80/443 for backward compat).

### Per-instance cloudflared config

Cloudflared configuration and credentials are stored in `LINKUP_HOME` instead of `~/.cloudflared/`:
- Config: `LINKUP_HOME/cloudflared-config.yml` (previously `~/.cloudflared/config.yml`)
- Credentials: `LINKUP_HOME/cloudflared-creds.json` (previously `~/.cloudflared/<tunnel-id>.json`)

The `cloudflared tunnel run` command is started with `--config <LINKUP_HOME>/cloudflared-config.yml`.

### Instance-scoped service IDs

`service_id(base_id)` in `services/mod.rs` produces process-unique identifiers:
- When `LINKUP_HOME` is **not** set: returns the base ID unchanged (e.g., `"linkup-local-server"`)
- When `LINKUP_HOME` **is** set: appends a short hash (e.g., `"linkup-local-server-a1b2c3d4"`)

**Pattern to follow when adding new background services:**
- Always use `service_id(Self::ID)` when setting the `LINKUP_SERVICE_ID` env var on spawned processes.
- Always use `service_id(SomeService::ID)` when calling `find_service_pid()` or `stop_service()`.
- Never pass the raw `Self::ID` const directly to process management functions.

### Instance management commands

- `linkup remove-instance <N>` — stops services for instance N (using `service_id_for_home`), then deletes `~/.linkup/instances/<N>/`.
- `linkup remove-all-instances` — stops services for all instances under `~/.linkup/instances/`, deletes the directory, and removes the `~/.linkup/next-instance` counter.
- `linkup status` — when multiple instances exist (default + any under `~/.linkup/instances/`), prints a padded table with columns `ID`, `DOMAIN`, `PATH`. The current instance (matching `linkup_dir_path()`) is marked with 📌. The domain column shows the full session domain (e.g., `full-hare.mentimeter.dev`). If only one instance exists, the summary is suppressed.

### `service_id_for_home`

`service_id_for_home(base_id, linkup_home)` in `services/mod.rs` computes a scoped service ID for an arbitrary `LINKUP_HOME` path. Used by `remove-instance` and `remove-all-instances` to stop services belonging to other instances without changing the current process's `LINKUP_HOME`.

### Backward compatibility

- `local_server_port` uses `#[serde(default)]` on `LinkupState` so old state files without the field deserialize as `None`.
- `service_id()` only appends a hash when `LINKUP_HOME` is explicitly set — the default instance keeps original IDs, so `linkup stop` works after upgrade without re-starting.
- `ensure_linkup_dir()` auto-creates whatever directory `linkup_dir_path()` returns on CLI startup.

## Gotchas

### Typos that are real identifiers — do NOT rename

These look like typos but are used as actual Cloudflare binding names and env var keys. Renaming them would break deployed infrastructure:

- `CloudflareEnvironemnt` in `worker/src/lib.rs`
- `CLOUDLFLARE_ALL_ZONE_IDS` in `worker/src/lib.rs`, `worker/wrangler.toml.sample`, and `linkup-cli/src/commands/deploy/resources.rs`

### Worker WASM is embedded in the CLI binary

`linkup-cli/build.rs` compiles the worker crate to WASM and copies the artifacts into `OUT_DIR`. The deploy command then includes them via `include_bytes!` in `linkup-cli/src/commands/deploy/resources.rs`. This means:

- Building `linkup-cli` triggers a full worker WASM build.
- CI creates empty stub files (`shim.mjs`, `index.wasm`) so that `cargo check --all` works without a real WASM build.
- `worker-build` is pinned to `0.1.4` in three places (`linkup-cli/build.rs`, `worker/wrangler.toml.sample`, `.github/workflows/ci.yml`) and must be bumped together with the `worker` crate.

### `LocalState::save()` is a no-op in tests

Under `#[cfg(test)]`, `LocalState::save()` returns `Ok(())` without writing anything. Tests never persist state to disk.

### Worker runtime constraints

The worker crate compiles to WASM and runs in Cloudflare's runtime, not a standard OS environment:

- Cannot use `tokio` or standard async runtimes. Uses `spawn_local` from `wasm_bindgen_futures`.
- `reqwest` timeouts and `resolve_ip` are no-ops in WASM.
- The `blocking` API in the cloudflare crate is disabled for `wasm32` targets.

### Platform-specific code paths (Linux vs macOS)

Several CLI commands have `#[cfg(target_os = "linux")]` / `#[cfg(not(target_os = "linux"))]` branches with different behavior:

- **DNS** (`local_dns.rs`): Linux uses `resolvectl`; macOS uses `dscacheutil`/`mDNSResponder`. Linux logs DNS errors as warnings and returns `Ok(())`; macOS returns `Err`.
- **Update** (`update.rs`): Linux uses `sudo` and `setcap cap_net_bind_service=+ep`; macOS uses plain `fs::rename`.
- **Uninstall** (`uninstall.rs`): Linux uses `sudo rm`; macOS uses `fs::remove_file`.
- **Health** (`health.rs`): Linux-only `is_cap_set()` capability check.
- **Certificates** (`local-server/src/certificates/`): No Windows support.

### Vendored cloudflare crate

`cloudflare/` is a fork of [cloudflare-rs](https://github.com/cloudflare/cloudflare-rs). Linkup-specific additions live in `cloudflare/src/linkup.rs` and are not intended to be upstreamed. Other changes (e.g. DNS batch endpoints) have TODOs to upstream.

### Hidden `Server` subcommand

The `Server` variant in the CLI's clap definition is `#[clap(hide = true)]`. It is not user-facing — the CLI spawns it internally to run the local server. The `LocalWorker` variant accepts `--http-port` and `--https-port` flags (defaulting to 80 and 443) for multi-instance port isolation.

### `serde_yaml` is intentionally on a deprecated version

`linkup-cli/Cargo.toml` pins `serde_yaml = "0.9.34-deprecated"`. This is deliberate, not an accident.

## Shell completions

After adding or renaming CLI commands, regenerate shell completions so tab-completion stays current:

```bash
linkup completion --shell zsh | sudo tee /usr/local/share/zsh/site-functions/_linkup > /dev/null && exec zsh
```

For bash: `linkup completion --shell bash > ~/.local/share/bash-completion/completions/linkup`

## Known issues

- `ConfigError::InvalidRegex` in `linkup/src/session.rs` uses `{0}` twice in its format string instead of `{0}, {1}`, so the regex error is not displayed.
- `WildcardSniResolver` in `local-server/src/certificates/wildcard_sni_resolver.rs` checks `path.starts_with("linkup_ca")` on a full path, which never matches for absolute paths. Should use `file_name.starts_with("linkup_ca")`.

## Testing notes

- `cargo test` runs unit tests only. `cargo test --all-features` runs the full suite including integration tests that require a Wrangler worker on `localhost:8787`.
- Worker detection in `server-tests/tests/helpers.rs` uses `lsof` (Unix-only).
- Tests use `rstest` with `#[values(ServerKind::Local, ServerKind::Worker)]`, so each test runs against both the local server and the worker.

### Multi-instance tests

- `service_id()` tests in `services/mod.rs` manipulate `LINKUP_HOME` via `std::env::set_var`/`remove_var`. Since env vars are process-global, these tests use a `Mutex` (`ENV_MUTEX`) to run serially and save/restore the previous value.
- `local_server_port` tests in `local_config.rs` verify config parsing (`config_to_state`), YAML serialization roundtrip, and backward-compatible deserialization of state files that lack the `local_server_port` field.
- `LocalServer::url()` and `https_port()` tests in `services/local_server.rs` verify URL construction and port derivation arithmetic.
- When writing new tests that touch `LINKUP_HOME`, always acquire `ENV_MUTEX` first, save the previous value, and restore it in all code paths (including early returns).
