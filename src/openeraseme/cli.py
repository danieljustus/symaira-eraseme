from __future__ import annotations

from enum import StrEnum

import typer

app = typer.Typer(
    name="openeraseme",
    help="Automated data broker removal tool",
    no_args_is_help=True,
)

accounts_app = typer.Typer(
    name="accounts",
    help="Manage email accounts (OAuth2 setup, list, remove)",
    no_args_is_help=True,
)
app.add_typer(accounts_app)


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
    from openeraseme.core.identity import profile_exists, save_profile
    from openeraseme.registry.schema import IdentityProfile

    profile = IdentityProfile(full_name=full_name, email_addresses=[email])
    path = save_profile(profile)
    action = "Updated" if profile_exists() else "Created"
    typer.echo(f"{action} encrypted identity profile at {path}")


@app.command()
def show_profile() -> None:
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


@app.command()
def render_template(
    template: str = typer.Argument(help="Template name (e.g. gdpr-art17.de.md.j2)"),
    broker_name: str = typer.Option("", help="Name of the data broker"),
    broker_website: str = typer.Option("", help="Broker website URL"),
) -> None:
    from openeraseme.core.identity import load_profile, profile_exists
    from openeraseme.core.templating import render_template as _render

    profile = load_profile() if profile_exists() else None
    result = _render(
        template,
        profile=profile,
        broker_name=broker_name,
        broker_website=broker_website,
    )
    typer.echo(result)


@accounts_app.command()
def add(
    provider: str = typer.Argument(help="Provider: gmail or outlook"),
    email: str = typer.Option(..., prompt=True, help="Email address"),
    client_id: str = typer.Option(..., prompt=True, help="OAuth2 client ID"),
    client_secret: str = typer.Option(
        ..., prompt=True, hide_input=True, help="OAuth2 client secret"
    ),
) -> None:
    from openeraseme.adapters.email.oauth2 import (
        _redirect_uri,
        _save_account_index,
        authorize_url,
        exchange_code,
        run_local_server,
        save_client_credentials,
        save_refresh_token,
    )

    save_client_credentials(email, client_id, client_secret)
    url = authorize_url(provider, client_id, _redirect_uri)
    typer.echo(f"Opening browser for OAuth2 authorization: {url}")
    import webbrowser

    webbrowser.open(url)

    typer.echo("Waiting for authorization callback on http://localhost:8899 ...")
    try:
        code = run_local_server()
    except TimeoutError:
        typer.echo(
            "Timed out waiting for authorization. "
            "You can also paste the code from the redirect URL."
        )
        code = typer.prompt("Authorization code")
    token_data = exchange_code(provider, code, client_id, client_secret, _redirect_uri)
    refresh_token = token_data.get("refresh_token", "")
    if refresh_token:
        save_refresh_token(email, refresh_token)
    _save_account_index(email, provider)
    typer.echo(f"Account {email} ({provider}) configured successfully.")


@accounts_app.command()
def list_cmd() -> None:
    from openeraseme.adapters.email.oauth2 import list_accounts

    accounts = list_accounts()
    if not accounts:
        typer.echo("No accounts configured.")
        return
    for acc in accounts:
        typer.echo(f"  {acc['email']} ({acc['provider']})")


@accounts_app.command()
def remove(
    email: str = typer.Argument(help="Email address to remove"),
) -> None:
    from openeraseme.adapters.email.oauth2 import _remove_from_index, delete_account

    delete_account(email)
    _remove_from_index(email)
    typer.echo(f"Account {email} removed.")


# ---------------------------------------------------------------------------
# plan, plan show
# ---------------------------------------------------------------------------

plan_app = typer.Typer(
    name="plan",
    help="Plan a removal campaign (scan registry, create events)",
    no_args_is_help=True,
)
app.add_typer(plan_app)


@plan_app.command()
def create(
    ctx: typer.Context,
    campaign_id: str = typer.Option(
        ..., "--campaign", help="Campaign identifier (e.g. initial-2026-Q2)"
    ),
    jurisdiction: str = typer.Option(None, help="Filter by jurisdiction (e.g. DE, US)"),
    priority: str = typer.Option(None, help="Filter by priority (high, medium, low)"),
    max_brokers: int = typer.Option(30, "--max", help="Maximum brokers to plan"),
) -> None:
    from openeraseme.core.db import init_db
    from openeraseme.core.orchestrator import plan_campaign

    init_db()
    result = plan_campaign(
        campaign_id=campaign_id,
        jurisdiction=jurisdiction,
        priority=priority,
        max_brokers=max_brokers,
    )

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(_json.dumps(result, indent=2, default=str))
        return

    typer.echo(f"Campaign: {result['campaign_id']}")
    typer.echo(f"  Total brokers scanned: {result['total_brokers']}")
    typer.echo(f"  Planned requests: {result['planned']}")
    for r in result["requests"]:
        typer.echo(f"    #{r['request_id']} {r['broker_name']} ({r['channel']})")


@plan_app.command(name="show")
def plan_show(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
) -> None:
    from openeraseme.core.db import init_db
    from openeraseme.core.orchestrator import get_plan

    init_db()
    result = get_plan(campaign_id=campaign_id, status=status)

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(_json.dumps(result, indent=2, default=str))
        return

    typer.echo(f"Plan: {result['campaign_id']} ({result['total']} requests)")
    for r in result["requests"]:
        status_str = r.get("current_status", "PLANNED")
        typer.echo(f"  #{r['id']} [{status_str}] {r.get('broker_id', '?')}")


# ---------------------------------------------------------------------------
# execute
# ---------------------------------------------------------------------------


@app.command()
def execute(
    ctx: typer.Context,
    campaign_id: str = typer.Option(..., "--campaign", help="Campaign to execute"),
    account: str = typer.Option(None, "--account", help="Himalaya account name"),
    batch_size: int = typer.Option(5, "--batch-size", help="Number to send"),
    dry_run: bool = typer.Option(False, "--dry-run", help="Simulate only"),
    yes: bool = typer.Option(False, "--yes", help="Skip consent prompt (destructive)"),
    consent_token: str = typer.Option(None, "--consent", help="Pre-issued consent token"),
) -> None:
    from openeraseme.core.consent import check_consent
    from openeraseme.core.db import init_db
    from openeraseme.core.orchestrator import execute_campaign

    if not dry_run and not check_consent("execute", yes=yes, consent_token=consent_token):
        typer.echo(
            "Error: Destructive command requires consent. "
            "Use --yes or issue a token via 'grant' command.",
            err=True,
        )
        raise typer.Exit(1)

    init_db()
    result = execute_campaign(
        campaign_id,
        account=account,
        batch_size=batch_size,
        dry_run=dry_run,
    )

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(_json.dumps(result, indent=2, default=str))
        return

    for r in result["results"]:
        status = "OK" if r["success"] else "FAIL"
        extra = r.get("dry_run", False) and " (dry-run)" or ""
        typer.echo(f"  #{r['request_id']} {status}{extra}")
        if not r["success"]:
            typer.echo(f"    Error: {r.get('error', 'unknown')}")


# ---------------------------------------------------------------------------
# grant — issue consent token
# ---------------------------------------------------------------------------


@app.command()
def grant(
    command: str = typer.Argument("execute", help="Command to authorize (e.g. execute)"),
    ttl: int = typer.Option(86400, "--ttl", help="Token TTL in seconds"),
) -> None:
    from openeraseme.core.consent import issue_token

    token = issue_token(command, ttl=ttl)
    typer.echo(f"Consent token: {token}")
    typer.echo(f"  Command: {command}")
    typer.echo(f"  TTL: {ttl}s")
    typer.echo("")
    typer.echo(f"Use: OPENERASEME_CONSENT={token} openeraseme {command} ...")
    typer.echo(f"Or:  openeraseme {command} ... --consent {token}")


# ---------------------------------------------------------------------------
# events
# ---------------------------------------------------------------------------

events_app = typer.Typer(
    name="events",
    help="View removal request event history",
    no_args_is_help=True,
)
app.add_typer(events_app)


@events_app.command(name="show")
def events_show(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID"),
) -> None:
    from openeraseme.core.db import init_db
    from openeraseme.core.events import get_events

    init_db()
    events = get_events(request_id)

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(_json.dumps(events, indent=2, default=str))
        return

    if not events:
        typer.echo(f"No events found for request #{request_id}")
        return

    typer.echo(f"Events for request #{request_id}:")
    for e in events:
        typer.echo(f"  #{e['id']} {e['event_type']} @ {e['occurred_at']} (source: {e['source']})")


# ---------------------------------------------------------------------------
# requests
# ---------------------------------------------------------------------------

requests_app = typer.Typer(
    name="requests",
    help="List and manage removal requests",
    no_args_is_help=True,
)
app.add_typer(requests_app)


@requests_app.command(name="list")
def requests_list(
    ctx: typer.Context,
    campaign_id: str = typer.Option(None, "--campaign", help="Filter by campaign"),
    status: str = typer.Option(None, "--status", help="Filter by status"),
    broker_id: str = typer.Option(None, "--broker", help="Filter by broker ID"),
) -> None:
    from openeraseme.core.db import init_db
    from openeraseme.core.events import list_removal_requests

    init_db()
    requests = list_removal_requests(
        campaign_id=campaign_id,
        status=status,
        broker_id=broker_id,
    )

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(_json.dumps(requests, indent=2, default=str))
        return

    if not requests:
        typer.echo("No requests found.")
        return

    for r in requests:
        typer.echo(
            f"  #{r['id']} [{r.get('current_status', 'N/A')}] {r['broker_id']} ({r['campaign_id']})"
        )


# ---------------------------------------------------------------------------
# poll-inbox
# ---------------------------------------------------------------------------


@app.command(name="poll-inbox")
def poll_inbox(
    ctx: typer.Context,
    host: str = typer.Option("imap.gmail.com", "--host", help="IMAP server"),
    port: int = typer.Option(993, "--port", help="IMAP port"),
    username: str = typer.Option(..., "--username", prompt=True, help="IMAP username"),
    password: str = typer.Option(
        ..., "--password", prompt=True, hide_input=True, help="IMAP password"
    ),
    since_days: int = typer.Option(1, "--since", help="Look back N days"),
    ssl: bool = typer.Option(True, "--ssl/--no-ssl"),
    campaign_id: str = typer.Option(None, "--campaign", help="Campaign to match replies against"),
) -> None:
    from openeraseme.adapters.email.smtp_imap import (
        IMAPError,
        match_reply_to_request,
    )
    from openeraseme.adapters.email.smtp_imap import (
        poll_inbox as _poll,
    )
    from openeraseme.core.db import init_db
    from openeraseme.core.events import list_removal_requests
    from openeraseme.core.orchestrator import submit_inbox_reply

    init_db()

    try:
        messages = _poll(
            host=host,
            port=port,
            username=username,
            password=password,
            ssl=ssl,
            since_days=since_days,
        )
    except IMAPError as e:
        typer.echo(f"IMAP error: {e}", err=True)
        raise typer.Exit(1) from e

    # Match against existing removal requests
    if messages:
        requests = list_removal_requests(campaign_id=campaign_id)
        matched = match_reply_to_request(messages, requests)

        for msg in matched:
            submit_inbox_reply(
                msg.get("message_id", ""),
                request_id=msg.get("request_id"),
                from_addr=msg.get("from_addr", ""),
                subject=msg.get("subject", ""),
                snippet=msg.get("body", "")[:200],
            )
    else:
        matched = []

    if ctx.obj.get("output") == "json":
        import json as _json

        output = {
            "total_fetched": len(messages),
            "total_matched": sum(1 for m in matched if m.get("request_id") is not None),
            "messages": matched,
        }
        typer.echo(_json.dumps(output, indent=2, default=str))
        return

    typer.echo(f"Fetched {len(messages)} messages from inbox")
    matched_count = sum(1 for m in matched if m.get("request_id") is not None)
    typer.echo(f"Matched to requests: {matched_count}")
    for m in matched:
        req_id = m.get("request_id", "unmatched")
        typer.echo(f"  [{req_id}] {m.get('subject', '(no subject)')}")

    if not messages:
        typer.echo("No new messages found.")


# ---------------------------------------------------------------------------
# classify-reply
# ---------------------------------------------------------------------------


@app.command(name="classify-reply")
def classify_reply(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID to classify the reply for"),
    api_key: str = typer.Option(
        None, "--api-key", envvar="ANTHROPIC_API_KEY", help="Anthropic API key"
    ),
    model: str = typer.Option("claude-3-5-sonnet-latest", "--model", help="Claude model name"),
    save: bool = typer.Option(True, "--save/--no-save", help="Save classification result to DB"),
) -> None:
    """Classify a broker reply using the LLM classifier."""
    from openeraseme.adapters.triage.classifier import ReplyClassifier
    from openeraseme.core.db import init_db
    from openeraseme.core.events import append_event, get_events, get_removal_request
    from openeraseme.core.orchestrator import submit_inbox_reply
    from openeraseme.core.projection import upsert_state
    from openeraseme.registry.loader import load_broker
    from openeraseme.registry.schema import Broker

    init_db()

    req = get_removal_request(request_id)
    if req is None:
        typer.echo(f"Request #{request_id} not found.", err=True)
        raise typer.Exit(1)

    events = get_events(request_id)
    if not events:
        typer.echo(f"No events found for request #{request_id}.", err=True)
        raise typer.Exit(1)

    last_event = events[-1]
    payload = last_event.get("payload_json", {})

    broker_id = req.get("broker_id", "")
    try:
        broker: Broker | None = load_broker(broker_id)
    except Exception:
        broker = None

    broker_name = broker.name if broker else broker_id
    broker_website = broker.website if broker else ""
    original_subject = f"Data Deletion Request — {broker_name}"
    original_snippet = payload.get("template", "")

    # Find the latest unmatched inbox reply for this request
    from openeraseme.core.db import get_connection

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, subject, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? AND classified_as IS NULL "
        "ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    if reply is None:
        typer.echo(f"No unclassified inbox reply found for request #{request_id}.", err=True)
        raise typer.Exit(1)

    classifier = ReplyClassifier(api_key=api_key, model=model)
    if not classifier.is_available():
        typer.echo(
            "Anthropic API is not available. Set ANTHROPIC_API_KEY or provide --api-key.",
            err=True,
        )
        raise typer.Exit(1)

    result = classifier.classify(
        broker_name=broker_name,
        broker_website=broker_website,
        original_subject=original_subject,
        original_request_snippet=original_snippet,
        reply_subject=reply["subject"] or "",
        reply_body=reply["snippet"] or "",
        cache_key=f"broker:{broker_id}",
    )

    if ctx.obj.get("output") == "json":
        import json as _json

        output = {
            "request_id": request_id,
            "reply_id": reply["id"],
            "classification": result.label,
            "event_type": result.event_type,
            "confidence": result.confidence,
            "summary": result.summary,
            "needs_human_review": result.needs_human_review,
            "extracted_fields": result.extracted_fields,
        }
        if result.usage_record:
            output["usage"] = result.usage_record.record()
        typer.echo(_json.dumps(output, indent=2, default=str))
    else:
        typer.echo(f"Classification for request #{request_id}:")
        typer.echo(f"  Label:      {result.label}")
        typer.echo(f"  Event:      {result.event_type}")
        typer.echo(f"  Confidence: {result.confidence:.2f}")
        typer.echo(f"  Summary:    {result.summary}")
        if result.extracted_fields:
            typer.echo(f"  Extracted:  {result.extracted_fields}")
        if result.needs_human_review:
            typer.echo("  *** Needs human review ***")
        if result.usage_record:
            usage = result.usage_record.record()
            typer.echo(
                f"  Cost:       ${usage['cost']:.6f}"
                f" ({usage['input_tokens']} in / {usage['output_tokens']} out)"
            )

    if save:
        submit_inbox_reply(
            message_id=str(reply["id"]),
            request_id=request_id,
            from_addr=reply["from_addr"] or "",
            subject=reply["subject"] or "",
            snippet=reply["snippet"] or "",
            classified_as=result.label,
        )
        append_event(
            request_id,
            result.event_type,
            payload={
                "classification": result.label,
                "confidence": result.confidence,
                "summary": result.summary,
                "extracted_fields": result.extracted_fields,
                "reply_id": reply["id"],
            },
            source="system",
        )
        upsert_state(request_id)
        typer.echo("Classification saved to database.")


# ---------------------------------------------------------------------------
# db
# ---------------------------------------------------------------------------


@app.command()
def db_init() -> None:
    from openeraseme.core.db import init_db

    path = init_db()
    typer.echo(f"Database initialized at {path}")


if __name__ == "__main__":
    app()
