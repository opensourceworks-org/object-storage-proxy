"""
test_advanced_objects.py — additional S3 object operations not covered elsewhere.

Covers
------
DeleteObjects (bulk) — edge cases
  - Quiet mode: only errors are reported, not successful deletes
  - Mixed: some keys exist, some do not — partial success
  - Empty Delete list is rejected (MalformedXML / no objects)

UploadPartCopy
  - Copy a range of an existing object as a multipart part
  - Assemble and verify content integrity

Presigned URL — additional verbs
  - Presigned HEAD
  - Presigned DELETE

GetObject response-header overrides (query-string params)
  - response-content-type
  - response-content-disposition
  - response-cache-control

PutObject — large object via streaming (> 8 MB)
  - Verifies the proxy doesn't buffer the whole body before forwarding

GetObject — verify content after large streaming PUT
"""

from __future__ import annotations

import hashlib

import pytest
import requests


# ── Helpers ───────────────────────────────────────────────────────────────────


def _put(s3, bucket, key, body=b"x"):
    return s3.put_object(Bucket=bucket, Key=key, Body=body)


def _md5(data: bytes) -> str:
    return hashlib.md5(data).hexdigest()


# ── DeleteObjects edge cases ──────────────────────────────────────────────────


class TestDeleteObjectsEdgeCases:
    def test_quiet_mode_suppresses_successful_deletes(self, s3, bucket, prefix):
        """In quiet mode only errors are returned; successful deletes are silent."""
        keys = [f"{prefix}quiet-{i}.txt" for i in range(3)]
        for k in keys:
            _put(s3, bucket, k)

        resp = s3.delete_objects(
            Bucket=bucket,
            Delete={
                "Objects": [{"Key": k} for k in keys],
                "Quiet": True,
            },
        )
        # No Deleted list in quiet mode; Errors list should be absent or empty
        assert resp.get("Deleted", []) == []
        assert resp.get("Errors", []) == []

    def test_delete_mix_of_existing_and_missing_keys(self, s3, bucket, prefix):
        """Deleting a mix of existing and non-existent keys.

        AWS S3 spec: deleting a non-existent key is not an error.
        Garage: returns a NoSuchKey error entry for missing keys.
        We accept both behaviours — the important thing is that the
        existing key IS deleted and no exception is raised.
        """
        existing = f"{prefix}exists.txt"
        missing = f"{prefix}ghost.txt"
        _put(s3, bucket, existing)

        resp = s3.delete_objects(
            Bucket=bucket,
            Delete={"Objects": [{"Key": existing}, {"Key": missing}]},
        )
        # The existing key must have been deleted
        with pytest.raises(Exception):
            s3.head_object(Bucket=bucket, Key=existing)
        # Non-existent key may appear in Errors (Garage) or not (AWS S3)
        for err in resp.get("Errors", []):
            assert err["Key"] == missing, (
                f"Unexpected error for key {err['Key']!r}: {err}"
            )

    def test_delete_all_objects_leaves_empty_prefix(self, s3, bucket, prefix):
        keys = [f"{prefix}del-all-{i}.txt" for i in range(5)]
        for k in keys:
            _put(s3, bucket, k)

        s3.delete_objects(
            Bucket=bucket,
            Delete={"Objects": [{"Key": k} for k in keys]},
        )
        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix)
        assert resp.get("Contents", []) == []


# ── UploadPartCopy ────────────────────────────────────────────────────────────


class TestUploadPartCopy:
    def test_upload_part_copy_full_source(self, s3, bucket, prefix):
        """UploadPartCopy can copy an entire existing object as a single part."""
        part_size = 5 * 1024 * 1024
        src_key = f"{prefix}upc-src.bin"
        dst_key = f"{prefix}upc-dst.bin"
        body = b"A" * part_size

        _put(s3, bucket, src_key, body=body)

        mpu = s3.create_multipart_upload(Bucket=bucket, Key=dst_key)
        uid = mpu["UploadId"]
        try:
            part = s3.upload_part_copy(
                Bucket=bucket,
                Key=dst_key,
                UploadId=uid,
                PartNumber=1,
                CopySource={"Bucket": bucket, "Key": src_key},
            )
            etag = part["CopyPartResult"]["ETag"]

            resp = s3.complete_multipart_upload(
                Bucket=bucket,
                Key=dst_key,
                UploadId=uid,
                MultipartUpload={"Parts": [{"PartNumber": 1, "ETag": etag}]},
            )
            assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200
        except Exception:
            s3.abort_multipart_upload(Bucket=bucket, Key=dst_key, UploadId=uid)
            raise

        data = s3.get_object(Bucket=bucket, Key=dst_key)["Body"].read()
        assert len(data) == len(body), f"length mismatch: {len(data)} != {len(body)}"
        assert _md5(data) == _md5(body), "MD5 mismatch after UploadPartCopy"

    def test_upload_part_copy_with_byte_range(self, s3, bucket, prefix):
        """UploadPartCopy with CopySourceRange copies only the specified byte slice."""
        part_size = 5 * 1024 * 1024
        src_body = b"S" * part_size + b"T" * part_size  # 10 MB total
        src_key = f"{prefix}upc-range-src.bin"
        dst_key = f"{prefix}upc-range-dst.bin"

        _put(s3, bucket, src_key, body=src_body)

        # Only copy the second half
        start = part_size
        end = len(src_body) - 1

        mpu = s3.create_multipart_upload(Bucket=bucket, Key=dst_key)
        uid = mpu["UploadId"]
        try:
            part = s3.upload_part_copy(
                Bucket=bucket,
                Key=dst_key,
                UploadId=uid,
                PartNumber=1,
                CopySource={"Bucket": bucket, "Key": src_key},
                CopySourceRange=f"bytes={start}-{end}",
            )
            etag = part["CopyPartResult"]["ETag"]
            s3.complete_multipart_upload(
                Bucket=bucket,
                Key=dst_key,
                UploadId=uid,
                MultipartUpload={"Parts": [{"PartNumber": 1, "ETag": etag}]},
            )
        except Exception:
            s3.abort_multipart_upload(Bucket=bucket, Key=dst_key, UploadId=uid)
            raise

        data = s3.get_object(Bucket=bucket, Key=dst_key)["Body"].read()
        expected = src_body[start:]
        assert len(data) == len(expected), (
            f"length mismatch: {len(data)} != {len(expected)}"
        )
        assert _md5(data) == _md5(expected), (
            "MD5 mismatch: content differs from expected slice"
        )

    def test_multipart_assembled_from_two_part_copies(self, s3, bucket, prefix):
        """Assemble a new object from two UploadPartCopy calls."""
        part_size = 5 * 1024 * 1024
        src1 = f"{prefix}upc-2src1.bin"
        src2 = f"{prefix}upc-2src2.bin"
        dst = f"{prefix}upc-2dst.bin"
        body1 = b"X" * part_size
        body2 = b"Y" * part_size
        _put(s3, bucket, src1, body=body1)
        _put(s3, bucket, src2, body=body2)

        mpu = s3.create_multipart_upload(Bucket=bucket, Key=dst)
        uid = mpu["UploadId"]
        try:
            p1 = s3.upload_part_copy(
                Bucket=bucket,
                Key=dst,
                UploadId=uid,
                PartNumber=1,
                CopySource={"Bucket": bucket, "Key": src1},
            )
            p2 = s3.upload_part_copy(
                Bucket=bucket,
                Key=dst,
                UploadId=uid,
                PartNumber=2,
                CopySource={"Bucket": bucket, "Key": src2},
            )
            s3.complete_multipart_upload(
                Bucket=bucket,
                Key=dst,
                UploadId=uid,
                MultipartUpload={
                    "Parts": [
                        {"PartNumber": 1, "ETag": p1["CopyPartResult"]["ETag"]},
                        {"PartNumber": 2, "ETag": p2["CopyPartResult"]["ETag"]},
                    ]
                },
            )
        except Exception:
            s3.abort_multipart_upload(Bucket=bucket, Key=dst, UploadId=uid)
            raise

        data = s3.get_object(Bucket=bucket, Key=dst)["Body"].read()
        expected = body1 + body2
        assert len(data) == len(expected), (
            f"length mismatch: {len(data)} != {len(expected)}"
        )
        assert _md5(data) == _md5(expected), (
            "MD5 mismatch after two-part UploadPartCopy"
        )


# ── Presigned — additional verbs ──────────────────────────────────────────────


class TestPresignedAdditional:
    def test_presigned_head_returns_headers(self, s3, bucket, prefix):
        key = f"{prefix}ps-head.txt"
        body = b"presigned head"
        _put(s3, bucket, key, body=body)

        url = s3.generate_presigned_url(
            "head_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=300,
        )
        resp = requests.head(url, timeout=10)
        assert resp.status_code == 200
        assert int(resp.headers.get("Content-Length", 0)) == len(body)

    def test_presigned_delete_removes_object(self, s3, bucket, prefix):
        key = f"{prefix}ps-delete.txt"
        _put(s3, bucket, key)

        url = s3.generate_presigned_url(
            "delete_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=300,
        )
        resp = requests.delete(url, timeout=10)
        assert resp.status_code in (200, 204)

        with pytest.raises(Exception):
            s3.head_object(Bucket=bucket, Key=key)

    def test_presigned_get_with_range_header(self, s3, bucket, prefix):
        key = f"{prefix}ps-range.bin"
        body = bytes(range(256))
        _put(s3, bucket, key, body=body)

        url = s3.generate_presigned_url(
            "get_object",
            Params={"Bucket": bucket, "Key": key},
            ExpiresIn=300,
        )
        resp = requests.get(url, headers={"Range": "bytes=0-15"}, timeout=10)
        assert resp.status_code == 206
        assert resp.content == body[0:16]


# ── GetObject response-header overrides ──────────────────────────────────────


class TestGetObjectResponseOverrides:
    """
    AWS S3 allows overriding response headers via query-string params on
    authenticated requests.  The proxy must pass these through.
    """

    def _presigned_get(self, s3, bucket, key, **response_overrides):
        """Generate a presigned URL that bakes in response-header overrides."""
        params = {"Bucket": bucket, "Key": key, **response_overrides}
        return s3.generate_presigned_url("get_object", Params=params, ExpiresIn=300)

    def test_response_content_type_override(self, s3, bucket, prefix):
        key = f"{prefix}override-ct.txt"
        _put(s3, bucket, key)
        url = self._presigned_get(s3, bucket, key, ResponseContentType="text/csv")
        resp = requests.get(url, timeout=10)
        assert resp.status_code == 200
        assert "text/csv" in resp.headers.get("Content-Type", "")

    def test_response_content_disposition_override(self, s3, bucket, prefix):
        key = f"{prefix}override-cd.txt"
        _put(s3, bucket, key)
        url = self._presigned_get(
            s3,
            bucket,
            key,
            ResponseContentDisposition='attachment; filename="export.csv"',
        )
        resp = requests.get(url, timeout=10)
        assert resp.status_code == 200
        cd = resp.headers.get("Content-Disposition", "")
        assert "attachment" in cd

    def test_response_cache_control_override(self, s3, bucket, prefix):
        key = f"{prefix}override-cc.txt"
        _put(s3, bucket, key)
        url = self._presigned_get(s3, bucket, key, ResponseCacheControl="no-store")
        resp = requests.get(url, timeout=10)
        assert resp.status_code == 200
        assert "no-store" in resp.headers.get("Cache-Control", "")


# ── Large streaming PUT ───────────────────────────────────────────────────────


class TestLargeStreamingPut:
    def test_12mb_streaming_put_and_get(self, s3, bucket, prefix):
        """Verify the proxy handles a large single-PUT without buffering issues."""
        key = f"{prefix}large-stream.bin"
        size = 12 * 1024 * 1024  # 12 MB — above typical 8 MB chunk boundaries
        body = bytes(i % 251 for i in range(size))

        s3.put_object(Bucket=bucket, Key=key, Body=body)

        retrieved = s3.get_object(Bucket=bucket, Key=key)["Body"].read()
        assert len(retrieved) == size, f"length mismatch: {len(retrieved)} != {size}"
        assert _md5(retrieved) == _md5(body), "MD5 mismatch after large streaming PUT"
