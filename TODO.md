# Open-source readiness tasks

## Documentation

- [x] 1. Rewrite README.md — clean up structure, remove personal paths, add license/rust-edition badges, clear opener paragraph, move long code examples to `examples/`.
- [x] 2. Add CONTRIBUTING.md — fork, branch, build, test, PR workflow and commit convention.
- [x] 3. Add CHANGELOG.md — start from 0.4.3 with an Unreleased section.
- [x] 4. Add CODE_OF_CONDUCT.md — Contributor Covenant.
- [x] 5. Add SECURITY.md — responsible disclosure instructions.
- [x] 6. Add .env.example — every environment variable with placeholder values; confirm .env is in .gitignore.
- [x] 7. Update object_storage_proxy.pyi — add missing parameters: `hmac_keystore`, `hmac_fetcher`, `skip_signature_validation`, `verify`, `max_presign_url_usage_attempts`, `server_name`.

## Code quality

- [x] 8. Replace `dbg!` and `println!` with `tracing` calls in production code paths (lib.rs, signer.rs, response.rs, functions.rs).
- [ ] 9. Reduce bare `.unwrap()` — 80 in non-test code; use `?` or `.expect("descriptive message")` in critical paths.
- [x] 10. Fix unused-import compiler warnings — run `cargo fix --lib -p object-storage-proxy`. Zero warnings remain.
- [ ] 11. Run `cargo clippy -- -D warnings` and resolve all findings before publishing.

## Tests

- [ ] 12. Add Rust unit tests for lib.rs — UrlTracker, ProxyServerConfig::default(), ProxyServerConfig::new(), __repr__.
- [ ] 13. Add a minimal pytest test that instantiates ProxyServerConfig and checks repr() without a live backend.
- [ ] 14. Wire Python tests into the CI workflow (currently only cargo test runs).
- [ ] 15. Split CI into `test:rust` and `test:python` jobs so failures are easy to attribute.

## Repository hygiene

- [x] 16. Confirm .gitignore excludes .env, .venv, target/, dist/, *.pem.
- [x] 17. Add GitHub issue templates (bug report, feature request) under .github/ISSUE_TEMPLATE/.
- [x] 18. Add pull request template under .github/PULL_REQUEST_TEMPLATE.md.
- [x] 19. Add `cargo clippy` and `cargo fmt --check` steps to the CI workflow (new `lint` job, added to `release` needs).
- [x] 20. Fix `[tool.uv.workspace]` in pyproject.toml — integration/ has no pyproject.toml, breaking `uv sync` for new contributors.

## Licensing / publishing

- [x] 21. Fix pyproject.toml license classifier — currently says "Proprietary", update to match actual MIT LICENSE file.
- [x] 22. Add metadata to Cargo.toml — license, homepage, repository, description for crates.io / cargo publish.
