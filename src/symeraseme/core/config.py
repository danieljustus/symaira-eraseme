"""Centralized configuration for default paths."""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Config:
    data_dir: str = "~/.local/share/symeraseme"
    config_dir: str = "~/.config/symeraseme"
    db_name: str = "symeraseme.db"

    @property
    def resolved_data_dir(self) -> Path:
        return Path(os.environ.get("SYMERASEME_DATA_DIR", self.data_dir)).expanduser()

    @property
    def resolved_config_dir(self) -> Path:
        return Path(self.config_dir).expanduser()

    @property
    def db_dir(self) -> Path:
        return Path(os.environ.get("SYMERASEME_DB_DIR", self.resolved_data_dir)).expanduser()

    @property
    def db_path(self) -> Path:
        return self.db_dir / self.db_name

    @property
    def consent_dir(self) -> Path:
        return self.resolved_data_dir

    @property
    def identity_path(self) -> Path:
        raw = os.environ.get("SYMERASEME_IDENTITY_PATH")
        if raw:
            return Path(raw).expanduser()
        return self.resolved_config_dir / "identity.enc"


def get_config() -> Config:
    return Config()
