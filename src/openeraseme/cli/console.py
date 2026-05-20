"""Shared rich Console and display helpers for CLI output."""

from __future__ import annotations

from typing import Any

from rich.console import Console as _RichConsole
from rich.panel import Panel
from rich.progress import Progress, SpinnerColumn, TextColumn
from rich.table import Table
from rich.text import Text

console = _RichConsole()


def print_success(message: str) -> None:
    """Print a success message in green."""
    console.print(f"[green]✓[/green] {message}")


def print_error(message: str) -> None:
    """Print an error message in red to stderr."""
    console.print(f"[red]✗[/red] {message}", style="red")


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


def spinner_progress(description: str = "Working...") -> Progress:
    """Return a Progress with a spinner for indeterminate progress."""
    progress = Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        transient=True,
    )
    progress.add_task(description=description, total=None)
    return progress
