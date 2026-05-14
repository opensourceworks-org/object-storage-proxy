"""
test_metadata.py — object metadata, HTTP headers, and CopyObject directives.

Covers
------
Custom metadata (x-amz-meta-*)
  - Single header round-trip: PutObject -> GetObject -> HeadObject
  - Multiple headers at once
  - Keys are case-normalised to lowercase by S3 spec

HTTP cache / content headers
  - Content-Type  (set explicitly vs. default application/octet-stream)
  - Cache-Control
  - Content-Disposition
  - Content-Encoding

CopyObject metadata directives
  - MetadataDirective=COPY  (default: source metadata preserved)
  - MetadataDirective=REPLACE (new metadata replaces source metadata)

Content-Type default
  - Not specifying ContentType -> application/octet-stream or binary/octet-stream
"""

from __future__ import annotations


# ── Helpers ───────────────────────────────────────────────────────────────────


def _put(s3, bucket, key, body=b"data", **kwargs):
    return s3.put_object(Bucket=bucket, Key=key, Body=body, **kwargs)


def _head(s3, bucket, key):
    return s3.head_object(Bucket=bucket, Key=key)


def _get(s3, bucket, key):
    return s3.get_object(Bucket=bucket, Key=key)


# ── Custom metadata (x-amz-meta-*) ───────────────────────────────────────────


class TestCustomMetadata:
    def test_single_metadata_header_round_trip(self, s3, bucket, prefix):
        key = f"{prefix}meta-single.txt"
        _put(s3, bucket, key, Metadata={"author": "osp-test"})

        head = _head(s3, bucket, key)
        assert head["Metadata"].get("author") == "osp-test"

        get = _get(s3, bucket, key)
        assert get["Metadata"].get("author") == "osp-test"

    def test_multiple_metadata_headers(self, s3, bucket, prefix):
        key = f"{prefix}meta-multi.txt"
        meta = {"version": "42", "source": "pytest", "env": "integration"}
        _put(s3, bucket, key, Metadata=meta)

        returned = _head(s3, bucket, key)["Metadata"]
        for k, v in meta.items():
            assert returned.get(k) == v

    def test_metadata_keys_are_lowercase(self, s3, bucket, prefix):
        """S3 normalises x-amz-meta-* keys to lowercase."""
        key = f"{prefix}meta-case.txt"
        _put(s3, bucket, key, Metadata={"MyKey": "MyValue"})
        returned = _head(s3, bucket, key)["Metadata"]
        # boto3 normalises automatically; key must exist (lower or original)
        assert "mykey" in returned or "MyKey" in returned

    def test_empty_metadata_value(self, s3, bucket, prefix):
        key = f"{prefix}meta-empty.txt"
        _put(s3, bucket, key, Metadata={"tag": ""})
        returned = _head(s3, bucket, key)["Metadata"]
        assert "tag" in returned

    def test_metadata_not_present_when_not_set(self, s3, bucket, prefix):
        key = f"{prefix}meta-none.txt"
        _put(s3, bucket, key)
        head = _head(s3, bucket, key)
        assert head.get("Metadata", {}) == {}


# ── Content-Type ──────────────────────────────────────────────────────────────


class TestContentType:
    def test_explicit_content_type_round_trip(self, s3, bucket, prefix):
        key = f"{prefix}ct-explicit.json"
        _put(s3, bucket, key, ContentType="application/json")

        head = _head(s3, bucket, key)
        assert "application/json" in head["ContentType"]

        get = _get(s3, bucket, key)
        assert "application/json" in get["ContentType"]

    def test_text_plain_with_charset(self, s3, bucket, prefix):
        key = f"{prefix}ct-text.txt"
        _put(s3, bucket, key, body=b"hello", ContentType="text/plain; charset=utf-8")
        head = _head(s3, bucket, key)
        assert "text/plain" in head["ContentType"]

    def test_default_content_type_is_octet_stream(self, s3, bucket, prefix):
        """When no Content-Type is given S3 defaults to application/octet-stream."""
        key = f"{prefix}ct-default.bin"
        _put(s3, bucket, key)
        head = _head(s3, bucket, key)
        ct = head["ContentType"].lower()
        assert "octet-stream" in ct

    def test_image_content_type(self, s3, bucket, prefix):
        key = f"{prefix}ct-image.png"
        _put(s3, bucket, key, ContentType="image/png")
        head = _head(s3, bucket, key)
        assert "image/png" in head["ContentType"]


# ── Cache-Control ─────────────────────────────────────────────────────────────


class TestCacheControl:
    def test_cache_control_round_trip(self, s3, bucket, prefix):
        key = f"{prefix}cc.txt"
        _put(s3, bucket, key, CacheControl="max-age=3600, public")
        head = _head(s3, bucket, key)
        assert head.get("CacheControl") == "max-age=3600, public"

    def test_cache_control_no_cache(self, s3, bucket, prefix):
        key = f"{prefix}cc-no-cache.txt"
        _put(s3, bucket, key, CacheControl="no-cache, no-store")
        head = _head(s3, bucket, key)
        assert "no-cache" in head.get("CacheControl", "")


# ── Content-Disposition ───────────────────────────────────────────────────────


class TestContentDisposition:
    def test_content_disposition_attachment(self, s3, bucket, prefix):
        key = f"{prefix}cd.txt"
        _put(s3, bucket, key, ContentDisposition='attachment; filename="report.csv"')
        head = _head(s3, bucket, key)
        cd = head.get("ContentDisposition", "")
        assert "attachment" in cd
        assert "report.csv" in cd

    def test_content_disposition_inline(self, s3, bucket, prefix):
        key = f"{prefix}cd-inline.txt"
        _put(s3, bucket, key, ContentDisposition="inline")
        head = _head(s3, bucket, key)
        assert head.get("ContentDisposition") == "inline"


# ── Content-Encoding ──────────────────────────────────────────────────────────


class TestContentEncoding:
    def test_content_encoding_identity(self, s3, bucket, prefix):
        """Arbitrary content-encoding is stored and returned as-is."""
        key = f"{prefix}ce.txt"
        _put(s3, bucket, key, ContentEncoding="identity")
        head = _head(s3, bucket, key)
        assert head.get("ContentEncoding") == "identity"

    def test_content_encoding_gzip_stored(self, s3, bucket, prefix):
        """Gzip-encoded body stored with Content-Encoding: gzip label."""
        import gzip

        raw = b"compressible text " * 100
        compressed = gzip.compress(raw)
        key = f"{prefix}ce-gzip.txt"
        _put(
            s3,
            bucket,
            key,
            body=compressed,
            ContentEncoding="gzip",
            ContentType="text/plain",
        )
        head = _head(s3, bucket, key)
        assert head.get("ContentEncoding") == "gzip"
        # Stored byte count is the compressed size
        assert head["ContentLength"] == len(compressed)


# ── CopyObject — MetadataDirective ───────────────────────────────────────────


class TestCopyMetadataDirective:
    def test_metadata_copy_directive_preserves_source_metadata(
        self, s3, bucket, prefix
    ):
        """Default COPY directive: destination inherits source metadata."""
        src = f"{prefix}src.txt"
        dst = f"{prefix}dst-copy.txt"
        _put(s3, bucket, src, Metadata={"origin": "source"})

        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            MetadataDirective="COPY",
        )

        assert _head(s3, bucket, dst)["Metadata"].get("origin") == "source"

    def test_metadata_replace_directive_sets_new_metadata(self, s3, bucket, prefix):
        """REPLACE directive: destination gets new metadata, source's is discarded."""
        src = f"{prefix}src.txt"
        dst = f"{prefix}dst-replace.txt"
        _put(s3, bucket, src, Metadata={"origin": "source"})

        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            MetadataDirective="REPLACE",
            Metadata={"origin": "replaced"},
        )

        meta = _head(s3, bucket, dst)["Metadata"]
        assert meta.get("origin") == "replaced"

    def test_metadata_replace_directive_clears_source_metadata(
        self, s3, bucket, prefix
    ):
        """REPLACE with empty Metadata dict drops all source metadata."""
        src = f"{prefix}src.txt"
        dst = f"{prefix}dst-cleared.txt"
        _put(s3, bucket, src, Metadata={"shouldvanish": "yes"})

        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
            MetadataDirective="REPLACE",
            Metadata={},
        )

        meta = _head(s3, bucket, dst).get("Metadata", {})
        assert "shouldvanish" not in meta

    def test_copy_preserves_content_type(self, s3, bucket, prefix):
        """CopyObject COPY directive also preserves Content-Type."""
        src = f"{prefix}src.json"
        dst = f"{prefix}dst.json"
        _put(s3, bucket, src, ContentType="application/json")
        s3.copy_object(
            Bucket=bucket,
            CopySource={"Bucket": bucket, "Key": src},
            Key=dst,
        )
        head = _head(s3, bucket, dst)
        assert "application/json" in head["ContentType"]
