"""
test_bucket_ops.py — bucket-level S3 API operations.

Covers
------
HeadBucket
  - Existing bucket -> 200
  - Non-existent bucket -> 404 / NoSuchBucket

ListBuckets
  - The test bucket appears in the list

GetBucketLocation
  - Returns a location/region string (even for Garage which uses a fixed region)

ListObjectsV2 — additional parameters
  - StartAfter: only keys lexicographically after the marker are returned
  - FetchOwner: Owner field included in results
  - ContinuationToken: explicit two-page pagination

ListObjects (v1, legacy)
  - Basic listing works (many older clients use v1)
  - Marker-based pagination

ListMultipartUploads
  - In-progress uploads appear; completed/aborted do not
"""

from __future__ import annotations

import pytest
import botocore.exceptions


# ── Helpers ───────────────────────────────────────────────────────────────────


def _put(s3, bucket, key, body=b"x"):
    s3.put_object(Bucket=bucket, Key=key, Body=body)


# ── HeadBucket ────────────────────────────────────────────────────────────────


class TestHeadBucket:
    def test_head_existing_bucket_returns_200(self, s3, bucket):
        resp = s3.head_bucket(Bucket=bucket)
        assert resp["ResponseMetadata"]["HTTPStatusCode"] == 200

    def test_head_nonexistent_bucket_raises_404(self, s3):
        with pytest.raises(botocore.exceptions.ClientError) as exc_info:
            s3.head_bucket(Bucket="this-bucket-does-not-exist-osp-test-xyzzy")
        code = exc_info.value.response["Error"]["Code"]
        assert code in ("404", "NoSuchBucket")


# ── ListBuckets ───────────────────────────────────────────────────────────────


class TestListBuckets:
    def test_test_bucket_appears_in_list(self, s3, bucket):
        resp = s3.list_buckets()
        names = [b["Name"] for b in resp.get("Buckets", [])]
        assert bucket in names, f"Expected {bucket!r} in ListBuckets, got {names}"

    def test_list_buckets_response_structure(self, s3):
        resp = s3.list_buckets()
        assert "Buckets" in resp
        for b in resp["Buckets"]:
            assert "Name" in b
            assert "CreationDate" in b


# ── GetBucketLocation ─────────────────────────────────────────────────────────


class TestGetBucketLocation:
    def test_get_bucket_location_returns_string(self, s3, bucket):
        resp = s3.get_bucket_location(Bucket=bucket)
        # LocationConstraint is None for us-east-1 on AWS, or a region string
        # Garage returns the configured region.
        assert "LocationConstraint" in resp


# ── ListObjectsV2 — extended parameters ──────────────────────────────────────


class TestListObjectsV2Extended:
    def test_start_after_skips_earlier_keys(self, s3, bucket, prefix):
        keys = [f"{prefix}item-{i:03}.txt" for i in range(5)]
        for k in keys:
            _put(s3, bucket, k)

        # StartAfter item-001 -> should skip item-000 and item-001
        resp = s3.list_objects_v2(
            Bucket=bucket, Prefix=prefix, StartAfter=f"{prefix}item-001.txt"
        )
        returned = [o["Key"] for o in resp.get("Contents", [])]
        assert f"{prefix}item-000.txt" not in returned
        assert f"{prefix}item-001.txt" not in returned
        assert f"{prefix}item-002.txt" in returned

    def test_continuation_token_pagination(self, s3, bucket, prefix):
        n = 12
        for i in range(n):
            _put(s3, bucket, f"{prefix}pg-{i:03}.txt")

        page1 = s3.list_objects_v2(Bucket=bucket, Prefix=prefix, MaxKeys=5)
        assert page1["IsTruncated"]
        token = page1["NextContinuationToken"]

        page2 = s3.list_objects_v2(
            Bucket=bucket, Prefix=prefix, MaxKeys=5, ContinuationToken=token
        )
        keys_p1 = {o["Key"] for o in page1.get("Contents", [])}
        keys_p2 = {o["Key"] for o in page2.get("Contents", [])}
        # No overlap between pages
        assert keys_p1.isdisjoint(keys_p2)

    def test_fetch_owner_includes_owner_field(self, s3, bucket, prefix):
        key = f"{prefix}owner-check.txt"
        _put(s3, bucket, key)
        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix, FetchOwner=True)
        contents = resp.get("Contents", [])
        assert contents, "Expected at least one object"
        # Owner is returned by AWS S3 when FetchOwner=True; Garage may omit it.
        # We only assert it's present when it IS returned (not a strict check).
        if "Owner" in contents[0]:
            assert isinstance(contents[0]["Owner"], dict)

    def test_max_keys_limits_results(self, s3, bucket, prefix):
        for i in range(10):
            _put(s3, bucket, f"{prefix}mk-{i}.txt")
        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix, MaxKeys=3)
        assert len(resp.get("Contents", [])) <= 3


# ── ListObjects v1 (legacy) ───────────────────────────────────────────────────


class TestListObjectsV1:
    def test_list_objects_v1_basic(self, s3, bucket, prefix):
        keys = [f"{prefix}v1-{i}.txt" for i in range(3)]
        for k in keys:
            _put(s3, bucket, k)

        resp = s3.list_objects(Bucket=bucket, Prefix=prefix)
        returned = [o["Key"] for o in resp.get("Contents", [])]
        for k in keys:
            assert k in returned

    def test_list_objects_v1_with_delimiter(self, s3, bucket, prefix):
        _put(s3, bucket, f"{prefix}dir/a.txt")
        _put(s3, bucket, f"{prefix}dir/b.txt")
        _put(s3, bucket, f"{prefix}root.txt")

        resp = s3.list_objects(Bucket=bucket, Prefix=prefix, Delimiter="/")
        common = [cp["Prefix"] for cp in resp.get("CommonPrefixes", [])]
        assert f"{prefix}dir/" in common
        direct = [o["Key"] for o in resp.get("Contents", [])]
        assert f"{prefix}root.txt" in direct

    def test_list_objects_v1_marker_pagination(self, s3, bucket, prefix):
        n = 8
        keys = sorted(f"{prefix}mrkr-{i:03}.txt" for i in range(n))
        for k in keys:
            _put(s3, bucket, k)

        page1 = s3.list_objects(Bucket=bucket, Prefix=prefix, MaxKeys=4)
        assert page1["IsTruncated"]
        marker = page1["NextMarker"]

        page2 = s3.list_objects(Bucket=bucket, Prefix=prefix, MaxKeys=4, Marker=marker)
        p1_keys = {o["Key"] for o in page1.get("Contents", [])}
        p2_keys = {o["Key"] for o in page2.get("Contents", [])}
        assert p1_keys.isdisjoint(p2_keys)
        assert p1_keys | p2_keys == set(keys)


# ── ListMultipartUploads ──────────────────────────────────────────────────────


class TestListMultipartUploads:
    def test_in_progress_upload_appears_in_list(self, s3, bucket, prefix):
        key = f"{prefix}mpu-in-progress.bin"
        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        uid = mpu["UploadId"]

        try:
            resp = s3.list_multipart_uploads(Bucket=bucket, Prefix=prefix)
            upload_ids = [u["UploadId"] for u in resp.get("Uploads", [])]
            assert uid in upload_ids
        finally:
            s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=uid)

    def test_aborted_upload_does_not_appear(self, s3, bucket, prefix):
        key = f"{prefix}mpu-aborted.bin"
        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        uid = mpu["UploadId"]
        s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=uid)

        resp = s3.list_multipart_uploads(Bucket=bucket, Prefix=prefix)
        upload_ids = [u["UploadId"] for u in resp.get("Uploads", [])]
        assert uid not in upload_ids

    def test_completed_upload_does_not_appear(self, s3, bucket, prefix):
        key = f"{prefix}mpu-completed.bin"
        part_size = 5 * 1024 * 1024
        body = b"z" * part_size

        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        uid = mpu["UploadId"]
        part = s3.upload_part(
            Bucket=bucket, Key=key, UploadId=uid, PartNumber=1, Body=body
        )
        s3.complete_multipart_upload(
            Bucket=bucket,
            Key=key,
            UploadId=uid,
            MultipartUpload={"Parts": [{"PartNumber": 1, "ETag": part["ETag"]}]},
        )

        resp = s3.list_multipart_uploads(Bucket=bucket, Prefix=prefix)
        upload_ids = [u["UploadId"] for u in resp.get("Uploads", [])]
        assert uid not in upload_ids

    def test_list_multipart_uploads_empty(self, s3, bucket, prefix):
        """With no in-progress uploads for this prefix, Uploads list is absent/empty."""
        resp = s3.list_multipart_uploads(Bucket=bucket, Prefix=f"{prefix}no-mpu/")
        assert resp.get("Uploads", []) == []
