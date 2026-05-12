# Build Instructions

`object-storage-proxy` is a mixed Rust/Python project built with [maturin](https://github.com/PyO3/maturin). The Rust library exposes a Python extension module via [PyO3](https://pyo3.rs).

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

`maturin develop` compiles the Rust extension and installs it directly into the active virtual environment as an editable package — the fastest workflow during development.

```bash
# Create/activate a virtual environment and install dev dependencies
uv sync

# Compile the Rust extension and install it into .venv
uv run maturin develop

# With release optimisations (faster runtime, slower build)
uv run maturin develop --release
```

The compiled `.so` is placed inside `.venv` and importable immediately as `import object_storage_proxy`.

---

## Build a wheel

```bash
# Debug wheel (quick, for local testing)
uv run maturin build

# Release wheel (optimised, for distribution)
uv run maturin build --release
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
# Build the extension first (if not already done)
uv run maturin develop

# Start the proxy server
uv run python test_server.py
```

The server listens on:
- HTTP  → `0.0.0.0:6190`
- HTTPS → `0.0.0.0:8443`

---

## Cargo commands (pure Rust)

```bash
# Build the Rust library
cargo build

# Run Rust unit tests
cargo test

# Run tests with nextest
cargo nextest run

# Watch for changes and recompile
cargo watch -x build
```

---

## Linting & formatting

```bash
# Rust
cargo fmt
cargo clippy -- -D warnings

# Python
uv run ruff check .
uv run ruff format .
```
