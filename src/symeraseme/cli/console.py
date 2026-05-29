"""Shared rich Console and display helpers for CLI output."""

from __future__ import annotations

from collections.abc import Generator
from contextlib import contextmanager
from enum import StrEnum
from typing import Any, NoReturn

import typer
from rich.console import Console as _RichConsole
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, TextColumn
from rich.table import Table
from rich.text import Text

from symeraseme.cli.types import CliResult

console = _RichConsole()
_error_console = _RichConsole(stderr=True)


def print_success(message: str) -> None:
    """Print a success message in green."""
    console.print(f"[green]✓[/green] {message}")


def print_error(message: str) -> None:
    """Print an error message in red to stderr."""
    _error_console.print(f"[red]✗[/red] {message}", style="red")


def print_info(message: str) -> None:
    """Print an informational message."""
    console.print(f"[dim]ℹ[/dim] {message}")


def print_warning(message: str) -> None:
    """Print a warning in yellow."""
    console.print(f"[yellow]⚠[/yellow] {message}")


def print_panel(title: str, content: str, **kwargs: Any) -> None:
    """Print content inside a rich Panel."""
    panel = Panel(Text(content), title=title, **kwargs)
    console.print(panel)


def make_table(title: str, columns: list[str], rows: list[list[str]]) -> Table:
    """Build a rich Table from column names and row data."""
    table = Table(title=title, title_style="bold")
    for col in columns:
        table.add_column(col)
    for row in rows:
        table.add_row(*row)
    return table


def print_table(title: str, columns: list[str], rows: list[list[str]]) -> None:
    """Build and print a rich Table."""
    console.print(make_table(title, columns, rows))


@contextmanager
def show_spinner(description: str = "Working...") -> Generator[Progress, None, None]:
    """Show a transient spinner during a long-running operation.

    Usage::

        with show_spinner("Processing..."):
            result = long_running_operation()

    The spinner is hidden automatically when the block exits.
    """
    progress = Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        transient=True,
        console=_error_console,
    )
    with progress:
        progress.add_task(description=description, total=None)
        yield progress


class OutputFormat(StrEnum):
    text = "text"
    json = "json"


def render_result(
    output_format: str,
    result: str | CliResult,
    result_obj: CliResult | None = None,
) -> None:
    """Print the result of a command handler, formatted appropriately.

    For JSON output the raw string is printed as-is (soft_wrap to avoid
    rich inserting line breaks into the serialized data).
    For text output the result is wrapped in a rich Panel when the content
    spans multiple lines or carries an error.

    Raises typer.Exit(1) when the result indicates failure so every command
    returns a non-zero exit code uniformly.
    """
    if isinstance(result, CliResult):
        result_obj = result
        result = result.message

    if output_format == "json":
        if result_obj is not None:
            import json as _json

            console.print(
                _json.dumps(result_obj.data, indent=2, default=str),
                markup=False,
                soft_wrap=True,
            )
        else:
            console.print(result, markup=False, soft_wrap=True)
    elif result_obj is not None and not result_obj.success:
        print_error(result_obj.message)
    elif "\n" not in result.strip():
        console.print(result, markup=False, soft_wrap=True)
    else:
        print_panel("Output", result.strip())

    if result_obj is not None and not result_obj.success:
        raise typer.Exit(1)


def render_error(message: str) -> NoReturn:
    """Print an error message and exit."""
    print_error(message)
    raise typer.Exit(1)
