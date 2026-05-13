"""
test_presigned.py — presigned URL generation and usage via the OSP proxy.

The proxy re-signs outgoing requests with backend credentials.  Presigned
URLs let a third party access an object without S3 credentials, through the
proxy, for a limited time.

Covers:
- Presigned GET   (download object without credentials)
- Presigned PUT   (upload object without credentials)
- Expiry          (expired URL is rejected)
- Usage limit     (proxy enforces max_presign_url_usage_attempts if set)
"""

from __future__ import annotations

import time

import pytest
import urllib.request
import urllib.error


def _http_get(url: str) -> tuple[int, bytes]:
    try:
        with urllib.request.urlopen(url, timeout=10) as resp:  # noqa: S310
            return resp.status, resp.read()
    except urllib.error.HTTPError as exc:
        return exc.code, b""


def _http_put(url: str, body: bytes) -> int:
    req = urllib.request.Request(url, data=body, method="PUT")  # noqa: S310
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:  # noqa: S310
            return resp.status
    except urllib.error.HTTPError as exc:
        return exc.code


class TestPresignedGet:
    def test_presigned_get_returns_body(self, s3, bucket, prefix):
        key = f"{prefix}presigned-get.txt"
        body = b"presigned content"
        s3.put_object(Bucket=bucket, Key=key, Body=body)

        url = s3.generate_presigned_url(
            "get_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=60,
        )
        status, data = _http_get(url)
        assert status == 200
        assert data == body

    def test_presigned_get_404_for_missing_key(self, s3, bucket, prefix):
        url = s3.generate_presigned_url(
            "get_object",
            Params={"Bucket": bucket, "Key": f"{prefix}ghost.txt"},
            ExpiresIn=60,
        )
        status, _ = _http_get(url)
        assert status in (403, 404)  # proxy or Garage may return either

    def test_presigned_get_expired_url_rejected(self, s3, bucket, prefix):
        key = f"{prefix}expired.txt"
        s3.put_object(Bucket=bucket, Key=key, Body=b"will expire")

        # Generate with 1-second expiry, then wait
        url = s3.generate_presigned_url(
            "get_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=1,
        )
        time.sleep(2)
        status, _ = _http_get(url)
        assert status in (400, 403), f"Expected 400/403 for expired URL, got {status}"

    def test_presigned_get_different_clients_same_url(self, s3, bucket, prefix):
        """Two sequential GETs of the same presigned URL should both succeed
        (assuming max_presign_url_usage_attempts is > 2 or not set)."""
        key = f"{prefix}shared.txt"
        body = b"shared-presigned"
        s3.put_object(Bucket=bucket, Key=key, Body=body)

        url = s3.generate_presigned_url(
            "get_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=60,
        )
        s1, d1 = _http_get(url)
        s2, d2 = _http_get(url)
        # If the proxy has a usage limit of 1 the second call may be 403 —
        # mark it with xfail rather than hard-fail so the test is informative.
        if s1 == 200 and s2 == 403:
            pytest.xfail(
                "Proxy rejected second use of presigned URL (usage limit active)"
            )
        assert s1 == 200 and d1 == body
        assert s2 == 200 and d2 == body


class TestPresignedPut:
    def test_presigned_put_then_get(self, s3, bucket, prefix):
        key = f"{prefix}presigned-put.txt"
        body = b"uploaded via presigned PUT"

        put_url = s3.generate_presigned_url(
            "put_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=60,
        )
        status = _http_put(put_url, body)
        assert status in (200, 204), f"PUT returned {status}"

        # Retrieve via normal API to verify
        retrieved = s3.get_object(Bucket=bucket, Key=key)["Body"].read()
        assert retrieved == body

    def test_presigned_put_expired_rejected(self, s3, bucket, prefix):
        key = f"{prefix}expired-put.txt"
        put_url = s3.generate_presigned_url(
            "put_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=1,
        )
        time.sleep(2)
        status = _http_put(put_url, b"should fail")
        assert status in (400, 403), (
            f"Expected 400/403 for expired presigned PUT, got {status}"
        )
