"""
conftest.py — shared pytest fixtures for OSP integration tests.

Fixtures
--------
backend     Parametrized over ["garage", "minio"]; all tests run against both.
            MinIO is skipped automatically when .env.minio is absent.
env         Raw config dict loaded from .env (Garage)
s3          boto3 S3 client -> OSP proxy; parametrized via backend
s3_direct   boto3 S3 client -> Garage directly (bypass proxy)
bucket      Name of the test bucket; parametrized via backend
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

import hashlib
import base64

# ── Constants ─────────────────────────────────────────────────────────────────

ENV_FILE = Path(__file__).parent.parent / ".env"


def _register_delete_objects_md5(client) -> None:
    """Inject Content-MD5 for DeleteObjects before signing.

    botocore >=1.43 switched from Content-MD5 to x-amz-checksum-crc32 for
    DeleteObjects.  Both Garage and MinIO still require Content-MD5 per the
    AWS S3 spec, so we compute and add it before the signature is calculated.
    """

    def _inject(request, **kwargs):
        body = request.body
        if body is None:
            return
        if isinstance(body, str):
            body = body.encode("utf-8")
        request.headers["Content-MD5"] = base64.b64encode(
            hashlib.md5(body).digest()
        ).decode()

    client.meta.events.register("before-sign.s3.DeleteObjects", _inject)


# ── Load .env once ───────────────────────────────────────────────────────


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
        pass  # got a response -> service is up
    except Exception as exc:
        pytest.fail(
            f"{label} not reachable at {url}.\n"
            f"Error: {exc}\n"
            "Run `task integration:up` first."
        )


@pytest.fixture(scope="session", autouse=True)
def require_services(env: dict[str, str]) -> None:
    proxy_url = f"http://{env.get('OSP_PROXY_HOST', 'localhost')}:{env.get('OSP_PROXY_PORT', '6190')}"
    garage_url = (
        f"http://{env.get('GARAGE_HOST', 'localhost')}:{env.get('GARAGE_PORT', '3900')}"
    )
    _check_reachable(garage_url, "Garage S3")
    _check_reachable(proxy_url, "OSP proxy")


# ── Backend parametrization ───────────────────────────────────────────────────


@pytest.fixture(scope="session", params=["garage", "minio"])
def backend(request) -> str:
    """Parametrize the whole test suite over both S3 backends.

    Every test that depends on ``s3`` or ``bucket`` runs twice: once against
    Garage, once against MinIO.  The MinIO parameter is skipped automatically
    when ``.env.minio`` is absent (i.e. MinIO is not running).
    """
    if request.param == "minio":
        _load_minio_env()  # calls pytest.skip() when .env.minio is absent
    return request.param


# ── boto3 clients ──────────────────────────────────────────────────────────────


def _boto_client(
    endpoint: str,
    access_key: str,
    secret_key: str,
    region: str,
    response_checksum_validation: str | None = None,
) -> "boto3.client":
    cfg_kwargs: dict = {
        "signature_version": "s3v4",
        "s3": {"addressing_style": "path"},
        "retries": {"max_attempts": 1},
        # Ensure botocore continues to send Content-MD5 for operations that
        # require it (e.g. DeleteObjects) rather than switching to
        # x-amz-checksum-* which some backends don't accept.
        "request_checksum_calculation": "when_required",
    }
    if response_checksum_validation is not None:
        cfg_kwargs["response_checksum_validation"] = response_checksum_validation
    client = boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=access_key,
        aws_secret_access_key=secret_key,
        region_name=region,
        config=botocore.config.Config(**cfg_kwargs),
    )
    _register_delete_objects_md5(client)
    return client


@pytest.fixture(scope="session")
def s3(backend: str, env: dict[str, str]):
    """boto3 client -> OSP proxy.  Parametrized: Garage or MinIO bucket."""
    if backend == "garage":
        return _boto_client(
            endpoint=f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}",
            access_key=env["OSP_CLIENT_ACCESS_KEY"],
            secret_key=env["OSP_CLIENT_SECRET_KEY"],
            region=env.get("GARAGE_REGION", "garage"),
        )
    menv = _load_minio_env()
    return _boto_client(
        endpoint=f"http://{menv['OSP_PROXY_HOST']}:{menv['OSP_PROXY_PORT']}",
        access_key=menv["OSP_CLIENT_ACCESS_KEY"],
        secret_key=menv["OSP_CLIENT_SECRET_KEY"],
        region=menv.get("MINIO_REGION", "us-east-1"),
    )


@pytest.fixture(scope="session")
def s3_nochecksum(backend: str, env: dict[str, str]):
    """boto3 client -> OSP proxy with response checksum validation disabled.

    Use for tests that make ranged GetObject requests; without this, botocore
    would compare the full-object checksum header against the partial body and
    raise FlexibleChecksumError.
    """
    if backend == "garage":
        return _boto_client(
            endpoint=f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}",
            access_key=env["OSP_CLIENT_ACCESS_KEY"],
            secret_key=env["OSP_CLIENT_SECRET_KEY"],
            region=env.get("GARAGE_REGION", "garage"),
            response_checksum_validation="when_required",
        )
    menv = _load_minio_env()
    return _boto_client(
        endpoint=f"http://{menv['OSP_PROXY_HOST']}:{menv['OSP_PROXY_PORT']}",
        access_key=menv["OSP_CLIENT_ACCESS_KEY"],
        secret_key=menv["OSP_CLIENT_SECRET_KEY"],
        region=menv.get("MINIO_REGION", "us-east-1"),
        response_checksum_validation="when_required",
    )


@pytest.fixture(scope="session")
def s3_direct(env: dict[str, str]):
    """boto3 client -> Garage directly (uses backend credentials).
    Useful for setup/teardown that doesn't need to go through the proxy."""
    return _boto_client(
        endpoint=f"http://{env['GARAGE_HOST']}:{env['GARAGE_PORT']}",
        access_key=env["GARAGE_ACCESS_KEY_ID"],
        secret_key=env["GARAGE_SECRET_ACCESS_KEY"],
        region=env.get("GARAGE_REGION", "garage"),
    )


@pytest.fixture(scope="session")
def bucket(backend: str, env: dict[str, str]) -> str:
    if backend == "garage":
        return env["GARAGE_BUCKET"]
    return _load_minio_env()["MINIO_BUCKET"]


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
    cfg_file.write_text(
        textwrap.dedent("""\
        [default]
        s3 =
            addressing_style = path
    """)
    )

    base = os.environ.copy()
    base.update(
        {
            "AWS_ACCESS_KEY_ID": env["OSP_CLIENT_ACCESS_KEY"],
            "AWS_SECRET_ACCESS_KEY": env["OSP_CLIENT_SECRET_KEY"],
            "AWS_ENDPOINT_URL": f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}",
            "AWS_DEFAULT_REGION": env.get("GARAGE_REGION", "garage"),
            "AWS_REQUEST_CHECKSUM_CALCULATION": "WHEN_REQUIRED",
            "AWS_CONFIG_FILE": str(cfg_file),
            # Suppress pager output in CLI responses
            "AWS_PAGER": "",
        }
    )
    return base


# ── SparkSession ───────────────────────────────────────────────────────────────


@pytest.fixture(scope="session")
def spark_session(env: dict[str, str]):
    """
    A session-scoped SparkSession wired to the OSP proxy via s3a (Garage backend).

    Spark is slow to start (~20-40 s on first run while Ivy downloads
    hadoop-aws), so we share one session across all spark tests.
    """
    import sys

    sys.path.insert(0, str(Path(__file__).parent.parent))
    from spark import build_spark_session  # noqa: PLC0415

    proxy_endpoint = f"http://{env['OSP_PROXY_HOST']}:{env['OSP_PROXY_PORT']}"
    spark = build_spark_session(
        access_key=env["OSP_CLIENT_ACCESS_KEY"],
        secret_key=env["OSP_CLIENT_SECRET_KEY"],
        endpoint=proxy_endpoint,
        region=env.get("GARAGE_REGION", "garage"),
    )
    yield spark
    spark.stop()


# ── DuckDB connection ──────────────────────────────────────────────────────────


def _duckdb_connect(
    host: str, port: str, access_key: str, secret_key: str, region: str
):
    import duckdb  # noqa: PLC0415

    con = duckdb.connect()
    con.execute("INSTALL httpfs")
    con.execute("LOAD httpfs")
    con.execute("SET s3_url_style='path'")
    con.execute(f"SET s3_endpoint='{host}:{port}'")
    con.execute(f"SET s3_access_key_id='{access_key}'")
    con.execute(f"SET s3_secret_access_key='{secret_key}'")
    con.execute(f"SET s3_region='{region}'")
    con.execute("SET s3_use_ssl=false")
    return con


@pytest.fixture(scope="session")
def duckdb_conn(backend: str, env: dict[str, str]):
    """
    A session-scoped DuckDB connection pre-configured with httpfs pointing at
    the OSP proxy.  Parametrized via ``backend``: runs against both Garage and
    MinIO (MinIO is skipped automatically when .env.minio is absent).

    The ``httpfs`` extension is installed and loaded once; S3 credentials and
    endpoint are configured via ``SET`` statements so every subsequent query
    can use ``s3://`` paths transparently.
    """
    if backend == "garage":
        con = _duckdb_connect(
            host=env["OSP_PROXY_HOST"],
            port=env["OSP_PROXY_PORT"],
            access_key=env["OSP_CLIENT_ACCESS_KEY"],
            secret_key=env["OSP_CLIENT_SECRET_KEY"],
            region=env.get("GARAGE_REGION", "garage"),
        )
    else:
        menv = _load_minio_env()
        con = _duckdb_connect(
            host=menv["OSP_PROXY_HOST"],
            port=menv["OSP_PROXY_PORT"],
            access_key=menv["OSP_CLIENT_ACCESS_KEY"],
            secret_key=menv["OSP_CLIENT_SECRET_KEY"],
            region=menv.get("MINIO_REGION", "us-east-1"),
        )
    yield con
    con.close()


ENV_MINIO_FILE = Path(__file__).parent.parent / ".env.minio"


def _load_minio_env() -> dict[str, str]:
    if not ENV_MINIO_FILE.exists():
        pytest.skip(
            f".env.minio not found at {ENV_MINIO_FILE}.\n"
            "Run `task integration:minio:bootstrap` first."
        )
    from dotenv import dotenv_values

    values = dotenv_values(ENV_MINIO_FILE)
    return {k: v for k, v in values.items() if v is not None}


@pytest.fixture(scope="session")
def minio_env() -> dict[str, str]:
    return _load_minio_env()


@pytest.fixture(scope="session")
def minio_bucket(minio_env: dict[str, str]) -> str:
    return minio_env["MINIO_BUCKET"]


@pytest.fixture(scope="session")
def spark_session_minio(minio_env: dict[str, str]):
    """
    A session-scoped SparkSession wired to the OSP proxy via s3a (MinIO backend).

    Uses the same proxy endpoint as the Garage session but targets the MinIO
    bucket registered in the cos_map.  Skipped automatically when .env.minio
    is absent (i.e. MinIO is not running).
    """
    import sys

    sys.path.insert(0, str(Path(__file__).parent.parent))
    from spark import build_spark_session  # noqa: PLC0415

    proxy_endpoint = (
        f"http://{minio_env['OSP_PROXY_HOST']}:{minio_env['OSP_PROXY_PORT']}"
    )
    spark = build_spark_session(
        access_key=minio_env["OSP_CLIENT_ACCESS_KEY"],
        secret_key=minio_env["OSP_CLIENT_SECRET_KEY"],
        endpoint=proxy_endpoint,
        region=minio_env.get("MINIO_REGION", "us-east-1"),
    )
    yield spark
    spark.stop()
