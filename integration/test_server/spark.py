"""
spark.py — SparkSession factory for OSP integration tests.

Can also be run directly as a smoke-test:
    uv run python spark.py
"""

from __future__ import annotations

import os
from pathlib import Path

from dotenv import load_dotenv
from pyspark.sql import SparkSession

# hadoop-aws version must match the Hadoop JARs bundled with your PySpark build.
# PySpark 4.x ships with Hadoop 3.4.x.
_HADOOP_AWS_VERSION = "3.5.0"


def build_spark_session(
    *,
    access_key: str,
    secret_key: str,
    endpoint: str,
    region: str,
    app_name: str = "osp-integration",
    master: str = "local[*]",
) -> SparkSession:
    """Return a SparkSession configured to talk to OSP via s3a."""
    return (
        SparkSession.builder.appName(app_name)
        .master(master)
        # Resolve hadoop-aws + aws-java-sdk-bundle via Ivy at session start
        .config(
            "spark.jars.packages",
            f"org.apache.hadoop:hadoop-aws:{_HADOOP_AWS_VERSION}",
        )
        .config(
            "spark.hadoop.fs.s3a.impl",
            "org.apache.hadoop.fs.s3a.S3AFileSystem",
        )
        .config(
            "spark.hadoop.fs.s3a.aws.credentials.provider",
            "org.apache.hadoop.fs.s3a.SimpleAWSCredentialsProvider",
        )
        .config("spark.hadoop.fs.s3a.access.key", access_key)
        .config("spark.hadoop.fs.s3a.secret.key", secret_key)
        .config("spark.hadoop.fs.s3a.endpoint", endpoint)
        .config("spark.hadoop.fs.s3a.endpoint.region", region)
        .config("spark.hadoop.fs.s3a.path.style.access", "true")
        .config("spark.hadoop.fs.s3a.connection.ssl.enabled", "false")
        # Avoid noisy Spark UI on ephemeral test runs
        .config("spark.ui.enabled", "false")
        .getOrCreate()
    )


# ── Standalone smoke-test ──────────────────────────────────────────────────────

if __name__ == "__main__":
    env_file = Path(__file__).parent / ".env"
    load_dotenv(env_file)

    spark = build_spark_session(
        access_key=os.environ["OSP_CLIENT_ACCESS_KEY"],
        secret_key=os.environ["OSP_CLIENT_SECRET_KEY"],
        endpoint=f"http://{os.environ['OSP_PROXY_HOST']}:{os.environ['OSP_PROXY_PORT']}",
        region=os.environ.get("GARAGE_REGION", "garage"),
    )

    bucket = os.environ["GARAGE_BUCKET"]
    path = f"s3a://{bucket}/spark-smoke-test/"

    df = spark.createDataFrame([(1, "hello"), (2, "world")], ["id", "msg"])
    df.write.mode("overwrite").parquet(path)
    result = spark.read.parquet(path)
    result.show()
    print(f"✅  Smoke test passed — {result.count()} rows round-tripped via {path}")
    spark.stop()
