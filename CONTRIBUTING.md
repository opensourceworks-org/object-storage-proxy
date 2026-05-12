# Contributing

Thank you for considering a contribution to object-storage-proxy. The following guidelines help keep the process smooth for everyone.

## Code of conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). Please read it before participating.

## Getting started

1. Fork the repository and clone your fork.
2. Create a feature or fix branch from `main`:
   ```bash
   git checkout -b feat/my-feature
   # or
   git checkout -b fix/my-bug
   ```
3. Set up the development environment (see [BUILD.md](BUILD.md)).
4. Make your changes, add tests, and ensure everything passes.
5. Open a pull request against `main`.

## Development setup

```bash
# Install Rust (stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install uv
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install dependencies and build the extension
uv sync
uv run maturin develop

# Or use the Nix flake
nix develop
```

## Running tests

```bash
# Rust unit tests
cargo test

# With nextest
cargo nextest run

# Python tests
uv run pytest
```

## Code style

### Rust

- Format with `cargo fmt` before committing.
- All `cargo clippy -- -D warnings` findings must be resolved.
- Prefer `?` over `.unwrap()` in non-test code.
- Use `tracing::{debug, info, warn, error}` instead of `println!` or `dbg!`.

### Python

- Type annotations are required on all public functions.
- Docstrings on all public symbols.

## Commit messages

Use the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<scope>): <short summary>

[optional body]

[optional footer]
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`.

Examples:

```
feat(proxy): add path-style to virtual-style translation for AWS
fix(signer): handle missing authorization header gracefully
docs: add CONTRIBUTING guide
```

## Pull request checklist

- [ ] Tests added or updated for every changed behaviour.
- [ ] `cargo fmt` and `cargo clippy` pass with no warnings.
- [ ] `CHANGELOG.md` updated under `Unreleased`.
- [ ] PR description explains the motivation and approach.

## Reporting bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).

## Requesting features

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md).
