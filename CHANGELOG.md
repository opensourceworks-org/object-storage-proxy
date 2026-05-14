# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.7] - 2026-05-14

### Chores
- Bump version

- Release v0.6.6

- Add lock


### Documentation
- Update CHANGELOG for v0.6.6 [skip ci]


## [0.6.6] - 2026-05-14

### Chores
- Release v0.6.6


### Documentation
- Update CHANGELOG for v0.6.4 [skip ci]


### Testing
- Add duckdb tests


## [0.6.4] - 2026-05-14

### Added
- Remove validation overhead


### Chores
- Release v0.6.4


### Documentation
- Update CHANGELOG for v0.6.3 [skip ci]


## [0.6.3] - 2026-05-14

### Chores
- Release v0.6.3


### Documentation
- Update CHANGELOG for v0.6.2 [skip ci]


### Fixed
- **perf**: Reduce processing overhead

- Improve caching, less overhead per request


## [0.6.2] - 2026-05-14

### Chores
- Release v0.6.2

- Cleanup gitignore

- Fix spark warning test


### Documentation
- Update CHANGELOG for v0.6.0 [skip ci]


### Fixed
- **test**: Add minio backend integration and compatibility fixes

- **ci**: Only trigger ci on tag push (release)


### Testing
- Add more tests, based on aws own s3 sdk tests -- garage does not implement some features, so xfail until alternative found


## [0.6.0] - 2026-05-13

### Added
- Spark streaming


### Chores
- Release v0.6.0


### Documentation
- Update README

- Update README

- Update CHANGELOG for v0.5.5 [skip ci]


## [0.5.5] - 2026-05-13

### Added
- Add base prometheus metrics

- Restrict presigned url usage (tracking/blocking)

- It works

- Send client request details to python validation callable

- Allow path style backends

- Http backends (ie. minio)

- Upgrade to pingora 0.5.0

- Add query parser, we want to pass request to python callbacks for more context

- Add integration

- Spark streaming support

- Upload performance

- Add support for presigned urls

- Adding presigned url support

- Frontend request signature validation

- Drop x- headers for signing request

- Https and http listeners are optional

- Pass in token to python callables

- Working hmac + redact secret_key in debug level for AwsSign struct

- Hmac auth

- Make threads configurable

- Add configurable request counting and only call api_key fetcher once

- Optimize build

- Https frontend

- Earlier exit if no api key

- Working version

- Cos_map is dict

- Validate with cache


### CI
- Auto release

- Add git cliff (changelog)

- Add git precommit

- Fix aarch64 cross-compiler setup for linux build

- Add linux aarch64 build target

- Drop macos-13 x86_64 runner

- Version bump

- Update ci.yml

- Gcc-aarch64-linux-gnu

- Install gcc

- Run it

- Arm install

- Arming

- More arm

- Arm

- Try windows

- Pathelf for python

- Patchelf

- Add cmake

- Musl ln

- Gcc

- Musl typo

- Add musl tools

- Override homes

- Override home

- Home override

- Musl;

- Manual musl

- More musl

- Musl

- Musllinux

- Mac arm64 release

- Mac aarch64

- Typo

- Test runner

- Badges

- Publish

- And drop windows

- Drop musl more

- Drop musl

- Dmn

- Docker options

- Env

- Env

- Set musl openssl location

- O now

- Musl

- Drop aarch64

- Chore

- Env aarch64

- Versions

- All lnx

- Lnx

- Cpan

- Add fi

- Yum or apt

- Jj

- Still

- Back to ci

- Disable actions

- Python

- Still

- Apt

- Apt not yum

- Openssl, of course

- Enving

- Perl

- More perl

- Perl deps

- Vendor openssl

- Maturin generated ci

- Diable sccache

- Init


### Chores
- Release v0.5.5

- Upgrade reqwest

- Upgrade all dependencies (pingora 0.5→0.8, keep pyo3 0.25/reqwest 0.12)

- Prep open

- Update

- Dependencies update

- Vendor openssl for aarch64 builds

- Reuse error response with server, code and message

- License

- Bump version

- Debug

- Bump version

- Update dependencies

- Cleanup

- Add query to python callbacks

- No jars

- Breaking changes

- Bump version

- More amz headers

- Version bump

- Always override payload

- Add conent-encoding header

- Version bump

- Reduce verbosity and move per-request info to debug

- Version bump

- License

- Add musl

- Bump version

- Add license

- Create LICENSE

- Upgrade dependency versions

- Version bump

- Badges

- Version bump

- Version bump

- Version bump

- Reorganize

- Bump version

- Cleanup

- Drop obsolete instance

- Openssl-probe

- Add vendored openssl

- Pkg-config

- Back to rustls

- Fix openssl

- Dmn

- Drop apt

- Out with openssl, in with rustls

- Rollback vendored

- Add the perl dependencies

- Move to opensourceworks-org


### Documentation
- Few fixes, upgrades, update doc

- Add build targets section to README

- Update README and example code

- Update pyproject links

- Update

- Add tasks

- Update docstrings

- Update readme

- Update readme

- Request stages flow

- Update for hmac

- Update for hmac

- Update readme

- Update readme

- Update readme example

- Link update

- Update readme

- Features

- Update

- README.md

- README.md


### Fixed
- **ci**: Linting

- Correct both remaining errors (clippy fmt)

- Correct linting errors

- **ci**: Fix perl deps

- **ci**: Linking against system openssl instead

- **ci**: Missing deps for aarch64

- **ci**: Aarch64 - native compiler

- **ci**: Aarch64 install cross-compiler

- **ci**: Dependency issues from migration to pingora 0.8.0 and rustls defaults

- **ci**: Breaking on aws-lc-rs

- **tests**: Add and fix tests

- **bugs**: Resovle all clippy warnings

- Remove wrap() in critical paths

- **style**: Align trace/logging

- Tests

- Unwrap on fallible result

- Working ssl

- The right credentials and region

- Uploads post awscli 2.23.0

- Fast streaming uploads

- Access_key dropped

- Track header range

- Response headers

- Missing imports

- Strip unwanted headers (proxy)

- Sni with bucket prefix

- Project github links

- Force replace host header


### Testing
- Add  integration test environment and automated tests

- Add more

- Add integration tests

- Add more

- Update python test with hmac keys

- Update comments


### License
- Update license


### Style
- Apply cargo fmt

- Apply cargo fmt
