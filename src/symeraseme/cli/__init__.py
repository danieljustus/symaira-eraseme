"""Openeraseme CLI package."""

import sys

__all__ = ["app"]


def __getattr__(name: str):
    if name == "app":
        from symeraseme.cli.app import app
        from symeraseme.core._compat import check_pydantic_core_compat

        check_pydantic_core_compat()
        return app
    msg = f"module {__name__!r} has no attribute {name!r}"
    raise AttributeError(msg)


# Prevent the submodule `symeraseme.cli.app` from shadowing the lazy-loaded
# `app` object. Python auto-adds imported submodules as attributes, so we
# must delete the module reference to let __getattr__ resolve to the Typer
# instance on attribute access.
_mod = sys.modules[__name__]
if hasattr(_mod, "app"):
    delattr(_mod, "app")
