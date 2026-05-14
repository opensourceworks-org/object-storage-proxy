"""
test_tagging.py — S3 object tagging via PutObjectTagging / GetObjectTagging /
                  DeleteObjectTagging and the inline x-amz-tagging PutObject path.

Covers
------
PutObjectTagging
  - Set tags on an existing object
  - Overwrite existing tags

GetObjectTagging
  - Retrieve tags set via PutObjectTagging
  - Empty tag set after DeleteObjectTagging

DeleteObjectTagging
  - Removes all tags from an object

Inline tagging at upload time (x-amz-tagging / Tagging= kwarg)
  - Tags set in PutObject are retrievable via GetObjectTagging

CopyObject TaggingDirective
  - COPY  -> source tags are copied to destination
  - REPLACE -> destination gets new (or empty) tags; source unchanged
"""

from __future__ import annotations

import pytest

# Object tagging (PutObjectTagging / GetObjectTagging / DeleteObjectTagging)
# is not yet implemented in OSP — it returns 501 NotImplemented.
# These tests document the expected behaviour and serve as regression tests
# once the feature is added.
pytestmark = pytest.mark.xfail(
    reason="Garage does not implement the S3 object tagging API "
    "(PutObjectTagging / GetObjectTagging / DeleteObjectTagging). "
    "These tests document expected behaviour for when a tagging-capable "
    "backend is used.",
    strict=False,  # allow unexpected passes if backend adds support
)


# ── Helpers ───────────────────────────────────────────────────────────────────


def _put(s3, bucket, key, body=b"tag-me", **kwargs):
    return s3.put_object(Bucket=bucket, Key=key, Body=body, **kwargs)


def _tags_dict(s3, bucket, key) -> dict[str, str]:
    resp = s3.get_object_tagging(Bucket=bucket, Key=key)
    return {t["Key"]: t["Value"] for t in resp.get("TagSet", [])}


def _tag_set(tags: dict[str, str]) -> list[dict]:
    return [{"Key": k, "Value": v} for k, v in tags.items()]


# ── PutObjectTagging / GetObjectTagging ───────────────────────────────────────


class TestObjectTagging:
    def test_put_and_get_single_tag(self, s3, bucket, prefix):
        key = f"{prefix}tag-single.txt"
        _put(s3, bucket, key)
        s3.put_object_tagging(
            Bucket=bucket, Key=key, Tagging={"TagSet": _tag_set({"env": "test"})}
        )
        assert _tags_dict(s3, bucket, key) == {"env": "test"}

    def test_put_and_get_multiple_tags(self, s3, bucket, prefix):
        key = f"{prefix}tag-multi.txt"
        tags = {"project": "osp", "stage": "integration", "owner": "ci"}
        _put(s3, bucket, key)
        s3.put_object_tagging(
            Bucket=bucket, Key=key, Tagging={"TagSet": _tag_set(tags)}
        )
        assert _tags_dict(s3, bucket, key) == tags

    def test_overwrite_tags(self, s3, bucket, prefix):
        key = f"{prefix}tag-overwrite.txt"
        _put(s3, bucket, key)
        s3.put_object_tagging(
            Bucket=bucket, Key=key, Tagging={"TagSet": _tag_set({"a": "1"})}
        )
        # Replace with different tag set
        s3.put_object_tagging(
            Bucket=bucket, Key=key, Tagging={"TagSet": _tag_set({"b": "2"})}
        )
        result = _tags_dict(s3, bucket, key)
        assert result == {"b": "2"}
        assert "a" not in result

    def test_no_tags_returns_empty_set(self, s3, bucket, prefix):
        key = f"{prefix}tag-empty.txt"
        _put(s3, bucket, key)
        resp = s3.get_object_tagging(Bucket=bucket, Key=key)
        assert resp["TagSet"] == []


# ── DeleteObjectTagging ───────────────────────────────────────────────────────


class TestDeleteObjectTagging:
    def test_delete_removes_all_tags(self, s3, bucket, prefix):
        key = f"{prefix}tag-del.txt"
        _put(s3, bucket, key)
        s3.put_object_tagging(
            Bucket=bucket, Key=key, Tagging={"TagSet": _tag_set({"keep": "no"})}
        )
        s3.delete_object_tagging(Bucket=bucket, Key=key)
        assert _tags_dict(s3, bucket, key) == {}

    def test_delete_tags_on_untagged_object_is_noop(self, s3, bucket, prefix):
        key = f"{prefix}tag-del-noop.txt"
        _put(s3, bucket, key)
        # Should not raise
        s3.delete_object_tagging(Bucket=bucket, Key=key)
        assert _tags_dict(s3, bucket, key) == {}


# ── Inline tagging at upload time ─────────────────────────────────────────────


class TestInlineTagging:
    def test_tagging_kwarg_on_put_object(self, s3, bucket, prefix):
        key = f"{prefix}tag-inline.txt"
        # boto3 sends this as the x-amz-tagging header (URL-encoded)
        s3.put_object(
            Bucket=bucket,
            Key=key,
            Body=b"inline",
            Tagging="env=prod&tier=free",
        )
        tags = _tags_dict(s3, bucket, key)
        assert tags.get("env") == "prod"
        assert tags.get("tier") == "free"

    def test_inline_tag_single_kv(self, s3, bucket, prefix):
        key = f"{prefix}tag-inline-single.txt"
        s3.put_object(Bucket=bucket, Key=key, Body=b"x", Tagging="type=test")
        assert _tags_dict(s3, bucket, key).get("type") == "test"


# ── CopyObject — TaggingDirective ─────────────────────────────────────────────


class TestCopyTaggingDirective:
    def test_tagging_copy_directive_inherits_source_tags(self, s3, bucket, prefix):
        """Default COPY: destination inherits source's tags."""
        src = f"{prefix}tag-src.txt"
        dst = f"{prefix}tag-dst-copy.txt"
        _put(s3, bucket, src)
        s3.put_object_tagging(
            Bucket=bucket, Key=src, Tagging={"TagSet": _tag_set({"from": "source"})}
        )
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            TaggingDirective="COPY",
        )
        assert _tags_dict(s3, bucket, dst).get("from") == "source"

    def test_tagging_replace_directive_sets_new_tags(self, s3, bucket, prefix):
        """REPLACE: destination gets explicitly provided tags; source unchanged."""
        src = f"{prefix}tag-src-r.txt"
        dst = f"{prefix}tag-dst-replace.txt"
        _put(s3, bucket, src)
        s3.put_object_tagging(
            Bucket=bucket, Key=src, Tagging={"TagSet": _tag_set({"original": "yes"})}
        )
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            TaggingDirective="REPLACE",
            Tagging="replaced=yes",
        )
        dst_tags = _tags_dict(s3, bucket, dst)
        assert dst_tags.get("replaced") == "yes"
        assert "original" not in dst_tags

        # Source tags must be unaffected
        src_tags = _tags_dict(s3, bucket, src)
        assert src_tags.get("original") == "yes"

    def test_tagging_replace_with_empty_clears_tags(self, s3, bucket, prefix):
        """REPLACE with empty Tagging string drops all tags on destination."""
        src = f"{prefix}tag-src-clr.txt"
        dst = f"{prefix}tag-dst-cleared.txt"
        _put(s3, bucket, src)
        s3.put_object_tagging(
            Bucket=bucket,
            Key=src,
            Tagging={"TagSet": _tag_set({"shouldvanish": "yes"})},
        )
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            TaggingDirective="REPLACE",
            Tagging="",
        )
        assert _tags_dict(s3, bucket, dst) == {}
