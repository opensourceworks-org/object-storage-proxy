"""
server.py — minimal OSP proxy for integration testing.

Reads Garage backend credentials from .env (written by bootstrap.py) and
exposes a single bucket through the proxy on http://localhost:6190.

Run with the *root* project venv (which has object_storage_proxy installed):
    uv run --no-sync python integration/test_server/server.py

Or via Taskfile:
    task integration:server:start
"""

from __future__ import annotations

import os
from pathlib import Path

from dotenv import load_dotenv

# Load .env written by bootstrap.py
load_dotenv(Path(__file__).parent / ".env")

# ── Import OSP from the root venv ─────────────────────────────────────────────
from object_storage_proxy import ProxyServerConfig, start_server  # noqa: E402

# ── Garage backend credentials ────────────────────────────────────────────────
GARAGE_HOST = os.environ.get("GARAGE_HOST", "localhost")
GARAGE_PORT = int(os.environ.get("GARAGE_PORT", "3900"))
GARAGE_REGION = os.environ.get("GARAGE_REGION", "garage")
GARAGE_BUCKET = os.environ.get("GARAGE_BUCKET", "test-bucket")
GARAGE_ACCESS_KEY = os.environ["GARAGE_ACCESS_KEY_ID"]
GARAGE_SECRET_KEY = os.environ["GARAGE_SECRET_ACCESS_KEY"]

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
