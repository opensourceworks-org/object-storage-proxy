"""
server.py — minimal OSP proxy for integration testing.

Reads Garage and (optionally) MinIO backend credentials from .env / .env.minio
(written by bootstrap.py / minio_bootstrap.py) and exposes all configured
buckets through a single proxy on http://localhost:6190.

Run with the *root* project venv (which has object_storage_proxy installed):
    uv run --no-sync python integration/test_server/server.py

Or via Taskfile:
    task integration:server:start
"""

from __future__ import annotations

import os
from pathlib import Path

from dotenv import load_dotenv

HERE = Path(__file__).parent

# Load Garage .env (always required)
load_dotenv(HERE / ".env")
# Load MinIO .env on top if present (values don't overlap — different key names)
load_dotenv(HERE / ".env.minio", override=False)

# ── Import OSP from the root venv ─────────────────────────────────────────────
from object_storage_proxy import ProxyServerConfig, start_server  # noqa: E402

# ── Garage backend credentials ────────────────────────────────────────────────
GARAGE_HOST = os.environ.get("GARAGE_HOST", "localhost")
GARAGE_PORT = int(os.environ.get("GARAGE_PORT", "3900"))
GARAGE_REGION = os.environ.get("GARAGE_REGION", "garage")
GARAGE_BUCKET = os.environ.get("GARAGE_BUCKET", "test-bucket")
GARAGE_ACCESS_KEY = os.environ["GARAGE_ACCESS_KEY_ID"]
GARAGE_SECRET_KEY = os.environ["GARAGE_SECRET_ACCESS_KEY"]

# ── MinIO backend credentials (optional) ──────────────────────────────────────
MINIO_HOST = os.environ.get("MINIO_HOST", "")
MINIO_PORT = int(os.environ.get("MINIO_PORT", "9000"))
MINIO_REGION = os.environ.get("MINIO_REGION", "us-east-1")
MINIO_BUCKET = os.environ.get("MINIO_BUCKET", "")
MINIO_ACCESS_KEY = os.environ.get("MINIO_ACCESS_KEY_ID", "")
MINIO_SECRET_KEY = os.environ.get("MINIO_SECRET_ACCESS_KEY", "")

# ── Frontend credentials (what OSP clients present) ───────────────────────────
CLIENT_ACCESS_KEY = os.environ.get("OSP_CLIENT_ACCESS_KEY", "osp-client")
CLIENT_SECRET_KEY = os.environ.get("OSP_CLIENT_SECRET_KEY", "osp-client-secret")

OSP_PROXY_PORT = int(os.environ.get("OSP_PROXY_PORT", "6190"))
OSP_METRICS_PORT = int(os.environ.get("OSP_METRICS_PORT", "9091"))


# ── Callbacks ─────────────────────────────────────────────────────────────────


def lookup_secret(access_key: str) -> str | None:
    """Return the secret key for a given client access key."""
    if access_key == CLIENT_ACCESS_KEY:
        return CLIENT_SECRET_KEY
    return None


def authorize(token: str, bucket: str, request: dict) -> bool:
    """Allow any request coming from a recognised client key."""
    return token == CLIENT_ACCESS_KEY


# ── cos_map ───────────────────────────────────────────────────────────────────

cos_map = {
    GARAGE_BUCKET: {
        "host": GARAGE_HOST,
        "port": GARAGE_PORT,
        "region": GARAGE_REGION,
        "access_key": GARAGE_ACCESS_KEY,
        "secret_key": GARAGE_SECRET_KEY,
        "addressing_style": "path",  # Garage works best with path style
        "is_tls_enabled": False,
    },
}

# Add MinIO bucket if credentials are available
if MINIO_HOST and MINIO_BUCKET and MINIO_ACCESS_KEY:
    cos_map[MINIO_BUCKET] = {
        "host": MINIO_HOST,
        "port": MINIO_PORT,
        "region": MINIO_REGION,
        "access_key": MINIO_ACCESS_KEY,
        "secret_key": MINIO_SECRET_KEY,
        "addressing_style": "path",
        "is_tls_enabled": False,
    }
    print(
        f"[osp] minio      -> http://{MINIO_HOST}:{MINIO_PORT}  bucket={MINIO_BUCKET}"
    )

# ── Server ────────────────────────────────────────────────────────────────────

config = ProxyServerConfig(
    cos_map=cos_map,
    hmac_fetcher=lookup_secret,
    validator=authorize,
    http_port=OSP_PROXY_PORT,
    metrics_port=OSP_METRICS_PORT,
    skip_signature_validation=False,
    threads=2,
)

print(f"[osp] proxy      -> http://0.0.0.0:{OSP_PROXY_PORT}")
print(f"[osp] metrics    -> http://0.0.0.0:{OSP_METRICS_PORT}/metrics")
print(f"[osp] backend    -> http://{GARAGE_HOST}:{GARAGE_PORT}  bucket={GARAGE_BUCKET}")
print(f"[osp] client key -> {CLIENT_ACCESS_KEY}")

start_server(config)
