"""
test_range_requests.py — GetObject byte-range and conditional-request tests.

Covers
------
Range requests
  - bytes=0-N   (first N+1 bytes)
  - bytes=N-    (from offset to end)
  - bytes=-N    (last N bytes / suffix range)
  - 206 Partial Content status
  - Content-Range response header
  - Range larger than object -> full object returned (200 or 206)

Conditional requests
  - If-Match:         ETag matches   -> 200
  - If-Match:         ETag mismatch  -> 412 PreconditionFailed
  - If-None-Match:    ETag matches   -> 304 NotModified
  - If-None-Match:    ETag mismatch  -> 200
  - If-Modified-Since:   before mtime -> 200
  - If-Modified-Since:   after mtime  -> 304 NotModified
  - If-Unmodified-Since: after mtime  -> 200
  - If-Unmodified-Since: before mtime -> 412 PreconditionFailed

These are critical for analytics engines (Spark, Trino, Presto) and for
the s3transfer library, which issues If-Match on every ranged part download.
"""

from __future__ import annotations

import hashlib

import botocore.exceptions
import pytest


# ── Fixtures ──────────────────────────────────────────────────────────────────

BODY = bytes(range(256)) * 4  # 1 KB of deterministic bytes


@pytest.fixture
def obj(s3_nochecksum, bucket, prefix):
    """Upload a 1 KB object; return (s3_client, key, body, etag).

    Uses a client with response_checksum_validation=when_required so that
    botocore does not attempt to validate the full-object CRC32 header
    against a partial (ranged) response body.
    """
    key = f"{prefix}range-test.bin"
    resp = s3_nochecksum.put_object(Bucket=bucket, Key=key, Body=BODY)
    etag = resp["ETag"]
    return s3_nochecksum, key, BODY, etag


# ── Range requests ────────────────────────────────────────────────────────────


class TestRangeRequests:
    def test_range_first_100_bytes(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=0-99")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        data = resp["Body"].read()
        assert data == body[0:100]
        assert len(data) == 100

    def test_range_content_range_header(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=0-99")
        content_range = resp.get("ContentRange", "")
        # e.g. "bytes 0-99/1024"
        assert content_range.startswith("bytes 0-99/")

    def test_range_mid_object(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=100-199")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        assert resp["Body"].read() == body[100:200]

    def test_range_open_ended(self, bucket, obj):
        """bytes=N- should return from offset N to end of object."""
        s3, key, body, _ = obj
        offset = 512
        resp = s3.get_object(Bucket=bucket, Key=key, Range=f"bytes={offset}-")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        assert resp["Body"].read() == body[offset:]

    def test_range_suffix(self, bucket, obj):
        """bytes=-N should return the last N bytes."""
        s3, key, body, _ = obj
        n = 64
        resp = s3.get_object(Bucket=bucket, Key=key, Range=f"bytes=-{n}")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        data = resp["Body"].read()
        assert data == body[-n:]
        assert len(data) == n

    def test_range_single_byte(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=0-0")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        assert resp["Body"].read() == body[0:1]

    def test_range_last_byte(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=-1")
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        assert resp["Body"].read() == body[-1:]

    def test_range_entire_object(self, bucket, obj):
        """Range covering the full object should return all bytes."""
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range=f"bytes=0-{len(body) - 1}")
        status = resp["ResponseMetadata"]["HTTPStatusCode"]
        assert status in (200, 206)
        assert resp["Body"].read() == body

    def test_range_out_of_bounds_returns_416(self, bucket, obj):
        """A range entirely beyond EOF must return 416 InvalidRange."""
        s3, key, body, _ = obj
        beyond = len(body) + 1000
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.get_object(Bucket=bucket, Key=key, Range=f"bytes={beyond}-{beyond + 99}")
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("InvalidRange", "416")

    def test_range_content_length_matches_slice(self, bucket, obj):
        """ContentLength in the response must equal the number of bytes returned."""
        s3, key, body, _ = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=200-299")
        assert resp["ContentLength"] == 100
        assert len(resp["Body"].read()) == 100

    def test_range_integrity(self, bucket, obj):
        """Reassemble object from two halves and verify MD5 matches original."""
        s3, key, body, _ = obj
        mid = len(body) // 2
        part1 = s3.get_object(Bucket=bucket, Key=key, Range=f"bytes=0-{mid - 1}")[
            "Body"
        ].read()
        part2 = s3.get_object(Bucket=bucket, Key=key, Range=f"bytes={mid}-")[
            "Body"
        ].read()
        reassembled = part1 + part2
        assert reassembled == body
        assert hashlib.md5(reassembled).hexdigest() == hashlib.md5(body).hexdigest()


# ── Conditional requests ──────────────────────────────────────────────────────


class TestConditionalGet:
    def test_if_match_correct_etag_returns_200(self, bucket, obj):
        s3, key, body, etag = obj
        resp = s3.get_object(Bucket=bucket, Key=key, IfMatch=etag)
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
        assert resp["Body"].read() == body

    @pytest.mark.xfail(
        reason="Garage v1.0.1 does not enforce If-Match; returns 200 instead of 412.",
        strict=True,
    )
    def test_if_match_wrong_etag_raises_412(self, bucket, obj):
        s3, key, _, _ = obj
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.get_object(
                Bucket=bucket, Key=key, IfMatch='"0000000000000000000000000000dead"'
            )
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("PreconditionFailed", "412")

    def test_if_none_match_correct_etag_raises_304(self, bucket, obj):
        s3, key, _, etag = obj
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.get_object(Bucket=bucket, Key=key, IfNoneMatch=etag)
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("304", "NotModified")

    def test_if_none_match_wrong_etag_returns_200(self, bucket, obj):
        s3, key, body, _ = obj
        resp = s3.get_object(
            Bucket=bucket,
            Key=key,
            IfNoneMatch='"0000000000000000000000000000dead"',
        )
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
        assert resp["Body"].read() == body

    def test_if_modified_since_old_date_returns_200(self, bucket, obj):
        """If-Modified-Since set to epoch -> object is newer -> 200."""
        from email.utils import formatdate

        s3, key, body, _ = obj
        old_date = formatdate(0, usegmt=True)  # Thu, 01 Jan 1970 00:00:00 GMT
        resp = s3.get_object(Bucket=bucket, Key=key, IfModifiedSince=old_date)
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
        assert resp["Body"].read() == body

    def test_if_modified_since_future_date_raises_304(self, bucket, obj):
        """If-Modified-Since set to a future date -> object is older -> 304."""
        from datetime import datetime, timezone, timedelta

        s3, key, _, _ = obj
        future = datetime.now(timezone.utc) + timedelta(days=1)
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.get_object(Bucket=bucket, Key=key, IfModifiedSince=future)
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("304", "NotModified")

    def test_if_unmodified_since_future_date_returns_200(self, bucket, obj):
        """If-Unmodified-Since in the future -> condition satisfied -> 200."""
        from datetime import datetime, timezone, timedelta

        s3, key, body, _ = obj
        future = datetime.now(timezone.utc) + timedelta(days=1)
        resp = s3.get_object(Bucket=bucket, Key=key, IfUnmodifiedSince=future)
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
        assert resp["Body"].read() == body

    @pytest.mark.xfail(
        reason="Garage v1.0.1 does not enforce If-Unmodified-Since; returns 200 instead of 412.",
        strict=True,
    )
    def test_if_unmodified_since_old_date_raises_412(self, bucket, obj):
        """If-Unmodified-Since set to epoch -> object is newer -> 412."""
        from email.utils import formatdate

        s3, key, _, _ = obj
        old_date = formatdate(0, usegmt=True)
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.get_object(Bucket=bucket, Key=key, IfUnmodifiedSince=old_date)
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("PreconditionFailed", "412")

    def test_range_with_if_match(self, bucket, obj):
        """s3transfer pattern: range request guarded by If-Match on ETag."""
        s3, key, body, etag = obj
        resp = s3.get_object(Bucket=bucket, Key=key, Range="bytes=0-99", IfMatch=etag)
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 206
        assert resp["Body"].read() == body[0:100]
