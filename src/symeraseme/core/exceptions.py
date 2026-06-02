"""Domain exceptions for symeraseme.

All domain errors raised by the core layer inherit from SymerasemeError.
Service-layer handlers catch these and convert them to CliResult at the boundary.
"""

from __future__ import annotations


class SymerasemeError(Exception):
    """Base class for all domain errors."""

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message


class ProfileError(SymerasemeError):
    """Identity profile missing, incomplete, or unreadable."""


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
