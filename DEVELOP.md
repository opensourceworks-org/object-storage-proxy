# Developer Guide

`object-storage-proxy` is a mixed Rust/Python project built with [maturin](https://github.com/PyO3/maturin). The Rust library exposes a Python extension module via [PyO3](https://pyo3.rs).

[Task](https://taskfile.dev) is used as the task runner. All common workflows have a `task` shortcut — run `task` (or `task --list`) to see everything available.

---

## Quick reference

| Task | What it does |
|------|-------------|
| `task setup` | `uv sync` — install all Python dev dependencies |
| `task build` | dev build + install `.so` into `.venv` |
| `task build:release` | release build + install `.so` into `.venv` |
| `task run` | build (dev) then start `test_server.py` |
| `task run:release` | build (release) then start `test_server.py` |
| `task test:rust` | Rust unit tests via `cargo nextest` |
| `task test:rust:cargo` | Rust unit tests via plain `cargo test` |
| `task test:integration` | run `test_integration.sh` |
| `task test` | build → `test:rust` → `test:integration` |
| `task fmt` | `cargo fmt` |
| `task lint` | `cargo clippy -- -D warnings` |
| `task wheel` | debug wheel → `target/wheels/` |
| `task wheel:release` | release wheel → `target/wheels/` |
| `task clean` | remove Rust artefacts and `.venv` |
| `task clean:wheels` | remove `target/wheels/` only |
| `task integration:run` | automated integration test: up → test → down |
| `task integration:up` | start Garage + bootstrap + OSP proxy |
| `task integration:down` | stop proxy + stop Garage |
| `task integration:test` | run pytest suite against the running environment |
| `task hooks:install` | install (or re-install) the pre-commit git hooks |
| `task hooks:run` | run all pre-commit checks against every file |
| `task hooks:update` | bump pre-commit hook revisions to latest |

> **NixOS note:** `maturin develop` can silently leave a stale `.so` in site-packages due to hard-link restrictions.
> The `build` and `build:release` tasks work around this by copying the freshly-compiled `.so` directly.
> Always use `task build` / `task build:release` rather than calling `maturin develop` by hand.

---

## Prerequisites

| Tool | Purpose |
|------|---------|
| Rust (stable) | compile the core library |
| Python ≥ 3.10 | host the extension module |
| [uv](https://docs.astral.sh/uv/) | Python package/venv manager |
| `maturin` | build the PyO3 extension wheel |

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install uv

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

### Nix (optional)

A `flake.nix` is provided that sets up the full Rust + Python toolchain in an isolated shell:

```bash
nix develop
```

---

## Development build (editable install)

`maturin develop` compiles the Rust extension and installs it directly into the active virtual environment — the fastest workflow during development.

```bash
# First-time setup: create .venv and install dev dependencies
task setup

# Compile the Rust extension and install it into .venv (dev build)
task build

# With release optimisations (faster runtime, slower build)
task build:release
```

The compiled `.so` is placed inside `.venv` and importable immediately as `import object_storage_proxy`.

---

## Build a wheel

```bash
task wheel          # debug wheel (quick, for local testing)
task wheel:release  # release wheel (optimised, for distribution)
```

Wheels are written to `target/wheels/`.

---

## Install from wheel

```bash
pip install target/wheels/object_storage_proxy-*.whl
```

---

## Run the test server

`test_server.py` starts a local proxy server. It requires several environment variables (see `.env.example` or the table below).

### Required environment variables

| Variable | Description |
|----------|-------------|
| `COS_API_KEY` | IBM COS API key (used for `bucket1`, `bucket2`, `proxy-bucket01`) |
| `ACCESS_KEY` / `SECRET_KEY` | Default HMAC keypair |
| `LOCAL2_ACCESS_KEY` / `LOCAL2_SECRET_KEY` | HMAC keypair added to the keystore |
| `PROXY_BUCKET05_ACCESS_KEY` / `PROXY_BUCKET05_SECRET_KEY` | HMAC keypair for `proxy-bucket05` |
| `AWS_ACCESS_KEY` / `AWS_SECRET_KEY` | AWS keypair for `proxy-aws-bucket01` |

Copy `.env.example` to `.env` (if provided) or export the variables manually, then run:

```bash
task run          # dev build → start test_server.py
task run:release  # release build → start test_server.py
```

The server listens on:
- HTTP  → `0.0.0.0:6190`
- HTTPS → `0.0.0.0:8443`

---

## Running unit tests

```bash
task test:rust        # Rust unit tests via cargo nextest (recommended)
task test:rust:cargo  # Rust unit tests via plain cargo test
task test             # full suite: build → rust tests → integration tests
```

`cargo nextest` runs tests in parallel and gives richer output.  Install it once with:

```bash
cargo install cargo-nextest
```

To run a single test by name:

```bash
cargo test <test_name>
# e.g.
cargo test url_tracker_track_increments_count
```

---

## Linting & formatting

```bash
task fmt   # cargo fmt
task lint  # cargo clippy -- -D warnings
```

For Python:

```bash
uv run ruff check .
uv run ruff format .
```

---

## Pre-commit hooks

`task setup` installs the hooks automatically. To install them manually:

```bash
task hooks:install
```

The hooks run on every `git commit` and check:

| Hook | What it checks |
|------|---------------|
| `cargo fmt` | Rust formatting (fails if diff) |
| `cargo clippy` | Rust lints (`-D warnings`) |
| `ruff` | Python lints (auto-fixes staged files) |
| `ruff-format` | Python formatting |
| trailing whitespace | All text files |
| end-of-file newline | All text files |
| valid YAML / TOML | Config files |
| merge conflict markers | All files |

To run all checks manually without committing:

```bash
task hooks:run
```

To upgrade hook versions:

```bash
task hooks:update
```

---

## Integration testing

The integration test suite runs OSP against a real [Garage](https://garagehq.deuxfleurs.fr/) S3-compatible storage node inside Docker. All tests live under `integration/test_server/tests/` and use pytest + boto3.

### Prerequisites

- Docker (with Compose v2)
- `aws` CLI v2 (used by the CLI test suite)

### One-shot: automated run

```bash
task integration:setup    # install test Python deps (once)
task integration:run      # garage up → bootstrap → proxy → test → teardown
```

`integration:run` tears everything down even if tests fail.

### Manual workflow (recommended for development)

Bring the stack up once and iterate on tests without restarting:

```bash
task integration:setup           # install test Python deps (once)
task integration:garage:up       # start Garage container
task integration:garage:bootstrap  # create bucket + HMAC key, write .env
task integration:server:start    # start the OSP proxy in the background
```

Run the tests:

```bash
task integration:test            # full suite
task integration:test:fast       # stop on first failure (-x)
```

When done:

```bash
task integration:down            # stop proxy + stop Garage
task integration:garage:destroy  # also wipe Garage data volumes
```

### Task reference

| Task | Description |
|------|-------------|
| `integration:setup` | `uv sync` in `integration/test_server/` |
| `integration:up` | Garage up → bootstrap → proxy start |
| `integration:down` | Stop proxy → stop Garage |
| `integration:run` | Automated: up → test → down (teardown on failure too) |
| `integration:garage:up` | Start the Garage Docker container |
| `integration:garage:down` | Stop and remove the container |
| `integration:garage:destroy` | Stop container **and** remove data volumes |
| `integration:garage:bootstrap` | Create bucket + HMAC key in Garage, write `.env` |
| `integration:garage:logs` | Follow Garage container logs |
| `integration:garage:status` | Query Garage cluster status via admin API |
| `integration:server:start` | Start OSP proxy in the background (logs → `proxy.log`) |
| `integration:server:stop` | Stop the background proxy |
| `integration:server:logs` | Tail the proxy log |
| `integration:test` | Run pytest suite against the running environment |
| `integration:test:fast` | Same, with `-x` (stop on first failure) |

### What the tests cover

| File | Coverage |
|------|----------|
| `tests/test_objects.py` | PutObject, GetObject, HeadObject, CopyObject, DeleteObject, DeleteObjects, ListObjectsV2 |
| `tests/test_multipart.py` | CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload, ListParts |
| `tests/test_presigned.py` | Presigned GET/PUT, expiry enforcement, repeated-use limiting |
| `tests/test_aws_cli.py` | `aws s3 ls`, `cp`, `sync`, `rm` via subprocess |

### Ports used

| Port | Service |
|------|---------|
| 3900 | Garage S3 API |
| 3901 | Garage RPC |
| 3903 | Garage admin API |
| 6190 | OSP proxy (S3 frontend) |
| 9091 | OSP Prometheus metrics |

### Environment

`bootstrap.py` writes `integration/test_server/.env` with the generated Garage credentials. `server.py` and the pytest fixtures both load this file automatically. The file is gitignored — never commit it.
