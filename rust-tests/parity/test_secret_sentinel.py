#!/usr/bin/env python3
"""Check that profile setup does not expose its synthetic master key."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main() -> None:
    binary = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="symeraseme-secret-sentinel-") as root:
        home = Path(root)
        for index, key in enumerate(("0123456789abcdef" * 4, "synthetic-master-key-sentinel")):
            data = home / str(index)
            data.mkdir()
            env = dict(os.environ)
            env.update(
                HOME=str(data),
                SYMERASEME_DATA_DIR=str(data),
                SYMERASEME_IDENTITY_MASTER_KEY=key,
            )
            result = subprocess.run(
                [str(binary), "init-profile", "--full-name", "Test User", "--email", "test@example.invalid"],
                cwd=data,
                env=env,
                capture_output=True,
                timeout=10,
                check=False,
            )
            assert result.returncode == 0, "profile setup failed"
            needle = key.encode()
            assert needle not in result.stdout, "master key leaked to stdout"
            assert needle not in result.stderr, "master key leaked to stderr"
            for path in data.rglob("*"):
                if path.is_file():
                    assert needle not in path.read_bytes(), f"master key leaked to {path.name}"
        print("secret sentinel scan passed: stdout, stderr, generated files")


if __name__ == "__main__":
    main()
