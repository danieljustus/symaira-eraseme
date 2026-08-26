"""Tests for symeraseme.registry.sync."""

from __future__ import annotations

from pathlib import Path
from unittest.mock import patch

from symeraseme.registry.sync import _is_detached_head, _run_git_pull, sync_registry


class TestRunGitPull:
    def test_no_upstream_branch_skips_successfully(self, tmp_path):
        """A branch without upstream tracking must not surface a git error."""
        import subprocess

        with (
            patch("symeraseme.registry.sync._is_detached_head", return_value=False),
            patch("subprocess.run") as run_mock,
        ):

            def fake_run(args, **kwargs):
                args_list = list(args)
                if "@{upstream}" in args_list:
                    return subprocess.CompletedProcess(
                        args=args_list,
                        returncode=128,
                        stdout="",
                        stderr="fatal: no upstream configured for branch 'feature'",
                    )
                return subprocess.CompletedProcess(
                    args=args_list, returncode=0, stdout="", stderr=""
                )

            run_mock.side_effect = fake_run
            result = _run_git_pull(tmp_path)

        assert result["ok"] is True
        assert result.get("skipped") == "no-upstream"
        assert "no upstream" in result["message"].lower()

    def test_detached_head_still_skips(self, tmp_path):
        with patch("symeraseme.registry.sync._is_detached_head", return_value=True):
            result = _run_git_pull(tmp_path)
        assert result["ok"] is True
        assert result.get("detached_head") is True

    def test_pull_failure_is_reported_when_upstream_exists(self, tmp_path):
        import subprocess

        with (
            patch("symeraseme.registry.sync._is_detached_head", return_value=False),
            patch("subprocess.run") as run_mock,
        ):

            def fake_run(args, **kwargs):
                args_list = list(args)
                if "@{upstream}" in args_list:
                    return subprocess.CompletedProcess(
                        args=args_list, returncode=0, stdout="origin/main", stderr=""
                    )
                return subprocess.CompletedProcess(
                    args=args_list,
                    returncode=1,
                    stdout="",
                    stderr="error: could not fast-forward",
                )

            run_mock.side_effect = fake_run
            result = _run_git_pull(tmp_path)

        assert result["ok"] is False
        assert "could not fast-forward" in result["stderr"]
        assert "skipped" not in result


class TestSyncRegistry:
    @staticmethod
    def _patch_paths(tmp_path: Path):
        registry_dir = tmp_path / "registry"
        git_root = tmp_path / ".git-root-mock"
        return (
            patch("symeraseme.registry.sync._registry_dir", return_value=registry_dir),
            patch("symeraseme.registry.sync._find_git_root", return_value=git_root),
        )

    def test_git_mode_with_untracked_branch_reports_ok_skip(self, tmp_path):
        """The smoke-test scenario: git checkout, branch without upstream."""
        p1, p2 = self._patch_paths(tmp_path)
        with (
            p1,
            p2,
            patch(
                "symeraseme.registry.sync._run_git_pull",
                return_value={
                    "ok": True,
                    "skipped": "no-upstream",
                    "stdout": "",
                    "stderr": "",
                    "message": "Branch has no upstream tracking branch — nothing to pull. Skipping.",
                },
            ),
        ):
            data = sync_registry()

        assert data["mode"] == "git"
        assert data["ok"] is True

    def test_git_mode_failure_reports_not_ok(self, tmp_path):
        p1, p2 = self._patch_paths(tmp_path)
        with (
            p1,
            p2,
            patch(
                "symeraseme.registry.sync._run_git_pull",
                return_value={
                    "ok": False,
                    "exit_code": 1,
                    "stdout": "",
                    "stderr": "error: something broke",
                },
            ),
        ):
            data = sync_registry()

        assert data["mode"] == "git"
        assert data["ok"] is False


class TestIsDetachedHead:
    def test_returns_bool_for_scratch_dir(self, tmp_path):
        # No real git repo needed: the function must not raise on an
        # ordinary directory and must return a bool.
        result = _is_detached_head(tmp_path)
        assert isinstance(result, bool)
