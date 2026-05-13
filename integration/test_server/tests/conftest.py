"""
conftest.py — shared pytest fixtures for OSP integration tests.

Fixtures
--------
env         Raw config dict loaded from .env
s3          boto3 S3 client pointed at the OSP proxy
s3_direct   boto3 S3 client pointed directly at Garage (bypass proxy)
bucket      Name of the test bucket (from .env)
prefix      A per-test unique key prefix, e.g. "tests/test_put_object/<uuid>/"
aws_env     Dict of env vars ready to pass to subprocess for `aws s3 …` calls
tmp_dir     A fresh temporary directory (pathlib.Path) for each test
"""

from __future__ import annotations

import os
import uuid
import tempfile
import textwrap
from pathlib import Path
from typing import Generator

import boto3
import botocore.config
import pytest
from dotenv import dotenv_values

# ── Load .env once ─────────────────────────────────────────────────────────────

ENV_FILE = Path(__file__).parent.parent / ".env"


def _load_env() -> dict[str, str]:
    if not ENV_FILE.exists():
        pytest.fail(
            f".env not found at {ENV_FILE}.\n"
            "Run `task integration:garage:bootstrap` first."
        )
    values = dotenv_values(ENV_FILE)
    return {k: v for k, v in values.items() if v is not None}


@pytest.fixture(scope="session")
def env() -> dict[str, str]:
    return _load_env()


# ── Service availability checks ────────────────────────────────────────────────

def _check_reachable(url: str, label: str) -> None:
    """Fail the whole session early if a required service is not up."""
    import urllib.request
    import urllib.error

    try:
        urllib.request.urlopen(url, timeout=3)  # noqa: S310
    except urllib.error.HTTPError:
        pass  # got a response → service is up
    except Exception as exc:
        pytest.fail(
            f"{label} not reachable at {url}.\n"
            f"Error: {exc}\n"
            "Run `task integration:up` first."
        )


@pytest.fixture(scope="session", autouse=True)
def require_services(env: dict[str, str]) -> None:
    proxy_url   = f"http://{env.get('OSP_PROXY_HOST', 'localhost')}:{env.get('OSP_PROXY_PORT', '6190')}"
    garage_url  = f"http://{env.get('GARAGE_HOST', 'localhost')}:{env.get('GARAGE_PORT', '3900')}"
    _check_reachable(garage_url, "Garage S3")
    _check_reachable(proxy_url,  "OSP proxy")


# ── boto3 clients ──────────────────────────────────────────────────────────────

def _boto_client(endpoint: str, access_key: str, secret_key: str, region: str) -> "boto3.client":
    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=access_key,
        aws_secret_access_key=secret_key,
        region_name=region,
        config=botocore.config.Config(
            signature_version="s3v4",
            s3={"addressing_style": "path"},
            retries={"max_attempts": 1},
        ),
    )


@pytest.fixture(scope="session")
def s3(env: dict[str, str]):
    """boto3 client → OSP proxy (uses client credentials)."""
    return _boto_client(
        endpoint  = f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}",
        access_key = env["OSP_CLIENT_ACCESS_KEY"],
        secret_key = env["OSP_CLIENT_SECRET_KEY"],
        region    = env.get("GARAGE_REGION", "garage"),
    )


@pytest.fixture(scope="session")
def s3_direct(env: dict[str, str]):
    """boto3 client → Garage directly (uses backend credentials).
    Useful for setup/teardown that doesn't need to go through the proxy."""
    return _boto_client(
        endpoint  = f"http://{env['GARAGE_HOST']}:{env['GARAGE_PORT']}",
        access_key = env["GARAGE_ACCESS_KEY_ID"],
        secret_key = env["GARAGE_SECRET_ACCESS_KEY"],
        region    = env.get("GARAGE_REGION", "garage"),
    )


@pytest.fixture(scope="session")
def bucket(env: dict[str, str]) -> str:
    return env["GARAGE_BUCKET"]


# ── Per-test isolation ─────────────────────────────────────────────────────────

@pytest.fixture
def prefix(request) -> str:
    """Unique per-test key prefix, e.g. 'tests/test_put_object/3f2a…/'."""
    safe_name = request.node.name.replace("[", "_").replace("]", "")
    return f"tests/{safe_name}/{uuid.uuid4().hex[:8]}/"


@pytest.fixture
def tmp_dir() -> Generator[Path, None, None]:
    with tempfile.TemporaryDirectory() as d:
        yield Path(d)


# ── AWS CLI environment ────────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def aws_env(env: dict[str, str], tmp_path_factory) -> dict[str, str]:
    """
    Environment dict for subprocess calls to `aws s3 …`.

    Uses AWS_ENDPOINT_URL (boto3/CLI v2 native) so no profile config is
    needed.  AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_REQUIRED suppresses the
    CLI v2 checksum header that Garage doesn't always expect.

    A temporary AWS config file is written to force path-style addressing,
    because the CLI defaults to virtual-hosted style which OSP doesn't support.
    """
    # Write a minimal AWS config that forces path-style S3 addressing
    cfg_dir = tmp_path_factory.mktemp("aws_cfg")
    cfg_file = cfg_dir / "config"
    cfg_file.write_text(textwrap.dedent("""\
        [default]
        s3 =
            addressing_style = path
    """))

    base = os.environ.copy()
    base.update({
        "AWS_ACCESS_KEY_ID":                  env["OSP_CLIENT_ACCESS_KEY"],
        "AWS_SECRET_ACCESS_KEY":              env["OSP_CLIENT_SECRET_KEY"],
        "AWS_ENDPOINT_URL":                   f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}",
        "AWS_DEFAULT_REGION":                 env.get("GARAGE_REGION", "garage"),
        "AWS_REQUEST_CHECKSUM_CALCULATION":   "WHEN_REQUIRED",
        "AWS_CONFIG_FILE":                    str(cfg_file),
        # Suppress pager output in CLI responses
        "AWS_PAGER":                          "",
    })
    return base
