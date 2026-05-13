"""
test_multipart.py — multipart upload operations via the OSP proxy.

Covers: CreateMultipartUpload, UploadPart, CompleteMultipartUpload,
        AbortMultipartUpload, ListMultipartUploads, ListParts.

The minimum part size in S3 is 5 MiB for all parts except the last.
"""

from __future__ import annotations


import pytest

PART_SIZE = 5 * 1024 * 1024  # 5 MiB — S3 minimum


def _random_bytes(n: int) -> bytes:
    """Return n deterministic pseudo-random bytes (fast)."""
    return (b"abcdefghijklmnopqrstuvwxyz0123456789" * ((n // 36) + 1))[:n]


class TestMultipartUpload:
    def test_two_part_upload(self, s3, bucket, prefix):
        key = f"{prefix}multipart-2parts.bin"

        # Initiate
        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        upload_id = mpu["UploadId"]

        try:
            parts = []
            for i, body in enumerate(
                [_random_bytes(PART_SIZE), b"final-chunk"], start=1
            ):
                resp = s3.upload_part(
                    Bucket=bucket,
                    Key=key,
                    UploadId=upload_id,
                    PartNumber=i,
                    Body=body,
                )
                parts.append({"ETag": resp["ETag"], "PartNumber": i})

            # Complete
            s3.complete_multipart_upload(
                Bucket=bucket,
                Key=key,
                UploadId=upload_id,
                MultipartUpload={"Parts": parts},
            )
        except Exception:
            s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)
            raise

        # Verify assembled object
        resp = s3.head_object(Bucket=bucket, Key=key)
        assert resp["ContentLength"] == PART_SIZE + len(b"final-chunk")

    def test_three_part_upload_content_integrity(self, s3, bucket, prefix):
        key = f"{prefix}multipart-3parts.bin"
        chunks = [_random_bytes(PART_SIZE), _random_bytes(PART_SIZE), b"tail"]
        expected = b"".join(chunks)

        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        upload_id = mpu["UploadId"]

        try:
            parts = []
            for i, chunk in enumerate(chunks, start=1):
                resp = s3.upload_part(
                    Bucket=bucket,
                    Key=key,
                    UploadId=upload_id,
                    PartNumber=i,
                    Body=chunk,
                )
                parts.append({"ETag": resp["ETag"], "PartNumber": i})

            s3.complete_multipart_upload(
                Bucket=bucket,
                Key=key,
                UploadId=upload_id,
                MultipartUpload={"Parts": parts},
            )
        except Exception:
            s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)
            raise

        actual = s3.get_object(Bucket=bucket, Key=key)["Body"].read()
        assert actual == expected

    def test_abort_multipart_upload(self, s3, bucket, prefix):
        key = f"{prefix}multipart-abort.bin"
        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        upload_id = mpu["UploadId"]

        # Upload one part
        s3.upload_part(
            Bucket=bucket,
            Key=key,
            UploadId=upload_id,
            PartNumber=1,
            Body=_random_bytes(PART_SIZE),
        )

        # Abort — should not raise
        s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)

        # The object should not exist (was never completed)
        with pytest.raises(Exception) as exc_info:
            s3.head_object(Bucket=bucket, Key=key)
        assert exc_info.value.response["Error"]["Code"] in ("404", "NoSuchKey")

    def test_list_parts(self, s3, bucket, prefix):
        key = f"{prefix}multipart-listparts.bin"
        mpu = s3.create_multipart_upload(Bucket=bucket, Key=key)
        upload_id = mpu["UploadId"]

        try:
            etags = []
            for i in range(1, 3):
                resp = s3.upload_part(
                    Bucket=bucket,
                    Key=key,
                    UploadId=upload_id,
                    PartNumber=i,
                    Body=_random_bytes(PART_SIZE),
                )
                etags.append(resp["ETag"])

            resp = s3.list_parts(Bucket=bucket, Key=key, UploadId=upload_id)
            listed_numbers = [p["PartNumber"] for p in resp.get("Parts", [])]
            assert 1 in listed_numbers
            assert 2 in listed_numbers
        finally:
            s3.abort_multipart_upload(Bucket=bucket, Key=key, UploadId=upload_id)
