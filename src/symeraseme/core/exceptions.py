"""Domain exceptions for symeraseme.

All domain errors raised by the core layer inherit from SymerasemeError.
Service-layer handlers catch these and convert them to CliResult at the boundary.
"""

from __future__ import annotations

EXIT_OK = 0
EXIT_ERROR = 1
EXIT_CONFIG = 2
EXIT_NETWORK = 3


def safe_error_str(e: Exception, max_len: int = 300) -> str:
    """Truncate an exception string to prevent sensitive data leakage.

    SMTP/email SDK exceptions may include server banners, session tokens,
    or connection details. This helper ensures persisted error payloads
    are bounded in length.
    """
    return str(e)[:max_len]


class SymerasemeError(Exception):
    """Base class for all domain errors."""

    exit_code: int = EXIT_ERROR

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class ProfileError(SymerasemeError):
    """Identity profile missing, incomplete, or unreadable."""

    exit_code = EXIT_CONFIG


class RequestNotFoundError(SymerasemeError):
    """Removal request ID does not exist in the event store."""

    def __init__(self, request_id: int) -> None:
        self.request_id = request_id
        super().__init__(f"Request {request_id} not found")


class ExecutionError(SymerasemeError):
    """Sending or executing a removal request failed."""

    def __init__(self, message: str, request_id: int | None = None) -> None:
        self.request_id = request_id
        super().__init__(message)


class RegistryError(SymerasemeError):
    """Broker registry directory not found or broker not found in registry."""
