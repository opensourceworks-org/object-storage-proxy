"""
test_duckdb.py — DuckDB httpfs read/write tests via the OSP proxy.

Mirrors test_spark.py but uses DuckDB's httpfs extension instead of Spark s3a.
Tests are parametrized over the ``backend`` fixture so they run against both
Garage and MinIO (MinIO is skipped automatically when .env.minio is absent).

Each test uses a unique prefix for full isolation.
The DuckDB connection is shared across the session (one per backend) because
httpfs extension installation is a one-time cost.
"""

from __future__ import annotations

import pytest


pytestmark = pytest.mark.duckdb


# ── Helpers ───────────────────────────────────────────────────────────────────


def _s3(bucket: str, prefix: str, filename: str) -> str:
    return f"s3://{bucket}/{prefix}{filename}"


# ── Tests ─────────────────────────────────────────────────────────────────────


def test_duckdb_write_read_parquet(duckdb_conn, s3, bucket, prefix):
    """Write a small table as Parquet via OSP, read it back with DuckDB."""
    path = _s3(bucket, prefix, "data.parquet")

    # Write via boto3 (DuckDB COPY TO uses HTTPFS which goes through the proxy)
    duckdb_conn.execute(
        f"""
        COPY (SELECT unnest([1,2,3]) AS id, unnest(['alpha','beta','gamma']) AS name)
        TO '{path}' (FORMAT parquet)
        """
    )

    result = duckdb_conn.execute(
        f"SELECT * FROM read_parquet('{path}') ORDER BY id"
    ).fetchall()
    assert len(result) == 3
    assert [r[0] for r in result] == [1, 2, 3]
    assert [r[1] for r in result] == ["alpha", "beta", "gamma"]


def test_duckdb_write_read_csv(duckdb_conn, s3, bucket, prefix):
    """Write CSV via OSP, read back and verify a derived column."""
    path = _s3(bucket, prefix, "data.csv")

    duckdb_conn.execute(
        f"""
        COPY (SELECT i AS x, i * 2 AS y FROM range(5) t(i))
        TO '{path}' (FORMAT csv, HEADER true)
        """
    )

    result = duckdb_conn.execute(
        f"SELECT x, y FROM read_csv('{path}', AUTO_DETECT=true)"
    ).fetchall()
    assert len(result) == 5
    total_y = sum(r[1] for r in result)
    assert total_y == sum(i * 2 for i in range(5))


def test_duckdb_overwrite_replaces_data(duckdb_conn, s3, bucket, prefix):
    """A second COPY TO the same path should replace — not append — the data."""
    path = _s3(bucket, prefix, "overwrite.parquet")

    duckdb_conn.execute(
        f"COPY (SELECT unnest([10, 20]) AS val) TO '{path}' (FORMAT parquet)"
    )
    # Overwrite with completely different data
    duckdb_conn.execute(
        f"COPY (SELECT unnest([99]) AS val) TO '{path}' (FORMAT parquet)"
    )

    result = duckdb_conn.execute(f"SELECT val FROM read_parquet('{path}')").fetchall()
    assert len(result) == 1
    assert result[0][0] == 99


def test_duckdb_empty_result(duckdb_conn, s3, bucket, prefix):
    """A table written with zero rows should be readable and return zero rows."""
    path = _s3(bucket, prefix, "empty.parquet")

    duckdb_conn.execute(
        f"COPY (SELECT 1 AS id WHERE false) TO '{path}' (FORMAT parquet)"
    )

    result = duckdb_conn.execute(f"SELECT * FROM read_parquet('{path}')").fetchall()
    assert result == []


def test_duckdb_large_dataset(duckdb_conn, s3, bucket, prefix):
    """Round-trip 10 000 rows to confirm multi-row-group Parquet works end-to-end."""
    path = _s3(bucket, prefix, "large.parquet")

    duckdb_conn.execute(
        f"""
        COPY (
            SELECT i AS id, 'value_' || i::VARCHAR AS label, (i % 100) * 1.5 AS score
            FROM range(10000) t(i)
        )
        TO '{path}' (FORMAT parquet)
        """
    )

    count = duckdb_conn.execute(
        f"SELECT count(*) FROM read_parquet('{path}')"
    ).fetchone()[0]
    assert count == 10_000

    total_score = float(
        duckdb_conn.execute(
            f"SELECT sum(score) FROM read_parquet('{path}')"
        ).fetchone()[0]
    )
    expected = sum((i % 100) * 1.5 for i in range(10_000))
    assert abs(total_score - expected) < 1e-6
