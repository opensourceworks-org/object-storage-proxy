"""
test_spark.py — Spark s3a read/write tests via the OSP proxy.

Each test uses a unique prefix so they're fully isolated.
The SparkSession is shared across the module (session scope) because
Spark startup is expensive (~20-40 s on first run).
"""

from __future__ import annotations

import pytest


pytestmark = pytest.mark.spark


# ── Helpers ───────────────────────────────────────────────────────────────────


def _s3a(bucket: str, prefix: str) -> str:
    return f"s3a://{bucket}/{prefix}"


# ── Tests ─────────────────────────────────────────────────────────────────────


def test_spark_write_read_parquet(spark_session, bucket, prefix):
    """Write a small DataFrame as Parquet, read it back, verify contents."""
    path = _s3a(bucket, f"{prefix}parquet/")
    rows = [(1, "alpha"), (2, "beta"), (3, "gamma")]
    df = spark_session.createDataFrame(rows, ["id", "name"])

    df.write.mode("overwrite").parquet(path)

    result = spark_session.read.parquet(path)
    assert result.count() == 3
    ids = sorted(r.id for r in result.collect())
    assert ids == [1, 2, 3]


def test_spark_write_read_json(spark_session, bucket, prefix):
    """Write JSON, read back and verify a computed column survives the round-trip."""
    path = _s3a(bucket, f"{prefix}json/")
    rows = [{"x": i, "y": i * 2} for i in range(5)]
    df = spark_session.createDataFrame(rows)

    df.write.mode("overwrite").json(path)

    result = spark_session.read.json(path)
    assert result.count() == 5
    total_y = sum(r.y for r in result.collect())
    assert total_y == sum(i * 2 for i in range(5))


def test_spark_overwrite_replaces_data(spark_session, bucket, prefix):
    """A second overwrite write should replace — not append — the data."""
    path = _s3a(bucket, f"{prefix}overwrite/")

    first = spark_session.createDataFrame([(1, "old")], ["id", "val"])
    first.write.mode("overwrite").parquet(path)

    second = spark_session.createDataFrame([(2, "new"), (3, "new")], ["id", "val"])
    second.write.mode("overwrite").parquet(path)

    result = spark_session.read.parquet(path)
    assert result.count() == 2
    vals = {r.val for r in result.collect()}
    assert vals == {"new"}


def test_spark_empty_dataframe(spark_session, bucket, prefix):
    """Writing an empty DataFrame and reading it back should yield zero rows."""
    path = _s3a(bucket, f"{prefix}empty/")
    df = spark_session.createDataFrame(
        [], spark_session.createDataFrame([(0, "")], ["id", "val"]).schema
    )

    df.write.mode("overwrite").parquet(path)

    result = spark_session.read.parquet(path)
    assert result.count() == 0


def test_spark_large_dataframe(spark_session, bucket, prefix):
    """Write 10 000 rows to exercise multi-part / multi-file behaviour."""
    path = _s3a(bucket, f"{prefix}large/")
    rows = [(i, f"val-{i}") for i in range(10_000)]
    df = spark_session.createDataFrame(rows, ["id", "val"])

    df.write.mode("overwrite").parquet(path)

    result = spark_session.read.parquet(path)
    assert result.count() == 10_000
