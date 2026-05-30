"""Structured result types for CLI output.

Re-exported from symeraseme.core.result_types for backward compatibility.
Service handlers should import from symeraseme.core.result_types directly
 to avoid circular imports with the CLI package.
"""

from symeraseme.core.result_types import CliResult

__all__ = ["CliResult"]
