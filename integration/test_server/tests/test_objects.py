"""
test_objects.py — basic S3 object operations via the OSP proxy.

Covers: PutObject, GetObject, HeadObject, CopyObject, DeleteObject,
        DeleteObjects (bulk), ListObjectsV2 (plain, prefix, delimiter).
"""

from __future__ import annotations

import hashlib

import pytest


# ── Helpers ───────────────────────────────────────────────────────────────────


def _put(s3, bucket: str, key: str, body: bytes) -> None:
    s3.put_object(Bucket=bucket, Key=key, Body=body)


def _get_body(s3, bucket: str, key: str) -> bytes:
    return s3.get_object(Bucket=bucket, Key=key)["Body"].read()


def _keys_in(s3, bucket: str, **list_kwargs) -> list[str]:
    resp = s3.list_objects_v2(Bucket=bucket, **list_kwargs)
    return [o["Key"] for o in resp.get("Contents", [])]


# ── PutObject / GetObject ─────────────────────────────────────────────────────


class TestPutGet:
    def test_small_text_object(self, s3, bucket, prefix):
        key = f"{prefix}hello.txt"
        body = b"hello, OSP!"
        _put(s3, bucket, key, body)
        assert _get_body(s3, bucket, key) == body

    def test_binary_object(self, s3, bucket, prefix):
        key = f"{prefix}data.bin"
        body = bytes(range(256)) * 64  # 16 KB
        _put(s3, bucket, key, body)
        retrieved = _get_body(s3, bucket, key)
        assert retrieved == body
        assert hashlib.md5(retrieved).hexdigest() == hashlib.md5(body).hexdigest()

    def test_empty_object(self, s3, bucket, prefix):
        key = f"{prefix}empty"
        _put(s3, bucket, key, b"")
        assert _get_body(s3, bucket, key) == b""

    def test_overwrite_object(self, s3, bucket, prefix):
        key = f"{prefix}overwrite.txt"
        _put(s3, bucket, key, b"v1")
        _put(s3, bucket, key, b"v2")
        assert _get_body(s3, bucket, key) == b"v2"

    def test_object_with_special_chars_in_key(self, s3, bucket, prefix):
        key = f"{prefix}path/to/my file (1).txt"
        body = b"special key"
        _put(s3, bucket, key, body)
        assert _get_body(s3, bucket, key) == body

    def test_large_object_1mb(self, s3, bucket, prefix):
        key = f"{prefix}large.bin"
        body = b"x" * (1024 * 1024)
        _put(s3, bucket, key, body)
        assert len(_get_body(s3, bucket, key)) == len(body)

    def test_get_nonexistent_raises(self, s3, bucket, prefix):
        with pytest.raises(s3.exceptions.NoSuchKey):
            s3.get_object(Bucket=bucket, Key=f"{prefix}does-not-exist")


# ── HeadObject ────────────────────────────────────────────────────────────────


class TestHeadObject:
    def test_head_returns_correct_size(self, s3, bucket, prefix):
        body = b"hello head"
        key = f"{prefix}head.txt"
        _put(s3, bucket, key, body)
        resp = s3.head_object(Bucket=bucket, Key=key)
        assert resp["ContentLength"] == len(body)

    def test_head_nonexistent_raises(self, s3, bucket, prefix):
        with pytest.raises(Exception) as exc_info:
            s3.head_object(Bucket=bucket, Key=f"{prefix}ghost")
        assert exc_info.value.response["Error"]["Code"] in ("404", "NoSuchKey")


# ── CopyObject ────────────────────────────────────────────────────────────────


class TestCopyObject:
    def test_copy_within_bucket(self, s3, bucket, prefix):
        src_key = f"{prefix}original.txt"
        dst_key = f"{prefix}copy.txt"
        body = b"copy me"
        _put(s3, bucket, src_key, body)
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src_key},
            Key=dst_key,
        )
        assert _get_body(s3, bucket, dst_key) == body

    def test_copy_overwrites_destination(self, s3, bucket, prefix):
        src = f"{prefix}src.txt"
        dst = f"{prefix}dst.txt"
        _put(s3, bucket, src, b"new-content")
        _put(s3, bucket, dst, b"old-content")
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
        )
        assert _get_body(s3, bucket, dst) == b"new-content"


# ── DeleteObject ──────────────────────────────────────────────────────────────


class TestDeleteObject:
    def test_delete_existing(self, s3, bucket, prefix):
        key = f"{prefix}delete-me.txt"
        _put(s3, bucket, key, b"bye")
        s3.delete_object(Bucket=bucket, Key=key)
        with pytest.raises(Exception):
            s3.head_object(Bucket=bucket, Key=key)

    def test_delete_nonexistent_is_noop(self, s3, bucket, prefix):
        # S3 spec: deleting a non-existent key is not an error
        s3.delete_object(Bucket=bucket, Key=f"{prefix}ghost")

    def test_delete_objects_bulk(self, s3, bucket, prefix):
        keys = [f"{prefix}bulk-{i}.txt" for i in range(5)]
        for k in keys:
            _put(s3, bucket, k, b"x")
        s3.delete_objects(
            Bucket=bucket,
            Delete={"Objects": [{"Key": k} for k in keys]},
        )
        remaining = _keys_in(s3, bucket, Prefix=prefix)
        assert not any(k in remaining for k in keys)


# ── ListObjectsV2 ─────────────────────────────────────────────────────────────


class TestListObjects:
    def test_list_own_prefix(self, s3, bucket, prefix):
        keys = [f"{prefix}a.txt", f"{prefix}b.txt", f"{prefix}c.txt"]
        for k in keys:
            _put(s3, bucket, k, b"x")
        listed = _keys_in(s3, bucket, Prefix=prefix)
        for k in keys:
            assert k in listed

    def test_list_with_prefix_filter(self, s3, bucket, prefix):
        _put(s3, bucket, f"{prefix}match-1.txt", b"a")
        _put(s3, bucket, f"{prefix}match-2.txt", b"b")
        _put(s3, bucket, f"{prefix}other.txt", b"c")
        listed = _keys_in(s3, bucket, Prefix=f"{prefix}match-")
        assert len(listed) == 2
        assert all("match-" in k for k in listed)

    def test_list_with_delimiter(self, s3, bucket, prefix):
        """Delimiter should collapse sub-paths into common prefixes."""
        _put(s3, bucket, f"{prefix}dir1/a.txt", b"a")
        _put(s3, bucket, f"{prefix}dir1/b.txt", b"b")
        _put(s3, bucket, f"{prefix}dir2/c.txt", b"c")
        _put(s3, bucket, f"{prefix}root.txt", b"r")

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix, Delimiter="/")
        common = [cp["Prefix"] for cp in resp.get("CommonPrefixes", [])]
        assert f"{prefix}dir1/" in common
        assert f"{prefix}dir2/" in common
        # root.txt is a direct key, not a prefix
        direct_keys = [o["Key"] for o in resp.get("Contents", [])]
        assert f"{prefix}root.txt" in direct_keys

    def test_list_pagination(self, s3, bucket, prefix):
        """Create more objects than MaxKeys and verify full retrieval via pagination."""
        n = 15
        for i in range(n):
            _put(s3, bucket, f"{prefix}page-{i:03}.txt", b"p")

        collected: list[str] = []
        kwargs: dict = {"Bucket": bucket, "Prefix": prefix, "MaxKeys": 5}
        while True:
            resp = s3.list_objects_v2(**kwargs)
            collected.extend(o["Key"] for o in resp.get("Contents", []))
            if not resp.get("IsTruncated"):
                break
            kwargs["ContinuationToken"] = resp["NextContinuationToken"]

        assert len(collected) >= n

    def test_list_returns_content_length(self, s3, bucket, prefix):
        body = b"size-check"
        key = f"{prefix}sized.txt"
        _put(s3, bucket, key, body)
        resp = s3.list_objects_v2(Bucket=bucket, Prefix=prefix)
        sizes = {o["Key"]: o["Size"] for o in resp.get("Contents", [])}
        assert sizes.get(key) == len(body)
