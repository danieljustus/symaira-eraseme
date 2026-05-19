"""CLI entrypoint for OpenEraseMe."""

from enum import StrEnum

import typer

app = typer.Typer(
    name="openeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
)


class OutputFormat(StrEnum):
    text = "text"
    json = "json"


@app.callback()
def main(
    ctx: typer.Context,
    output: OutputFormat = OutputFormat.text,
) -> None:
    ctx.ensure_object(dict)
    ctx.obj["output"] = output


@app.command()
def version() -> None:
    from openeraseme import __version__

    typer.echo(f"OpenEraseMe v{__version__}")


@app.command()
def init_profile(
    full_name: str = typer.Option(..., prompt="Full name"),
    email: str = typer.Option(..., prompt="Email address"),
) -> None:
    """Create or update your encrypted identity profile."""
    from openeraseme.core.identity import profile_exists, save_profile
    from openeraseme.registry.schema import IdentityProfile

    profile = IdentityProfile(full_name=full_name, email_addresses=[email])
    path = save_profile(profile)
    action = "Updated" if profile_exists() else "Created"
    typer.echo(f"{action} encrypted identity profile at {path}")


@app.command()
def show_profile() -> None:
    """Display your current identity profile."""
    from openeraseme.core.identity import load_profile, profile_exists

    if not profile_exists():
        typer.echo("No identity profile found. Run 'openeraseme init-profile' first.")
        raise typer.Exit(1)

    profile = load_profile()
    typer.echo(f"Name:  {profile.full_name}")
    for e in profile.email_addresses:
        typer.echo(f"Email: {e}")
    for a in profile.addresses:
        typer.echo(f"Address: {a.street}, {a.city}, {a.country}")
    for j in profile.jurisdictions:
        typer.echo(f"Jurisdiction: {j}")


if __name__ == "__main__":
    app()
