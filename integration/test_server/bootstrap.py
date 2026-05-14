"""
bootstrap.py — idempotent Garage cluster setup.

Run once (or any time) after `task integration:garage:up`.  It:

1. Waits for the Garage admin API to be ready.
2. Reads the node ID from /v1/status and assigns it a layout (zone + capacity).
3. Applies the layout.
4. Creates (or reuses) the test bucket.
5. Creates (or reuses) an HMAC access key for the OSP backend.
6. Grants the key read/write/owner access on the bucket.
7. Writes a `.env` file next to this script so server.py and the tests can
   pick up the generated credentials.

All steps are idempotent — safe to run multiple times.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

import requests

# ── Configuration ─────────────────────────────────────────────────────────────

ADMIN_BASE = "http://localhost:3903/v1"
ADMIN_TOKEN = "osp-integration-admin-token"  # must match garage.toml
BUCKET_NAME = "test-bucket"
KEY_NAME = "osp-backend-key"

HEADERS = {
    "Authorization": f"Bearer {ADMIN_TOKEN}",
    "Content-Type": "application/json",
}

HERE = Path(__file__).parent


# ── Helpers ───────────────────────────────────────────────────────────────────


def admin(method: str, path: str, **kwargs) -> requests.Response:
    url = f"{ADMIN_BASE}{path}"
    resp = requests.request(method, url, headers=HEADERS, **kwargs)
    if not resp.ok:
        print(
            f"  [!] {method} {path} -> {resp.status_code}: {resp.text}", file=sys.stderr
        )
        resp.raise_for_status()
    return resp


def wait_for_garage(timeout: int = 30) -> None:
    print("⏳ Waiting for Garage admin API …")
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            r = requests.get(f"{ADMIN_BASE}/status", headers=HEADERS, timeout=2)
            if r.ok:
                print("✅ Garage is ready.")
                return
        except requests.ConnectionError:
            pass
        time.sleep(1)
    raise RuntimeError("Garage did not become ready in time.")


# ── Steps ─────────────────────────────────────────────────────────────────────


def get_node_id() -> str:
    status = admin("GET", "/status").json()
    node_id: str = status["node"]
    print(f"   Node ID: {node_id[:12]}…")
    return node_id


def set_layout(node_id: str) -> None:
    """Assign a zone + capacity to the single node and apply.

    Garage v1 API:
      POST /v1/layout  ->  array of NodeRoleChangeEnum with action="configure"
                          capacity is in bytes
      POST /v1/layout/apply  ->  {"version": <next_version>}
    """
    layout = admin("GET", "/layout").json()
    current_version = layout.get("version", 0)

    # Check if this node already has a role
    roles = layout.get("roles", [])
    if any(r["id"].startswith(node_id[:8]) for r in roles):
        print("   Layout already applied, skipping.")
        return

    print("   Staging layout …")
    admin(
        "POST",
        "/layout",
        json=[
            {
                "id": node_id,
                "action": "configure",
                "zone": "dc1",
                "capacity": 1 * 1024 * 1024 * 1024,  # 1 GiB in bytes
                "tags": [],
            }
        ],
    )

    # Re-read to get the version that was actually staged
    layout = admin("GET", "/layout").json()
    new_version = current_version + 1
    print(f"   Applying layout version {new_version} …")
    admin("POST", "/layout/apply", json={"version": new_version})
    print("✅ Layout applied.")


def ensure_bucket() -> str:
    """Create the test bucket (or return its ID if it already exists)."""
    # List existing buckets
    buckets = admin("GET", "/bucket?list").json()
    for b in buckets:
        if b.get("globalAliases") and BUCKET_NAME in b["globalAliases"]:
            bucket_id: str = b["id"]
            print(f"   Bucket '{BUCKET_NAME}' already exists ({bucket_id[:12]}…).")
            return bucket_id

    print(f"   Creating bucket '{BUCKET_NAME}' …")
    resp = admin("POST", "/bucket", json={"globalAlias": BUCKET_NAME})
    bucket_id = resp.json()["id"]
    print(f"✅ Bucket created ({bucket_id[:12]}…).")
    return bucket_id


def ensure_key() -> tuple[str, str]:
    """Create the backend HMAC key (or reuse if a key with this name exists).

    Garage only returns the secret at creation time.  If the key already exists
    we try to recover the secret from the local .env file.  If that also fails
    (e.g. first run on a dirty cluster), we delete and recreate the key.
    """
    keys = admin("GET", "/key?list").json()
    for k in keys:
        if k.get("name") == KEY_NAME:
            access = k["id"]
            print(f"   Key '{KEY_NAME}' already exists ({access[:8]}…).")

            # Try to recover the secret from the existing .env
            env_path = HERE / ".env"
            if env_path.exists():
                for line in env_path.read_text().splitlines():
                    if line.startswith("GARAGE_SECRET_ACCESS_KEY="):
                        secret = line.split("=", 1)[1].strip()
                        print("   Recovered secret from .env.")
                        return access, secret

            # .env missing or doesn't have the secret — delete and recreate
            print("   Secret not recoverable, recreating key …")
            admin("DELETE", f"/key?id={access}")
            break

    print(f"   Creating key '{KEY_NAME}' …")
    detail = admin("POST", "/key", json={"name": KEY_NAME}).json()
    access = detail["accessKeyId"]
    secret = detail["secretAccessKey"]
    print(f"✅ Key created ({access[:8]}…).")
    return access, secret


def grant_key(bucket_id: str, access_key_id: str) -> None:
    """Grant the key full access on the bucket (idempotent)."""
    print("   Granting key access on bucket …")
    admin(
        "POST",
        "/bucket/allow",
        json={
            "bucketId": bucket_id,
            "accessKeyId": access_key_id,
            "permissions": {"read": True, "write": True, "owner": True},
        },
    )
    print("✅ Permissions granted.")


def write_env(access_key: str, secret_key: str) -> None:
    env_path = HERE / ".env"
    content = f"""\
# Auto-generated by bootstrap.py — do not edit manually.

# ── Garage backend ────────────────────────────────────────────────────────────
GARAGE_HOST=localhost
GARAGE_PORT=3900
GARAGE_REGION=garage
GARAGE_BUCKET={BUCKET_NAME}
GARAGE_ACCESS_KEY_ID={access_key}
GARAGE_SECRET_ACCESS_KEY={secret_key}

# ── OSP proxy (frontend client credentials) ───────────────────────────────────
OSP_PROXY_HOST=localhost
OSP_PROXY_PORT=6190
OSP_METRICS_PORT=9091
OSP_CLIENT_ACCESS_KEY=osp-client
OSP_CLIENT_SECRET_KEY=osp-client-secret
"""
    env_path.write_text(content)
    print(f"✅ Written {env_path}")


def print_summary(access_key: str, secret_key: str) -> None:
    print()
    print("─" * 60)
    print("  Integration environment ready")
    print("─" * 60)
    print("  Garage S3 endpoint : http://localhost:3900")
    print(f"  Bucket             : {BUCKET_NAME}")
    print(f"  Backend access key : {access_key}")
    print(f"  Backend secret key : {secret_key[:6]}…")
    print()
    print("  OSP proxy          : http://localhost:6190  (after server start)")
    print("  OSP metrics        : http://localhost:9091/metrics")
    print("  Client access key  : osp-client")
    print("  Client secret key  : osp-client-secret")
    print()
    print("  AWS CLI quick test:")
    print("    AWS_ACCESS_KEY_ID=osp-client \\")
    print("    AWS_SECRET_ACCESS_KEY=osp-client-secret \\")
    print("    AWS_ENDPOINT_URL=http://localhost:6190 \\")
    print("    AWS_DEFAULT_REGION=garage \\")
    print(f"    aws s3 ls s3://{BUCKET_NAME}/")
    print("─" * 60)


# ── Main ──────────────────────────────────────────────────────────────────────


def main() -> None:
    wait_for_garage()

    print("\n[1] Getting node ID …")
    node_id = get_node_id()

    print("\n[2] Setting cluster layout …")
    set_layout(node_id)

    print(f"\n[3] Ensuring bucket '{BUCKET_NAME}' …")
    bucket_id = ensure_bucket()

    print(f"\n[4] Ensuring access key '{KEY_NAME}' …")
    access_key, secret_key = ensure_key()

    print("\n[5] Granting key access on bucket …")
    grant_key(bucket_id, access_key)

    print("\n[6] Writing .env …")
    write_env(access_key, secret_key)

    print_summary(access_key, secret_key)


if __name__ == "__main__":
    main()
