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
| `task test` | build -> `test:rust` -> `test:integration` |
| `task fmt` | `cargo fmt` |
| `task lint` | `cargo clippy -- -D warnings` |
| `task wheel` | debug wheel -> `target/wheels/` |
| `task wheel:release` | release wheel -> `target/wheels/` |
| `task clean` | remove Rust artefacts and `.venv` |
| `task clean:wheels` | remove `target/wheels/` only |
| `task integration:run` | automated integration test: up -> test -> down |
| `task integration:up` | start Garage + bootstrap + OSP proxy |
| `task integration:down` | stop proxy + stop Garage |
| `task integration:test` | run pytest suite (excludes Spark) |
| `task integration:test:spark` | run only the Spark tests (`-m spark`) |
| `task integration:test:all` | run full suite including Spark tests |
| `task integration:test:spark` | run only the Spark tests (`-m spark`) |
| `task integration:test:all` | run full suite including Spark tests |
| `task hooks:install` | install (or re-install) the pre-commit git hooks |
| `task hooks:run` | run all pre-commit checks against every file |
| `task hooks:update` | bump pre-commit hook revisions to latest |
| `task changelog` | regenerate `CHANGELOG.md` from full git history |
| `task changelog:unreleased` | preview commits since the last tag |

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
task run          # dev build -> start test_server.py
task run:release  # release build -> start test_server.py
```

The server listens on:
- HTTP  -> `0.0.0.0:6190`
- HTTPS -> `0.0.0.0:8443`

---

## Running unit tests

```bash
task test:rust        # Rust unit tests via cargo nextest (recommended)
task test:rust:cargo  # Rust unit tests via plain cargo test
task test             # full suite: build -> rust tests -> integration tests
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

`task setup` installs the hooks automatically. To install them manually on an existing checkout:

```bash
task hooks:install
```

The hooks run on every `git commit` and enforce:

| Hook | Scope | What it checks |
|------|-------|---------------|
| `cargo fmt` | `*.rs` | Rust formatting — fails if a diff would be produced |
| `cargo clippy` | `*.rs` | Rust lints (`-D warnings`) |
| `ruff` (lint) | `*.py` | Python lints — auto-fixes staged files |
| `ruff-format` | `*.py` | Python formatting |
| trailing whitespace | source files | No trailing spaces |
| end-of-file newline | source files | Files end with exactly one newline |
| check-yaml | `*.yml / *.yaml` | Valid YAML syntax |
| check-toml | `*.toml` | Valid TOML syntax |
| merge conflict markers | all | No leftover `<<<<<<<` markers |

The following paths are excluded from all general hooks (they are third-party configs, build artefacts, or binary-ish files):

- `target/` — Rust build output
- `*.lock` — `Cargo.lock`, `uv.lock`
- `img/`, `*.svg` — image assets
- `integration/presto/`, `integration/trino/` — third-party Hadoop/Hive configs
- `*.properties`, `*.xml`, `*.cnf` — Java / OpenSSL config files

To run all checks against every file without committing:

```bash
task hooks:run
```

To bump hook revisions to their latest tagged versions:

```bash
task hooks:update
```

---

## Changelog

`CHANGELOG.md` is generated from git commit history using [git-cliff](https://git-cliff.org/).
Commits must follow the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>[optional scope]: <description>

feat: add presigned URL expiry enforcement
fix(ci): correct aarch64 linker flags
docs: update configuration reference
chore: bump dependency versions
```

Supported types and the changelog sections they map to:

| Type | Section |
|------|---------|
| `feat` | Added |
| `fix` | Fixed |
| `perf` | Performance |
| `refactor` | Changed |
| `docs` / `doc` | Documentation |
| `test` | Testing |
| `ci` | CI |
| `chore` | Chores |
| `revert` | Reverted |

Append `!` or add `BREAKING CHANGE:` in the footer to mark a breaking change.

### Regenerate the full changelog

```bash
task changelog
```

This overwrites `CHANGELOG.md` with the full history derived from all tags.

### Preview unreleased changes

```bash
task changelog:unreleased
```

Prints the section that would be added for commits since the last tag — useful before tagging a release.

### Generate for a specific range

```bash
task changelog:tag -- v0.5.3..v0.5.4
```

Configuration lives in [`cliff.toml`](cliff.toml).

### Tagging a release

`Cargo.toml` is the single source of truth for the version. The workflow is:

1. Edit `version = "…"` in `Cargo.toml`.
2. Run `task release`.

```bash
# 1. bump version in Cargo.toml, then:
task release
```

What it does:

1. Reads the version from `Cargo.toml`.
2. Runs `git cliff --tag vX.Y.Z -o CHANGELOG.md` to regenerate the full changelog.
3. Commits `Cargo.toml` and `CHANGELOG.md` with `chore: release vX.Y.Z`.
3. Creates an annotated tag `vX.Y.Z`.
4. Pushes the commit and tag.

### GitHub Actions release workflow

Pushing a `v*` tag triggers `.github/workflows/release.yml`, which:

1. Installs `git-cliff` and regenerates `CHANGELOG.md` from the tag.
2. Commits the updated changelog back to `main` (with `[skip ci]` to avoid loops).
3. Creates a GitHub Release with the changelog as the release body.

---

## Integration testing

The integration test suite runs OSP against a real [Garage](https://garagehq.deuxfleurs.fr/) S3-compatible storage node inside Docker. All tests live under `integration/test_server/tests/` and use pytest + boto3.

### Prerequisites

- Docker (with Compose v2)
- `aws` CLI v2 (used by the CLI test suite)

### One-shot: automated run

```bash
task integration:setup    # install test Python deps (once)
task integration:run      # garage up -> bootstrap -> proxy -> test -> teardown
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
| `integration:up` | Garage up -> bootstrap -> proxy start |
| `integration:down` | Stop proxy -> stop Garage |
| `integration:run` | Automated: up -> test -> down (teardown on failure too) |
| `integration:garage:up` | Start the Garage Docker container |
| `integration:garage:down` | Stop and remove the container |
| `integration:garage:destroy` | Stop container **and** remove data volumes |
| `integration:garage:bootstrap` | Create bucket + HMAC key in Garage, write `.env` |
| `integration:garage:logs` | Follow Garage container logs |
| `integration:garage:status` | Query Garage cluster status via admin API |
| `integration:server:start` | Start OSP proxy in the background (logs -> `proxy.log`) |
| `integration:server:stop` | Stop the background proxy |
| `integration:server:logs` | Tail the proxy log |
| `integration:test` | Run pytest suite against the running environment |
| `integration:test:fast` | Same, with `-x` (stop on first failure) |
| `integration:test:spark` | Run only the Spark tests (`-m spark`) |
| `integration:test:spark:fast` | Same, with `-x` (stop on first failure) |
| `integration:test:all` | Run full suite including Spark tests |

### What the tests cover

| File | Coverage |
|------|----------|
| `tests/test_objects.py` | PutObject, GetObject, HeadObject, CopyObject, DeleteObject, DeleteObjects, ListObjectsV2 |
| `tests/test_multipart.py` | CreateMultipartUpload, UploadPart, CompleteMultipartUpload, AbortMultipartUpload, ListParts |
| `tests/test_presigned.py` | Presigned GET/PUT, expiry enforcement, repeated-use limiting |
| `tests/test_aws_cli.py` | `aws s3 ls`, `cp`, `sync`, `rm` via subprocess |
| `tests/test_spark.py` | Spark s3a read/write: Parquet, JSON, overwrite, empty DataFrame, large DataFrame (10 000 rows) |

### Spark tests

The Spark test suite (`tests/test_spark.py`) exercises PySpark's `s3a://` connector against OSP.
Tests are marked `spark` and skipped by default unless explicitly requested, because PySpark startup takes 20–40 s on first run (Ivy downloads `hadoop-aws`).

#### Additional prerequisites

- Java 11 or 17 on `PATH` (`java -version`)
- PySpark + `pyspark` Python package (installed by `task integration:setup`)

#### Run Spark tests only

```bash
task integration:test:spark          # all 5 Spark tests
task integration:test:spark:fast     # stop on first failure
```

Or via pytest directly:

```bash
cd integration/test_server
uv run pytest -m spark tests/test_spark.py -v
```

#### Run everything (including Spark)

```bash
task integration:test:all
```

#### Spark smoke-test (standalone)

`spark.py` doubles as a runnable smoke-test that writes two rows and reads them back:

```bash
cd integration/test_server
uv run python spark.py
```

Expected output:

```
+---+-----+
| id|  msg|
+---+-----+
|  1|hello|
|  2|world|
+---+-----+
✅  Smoke test passed — 2 rows round-tripped via s3a://test-bucket/spark-smoke-test/
```

#### How Spark writes to S3A (and why it matters for OSP)

Spark's `FileOutputCommitter` (algorithm v2) writes data files directly to the final destination using a two-phase flow:

1. **Streaming PUT** — the file body is sent with `content-encoding: aws-chunked`, `transfer-encoding: chunked`, and `x-amz-content-sha256: STREAMING-AWS4-HMAC-SHA256-PAYLOAD`.  Each chunk is independently signed with a chain of HMAC-SHA256 signatures.
2. **CopyObject** — when committing, Spark renames the `_temporary/…` staging file to its final path by issuing a `CopyObject` request that includes both `x-amz-copy-source` and `x-amz-copy-source-if-match` in `SignedHeaders`.

OSP handles both cases transparently:

- The **streaming PUT** body is decoded from the client's aws-chunked framing, and the raw payload chunks are re-signed with the Garage backend credentials before forwarding.
- The **CopyObject** canonical request is built by sorting headers by key name only (not by `key:value` string), matching the AWS SigV4 specification.

#### SparkSession configuration

`integration/test_server/spark.py` configures the session via `build_spark_session()`:

```python
from spark import build_spark_session

spark = build_spark_session(
    access_key="...",
    secret_key="...",
    endpoint="http://localhost:6190",   # OSP proxy
    region="garage",
)
```

Key Hadoop S3A settings applied:

| Setting | Value | Why |
|---------|-------|-----|
| `fs.s3a.impl` | `S3AFileSystem` | Use the S3A connector |
| `fs.s3a.aws.credentials.provider` | `SimpleAWSCredentialsProvider` | Static key/secret — no IAM |
| `fs.s3a.path.style.access` | `true` | OSP requires path-style addressing |
| `fs.s3a.connection.ssl.enabled` | `false` | Plain HTTP for local testing |
| `spark.jars.packages` | `org.apache.hadoop:hadoop-aws:3.5.0` | Pulled via Ivy on first run |

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
