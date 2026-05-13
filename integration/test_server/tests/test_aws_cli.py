"""
test_aws_cli.py — AWS CLI operations through the OSP proxy.

Uses subprocess to run the real `aws s3` / `aws s3api` commands so we
exercise the actual CLI flag surface (--recursive, --human-readable,
--summarize, --delete, --exclude, --include, etc.).

All credentials and the endpoint URL are injected via environment variables
(the `aws_env` fixture) so no ~/.aws profile config is required.

Requires: `aws` CLI v2 installed and on PATH.
"""

from __future__ import annotations

import re
import shutil
import subprocess
from pathlib import Path

import pytest


# ── Skip entire module if aws CLI is not installed ────────────────────────────

pytestmark = pytest.mark.skipif(
    shutil.which("aws") is None,
    reason="aws CLI not found on PATH",
)


# ── Helpers ───────────────────────────────────────────────────────────────────


def aws(
    *args: str,
    env: dict,
    cwd: Path | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess:
    cmd = ["aws", *args]
    return subprocess.run(
        cmd,
        env=env,
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        check=check,
    )


def s3_url(bucket: str, key: str = "") -> str:
    return f"s3://{bucket}/{key}"


# ── aws s3 ls ─────────────────────────────────────────────────────────────────


class TestAwsLs:
    def test_ls_bucket(self, s3, bucket, prefix, aws_env):
        s3.put_object(Bucket=bucket, Key=f"{prefix}ls-test.txt", Body=b"ls")
        result = aws("s3", "ls", s3_url(bucket, prefix), env=aws_env)
        assert "ls-test.txt" in result.stdout

    def test_ls_recursive(self, s3, bucket, prefix, aws_env):
        for i in range(3):
            s3.put_object(Bucket=bucket, Key=f"{prefix}sub/file-{i}.txt", Body=b"x")
        result = aws("s3", "ls", s3_url(bucket, prefix), "--recursive", env=aws_env)
        assert result.stdout.count("file-") == 3

    def test_ls_recursive_human_readable_summarize(self, s3, bucket, prefix, aws_env):
        for i in range(3):
            s3.put_object(
                Bucket=bucket,
                Key=f"{prefix}hr/obj-{i}.dat",
                Body=b"a" * 1024,
            )
        result = aws(
            "s3",
            "ls",
            s3_url(bucket, prefix),
            "--recursive",
            "--human-readable",
            "--summarize",
            env=aws_env,
        )
        stdout = result.stdout
        # --summarize adds a "Total Objects" and "Total Size" footer
        assert "Total Objects" in stdout
        assert "Total Size" in stdout
        # --human-readable should produce a unit suffix (Bytes / KiB / MiB)
        assert re.search(r"\d+\s+(Bytes|KiB|MiB|GiB)", stdout)

    def test_ls_nonexistent_prefix_returns_empty(self, aws_env, bucket):
        result = aws(
            "s3", "ls", s3_url(bucket, "does-not-exist/"), env=aws_env, check=False
        )
        # aws s3 ls returns exit code 1 with empty output for a missing prefix
        assert result.returncode in (0, 1)
        assert result.stdout.strip() == ""


# ── aws s3 cp ─────────────────────────────────────────────────────────────────


class TestAwsCp:
    def test_cp_upload_file(self, tmp_dir, bucket, prefix, aws_env):
        local = tmp_dir / "upload.txt"
        local.write_bytes(b"aws cp upload")
        aws("s3", "cp", str(local), s3_url(bucket, f"{prefix}upload.txt"), env=aws_env)
        # Verify via CLI download
        out = tmp_dir / "downloaded.txt"
        aws("s3", "cp", s3_url(bucket, f"{prefix}upload.txt"), str(out), env=aws_env)
        assert out.read_bytes() == b"aws cp upload"

    def test_cp_download_file(self, s3, tmp_dir, bucket, prefix, aws_env):
        key = f"{prefix}download.txt"
        body = b"download-me"
        s3.put_object(Bucket=bucket, Key=key, Body=body)
        dest = tmp_dir / "result.txt"
        aws("s3", "cp", s3_url(bucket, key), str(dest), env=aws_env)
        assert dest.read_bytes() == body

    def test_cp_upload_with_content_type(self, tmp_dir, s3, bucket, prefix, aws_env):
        local = tmp_dir / "data.json"
        local.write_text('{"ok": true}')
        aws(
            "s3",
            "cp",
            str(local),
            s3_url(bucket, f"{prefix}data.json"),
            "--content-type",
            "application/json",
            env=aws_env,
        )
        resp = s3.head_object(Bucket=bucket, Key=f"{prefix}data.json")
        assert "json" in resp.get("ContentType", "").lower()

    def test_cp_recursive_upload_directory(self, tmp_dir, s3, bucket, prefix, aws_env):
        src_dir = tmp_dir / "src"
        src_dir.mkdir()
        for i in range(4):
            (src_dir / f"file-{i}.txt").write_bytes(f"content {i}".encode())

        aws(
            "s3",
            "cp",
            str(src_dir),
            s3_url(bucket, f"{prefix}dir/"),
            "--recursive",
            env=aws_env,
        )

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}dir/")
        keys = [o["Key"] for o in resp.get("Contents", [])]
        assert len(keys) == 4

    def test_cp_recursive_download_directory(
        self, s3, tmp_dir, bucket, prefix, aws_env
    ):
        for i in range(3):
            s3.put_object(
                Bucket=bucket, Key=f"{prefix}dl-dir/f{i}.bin", Body=bytes([i] * 10)
            )

        dest_dir = tmp_dir / "dest"
        dest_dir.mkdir()
        aws(
            "s3",
            "cp",
            s3_url(bucket, f"{prefix}dl-dir/"),
            str(dest_dir),
            "--recursive",
            env=aws_env,
        )

        files = list(dest_dir.iterdir())
        assert len(files) == 3


# ── aws s3 sync ───────────────────────────────────────────────────────────────


class TestAwsSync:
    def test_sync_upload(self, tmp_dir, s3, bucket, prefix, aws_env):
        src = tmp_dir / "sync-src"
        src.mkdir()
        for i in range(5):
            (src / f"item-{i}.txt").write_bytes(f"item {i}".encode())

        aws("s3", "sync", str(src), s3_url(bucket, f"{prefix}sync/"), env=aws_env)

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}sync/")
        assert len(resp.get("Contents", [])) == 5

    def test_sync_is_incremental(self, tmp_dir, s3, bucket, prefix, aws_env):
        src = tmp_dir / "inc"
        src.mkdir()
        (src / "a.txt").write_bytes(b"a")
        remote = s3_url(bucket, f"{prefix}inc/")

        aws("s3", "sync", str(src), remote, env=aws_env)

        # Second sync of same content should upload nothing
        result = aws("s3", "sync", str(src), remote, env=aws_env)
        assert "upload:" not in result.stdout.lower()

    def test_sync_delete_removes_orphans(self, tmp_dir, s3, bucket, prefix, aws_env):
        """After uploading 3 files, remove one locally and sync --delete."""
        src = tmp_dir / "del-src"
        src.mkdir()
        for name in ("keep1.txt", "keep2.txt", "remove.txt"):
            (src / name).write_bytes(name.encode())

        remote = s3_url(bucket, f"{prefix}del-sync/")
        aws("s3", "sync", str(src), remote, env=aws_env)

        # Remove the third file locally
        (src / "remove.txt").unlink()
        aws("s3", "sync", str(src), remote, "--delete", env=aws_env)

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}del-sync/")
        keys = [Path(o["Key"]).name for o in resp.get("Contents", [])]
        assert "keep1.txt" in keys
        assert "keep2.txt" in keys
        assert "remove.txt" not in keys

    def test_sync_download(self, s3, tmp_dir, bucket, prefix, aws_env):
        for i in range(3):
            s3.put_object(
                Bucket=bucket, Key=f"{prefix}sync-dl/{i}.txt", Body=bytes([i])
            )

        dest = tmp_dir / "dl"
        dest.mkdir()
        aws("s3", "sync", s3_url(bucket, f"{prefix}sync-dl/"), str(dest), env=aws_env)

        files = list(dest.iterdir())
        assert len(files) == 3

    def test_sync_with_exclude_include(self, tmp_dir, s3, bucket, prefix, aws_env):
        src = tmp_dir / "filter-src"
        src.mkdir()
        (src / "keep.txt").write_bytes(b"keep")
        (src / "skip.log").write_bytes(b"skip")

        remote = s3_url(bucket, f"{prefix}filtered/")
        aws(
            "s3",
            "sync",
            str(src),
            remote,
            "--exclude",
            "*.log",
            env=aws_env,
        )

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}filtered/")
        keys = [Path(o["Key"]).name for o in resp.get("Contents", [])]
        assert "keep.txt" in keys
        assert "skip.log" not in keys


# ── aws s3 rm ─────────────────────────────────────────────────────────────────


class TestAwsRm:
    def test_rm_single_object(self, s3, bucket, prefix, aws_env):
        key = f"{prefix}rm-me.txt"
        s3.put_object(Bucket=bucket, Key=key, Body=b"delete me")
        aws("s3", "rm", s3_url(bucket, key), env=aws_env)
        with pytest.raises(Exception):
            s3.head_object(Bucket=bucket, Key=key)

    def test_rm_recursive(self, s3, bucket, prefix, aws_env):
        keys = [f"{prefix}rm-dir/file-{i}.txt" for i in range(5)]
        for k in keys:
            s3.put_object(Bucket=bucket, Key=k, Body=b"x")

        aws("s3", "rm", s3_url(bucket, f"{prefix}rm-dir/"), "--recursive", env=aws_env)

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}rm-dir/")
        assert len(resp.get("Contents", [])) == 0

    def test_rm_recursive_with_exclude(self, s3, bucket, prefix, aws_env):
        s3.put_object(Bucket=bucket, Key=f"{prefix}excl/keep.txt", Body=b"keep")
        s3.put_object(Bucket=bucket, Key=f"{prefix}excl/remove.dat", Body=b"rm")

        aws(
            "s3",
            "rm",
            s3_url(bucket, f"{prefix}excl/"),
            "--recursive",
            "--exclude",
            "*.txt",
            env=aws_env,
        )

        resp = s3.list_objects_v2(Bucket=bucket, Prefix=f"{prefix}excl/")
        keys = [Path(o["Key"]).name for o in resp.get("Contents", [])]
        assert "keep.txt" in keys
        assert "remove.dat" not in keys
