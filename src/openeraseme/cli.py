from __future__ import annotations

import time
from datetime import datetime
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
    ctx: typer.Context,
    command: str = typer.Argument("execute", help="Command to authorize (e.g. execute)"),
    ttl: int = typer.Option(86400, "--ttl", help="Token TTL in seconds"),
    revoke: str = typer.Option(None, "--revoke", help="Revoke a consent token"),
    revoke_all: bool = typer.Option(False, "--revoke-all", help="Revoke all active tokens"),
    list_tokens: bool = typer.Option(False, "--list", help="List active tokens"),
) -> None:
    from openeraseme.core.consent import (
        consume_token,
        issue_token,
        revoke_token,
    )
    from openeraseme.core.consent import (
        list_tokens as _list_tokens,
    )

    if list_tokens:
        tokens = _list_tokens()
        if not tokens:
            typer.echo("No active tokens.")
            return
        for t in tokens:
            typer.echo(
                f"  {t['token']}  cmd={t['command']}  "
                f"expires={datetime.fromtimestamp(t['expires_at']).isoformat()}"
            )
        return

    if revoke:
        if revoke_token(revoke):
            typer.echo(f"Token revoked: {revoke}")
        else:
            typer.echo(f"Token not found: {revoke}", err=True)
            raise typer.Exit(1)
        return

    if revoke_all:
        tokens = _list_tokens()
        if not tokens:
            typer.echo("No active tokens to revoke.")
            return
        for t in tokens:
            consume_token(t["token"])
        typer.echo(f"Revoked {len(tokens)} token(s).")
        return

    token = issue_token(command, ttl=ttl)
    typer.echo(f"Consent token: {token}")
    typer.echo(f"  Command: {command}")
    typer.echo(f"  TTL: {ttl}s")
    typer.echo(f"  Expires: {datetime.fromtimestamp(int(time.time()) + ttl).isoformat()}")
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
# tick — lifecycle engine
# ---------------------------------------------------------------------------


@app.command()
def tick(
    ctx: typer.Context,
    dry_run: bool = typer.Option(False, "--dry-run", help="Show actions without executing"),
) -> None:
    """Run one tick cycle: check deadlines, send reminders, escalate."""
    from openeraseme.core.db import init_db
    from openeraseme.core.deadlines import apply_tick_actions, run_tick

    init_db()

    actions = run_tick(dry_run=dry_run)

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(
            _json.dumps(
                {
                    "total_actions": len(actions),
                    "actions": [a.__dict__ for a in actions],
                },
                indent=2,
                default=str,
            )
        )
        return

    if not actions:
        typer.echo("Tick: no actions needed.")
        return

    typer.echo(f"Tick: {len(actions)} action(s)")
    for a in actions:
        dry_tag = " (DRY RUN)" if a.dry_run else ""
        typer.echo(f"  #{a.request_id} [{a.action_type}] {a.description}{dry_tag}")

    if not dry_run:
        results = apply_tick_actions(actions)
        executed = sum(1 for r in results if r["executed"])
        typer.echo(f"Executed {executed}/{len(results)} actions.")


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
# run-web-form
# ---------------------------------------------------------------------------


@app.command(name="run-web-form")
def run_web_form(
    ctx: typer.Context,
    broker_id: str = typer.Argument(..., help="Broker ID from registry"),
    headed: bool = typer.Option(False, "--headed", help="Show browser window"),
    screenshot_dir: str = typer.Option(
        "", "--screenshots", help="Directory for screenshots"
    ),
) -> None:
    """Run a broker's web form opt-out using Playwright."""
    import asyncio

    from openeraseme.adapters.web.playwright_runner import (
        PlaywrightRunnerError,
    )
    from openeraseme.adapters.web.playwright_runner import (
        run_web_form as _run_form,
    )
    from openeraseme.core.identity import load_profile, profile_exists
    from openeraseme.registry.loader import load_broker

    try:
        broker = load_broker(broker_id)
    except Exception as e:
        typer.echo(f"Broker '{broker_id}' not found: {e}", err=True)
        raise typer.Exit(1) from e

    web_forms = [c for c in broker.opt_out if c.type == "web_form"]
    if not web_forms:
        typer.echo(f"Broker '{broker_id}' has no web form opt-out channel.", err=True)
        raise typer.Exit(1)

    form = web_forms[0]
    url = form.url
    steps_data = [s.model_dump(exclude_none=True) for s in form.form_spec.steps]

    identity_fields: dict[str, str] = {}
    if profile_exists():
        profile = load_profile()

        name_parts = profile.full_name.split(None, 1)
        first_name = name_parts[0] if name_parts else profile.full_name
        last_name = name_parts[1] if len(name_parts) > 1 else ""

        identity_fields = {
            "full_name": profile.full_name,
            "first_name": first_name,
            "last_name": last_name,
            "email": profile.email_addresses[0] if profile.email_addresses else "",
            "phone_number": profile.phone_numbers[0] if profile.phone_numbers else "",
        }
        for i, addr in enumerate(profile.addresses):
            identity_fields[f"address_street_{i}"] = addr.street
            identity_fields[f"address_city_{i}"] = addr.city
            identity_fields[f"address_zip_{i}"] = addr.postal_code
            identity_fields[f"address_state_{i}"] = addr.state if hasattr(addr, "state") else ""
            identity_fields[f"address_country_{i}"] = addr.country

    typer.echo(f"Running web form for {broker.name} ({url})")
    typer.echo(f"Steps: {len(steps_data)}")

    try:
        result = asyncio.run(
            _run_form(
                url=url,
                steps=steps_data,
                headless=not headed,
                timeout_seconds=form.form_spec.timeout_seconds,
                rate_limit_delay=form.form_spec.rate_limit_delay,
                screenshot_dir=screenshot_dir or None,
                identity_fields=identity_fields,
            )
        )
    except PlaywrightRunnerError as e:
        typer.echo(f"Playwright error: {e}", err=True)
        raise typer.Exit(1) from e

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(
            _json.dumps(
                {
                    "broker_id": broker_id,
                    "success": result.success,
                    "step_index": result.step_index,
                    "total_steps": result.total_steps,
                    "error": result.error,
                    "screenshot_path": result.screenshot_path,
                },
                indent=2,
            )
        )
        return

    if result.success:
        typer.echo(f"Web form completed successfully ({result.total_steps} steps).")
    else:
        typer.echo(
            f"Web form failed at step {result.step_index + 1}/{result.total_steps}: "
            f"{result.error}"
        )
        if result.screenshot_path:
            typer.echo(f"Screenshot saved to: {result.screenshot_path}")

        raise typer.Exit(1)


# ---------------------------------------------------------------------------
# auto-confirm
# ---------------------------------------------------------------------------


@app.command(name="auto-confirm")
def auto_confirm_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID to auto-confirm"),
    headed: bool = typer.Option(False, "--headed", help="Show browser window"),
    screenshot_dir: str = typer.Option("", "--screenshots", help="Directory for screenshots"),
    dry_run: bool = typer.Option(False, "--dry-run", help="Simulate without clicking"),
) -> None:
    """Auto-confirm a removal request by clicking verification links via Playwright."""
    import asyncio

    from openeraseme.adapters.web.confirmation_clicker import auto_confirm
    from openeraseme.core.db import init_db
    from openeraseme.core.events import append_event, get_events, get_removal_request
    from openeraseme.core.projection import upsert_state

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
    payload = last_event.get("payload_json", {}) or {}
    reply_body = payload.get("snippet", "") or payload.get("template", "") or ""

    from openeraseme.core.db import get_connection

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    if reply:
        reply_body = reply["snippet"] or reply_body
        from_addr = reply["from_addr"] or ""
    else:
        from_addr = ""

    typer.echo(f"Scanning for confirmation links in reply for request #{request_id}...")

    result = asyncio.run(
        auto_confirm(
            request_id,
            reply_body,
            from_addr=from_addr,
            headless=not headed,
            screenshot_dir=screenshot_dir or None,
            dry_run=dry_run,
        )
    )

    if not dry_run and result.success:
        append_event(
            request_id,
            "CONFIRMATION_LINK_CLICKED",
            payload={
                "url": result.clicked_url,
                "step": result.step,
                "screenshot_before": result.screenshot_before,
                "screenshot_after": result.screenshot_after,
            },
            source="system",
        )
        upsert_state(request_id)
    elif not dry_run and result.error:
        append_event(
            request_id,
            "NOTE_ADDED",
            payload={
                "note": f"Auto-confirm failed: {result.error}",
                "url": result.clicked_url,
            },
            source="system",
        )
        upsert_state(request_id)

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(
            _json.dumps(
                {
                    "request_id": request_id,
                    "success": result.success,
                    "step": result.step,
                    "clicked_url": result.clicked_url,
                    "error": result.error,
                    "dry_run": result.dry_run,
                    "screenshot_before": result.screenshot_before,
                    "screenshot_after": result.screenshot_after,
                },
                indent=2,
                default=str,
            )
        )
        return

    if result.dry_run:
        typer.echo(f"[DRY RUN] Would click: {result.clicked_url}")
        return

    if result.success:
        typer.echo(f"Confirmation link clicked: {result.clicked_url}")
        typer.echo(f"  Step: {result.step}")
        if result.screenshot_before:
            typer.echo(f"  Screenshot before: {result.screenshot_before}")
        if result.screenshot_after:
            typer.echo(f"  Screenshot after: {result.screenshot_after}")
    else:
        typer.echo(f"Failed: {result.error}", err=True)
        if result.clicked_url:
            typer.echo(f"  URL: {result.clicked_url}")
        raise typer.Exit(1)


# ---------------------------------------------------------------------------
# generate-rebuttal
# ---------------------------------------------------------------------------


@app.command(name="generate-rebuttal")
def generate_rebuttal_cmd(
    ctx: typer.Context,
    request_id: int = typer.Argument(..., help="Request ID to generate rebuttal for"),
    api_key: str = typer.Option(
        None, "--api-key", envvar="ANTHROPIC_API_KEY", help="Anthropic API key"
    ),
    model: str = typer.Option("claude-3-5-sonnet-latest", "--model", help="Claude model name"),
    save: bool = typer.Option(True, "--save/--no-save", help="Save rebuttal to DB"),
) -> None:
    """Generate a rebuttal email for a broker rejection reply."""
    from openeraseme.adapters.triage.responder import generate_rebuttal
    from openeraseme.core.db import init_db
    from openeraseme.core.events import (
        append_event,
        get_events,
        get_removal_request,
    )
    from openeraseme.core.identity import load_profile, profile_exists
    from openeraseme.core.projection import upsert_state
    from openeraseme.registry.loader import load_broker

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
        broker = load_broker(broker_id)
    except Exception:
        broker = None

    broker_name = broker.name if broker else broker_id
    broker_website = broker.website if broker else ""

    # Find latest inbox reply for this request
    from openeraseme.core.db import get_connection

    conn = get_connection()
    reply = conn.execute(
        "SELECT id, subject, snippet, from_addr FROM inbox_replies "
        "WHERE request_id = ? ORDER BY received_at DESC LIMIT 1",
        (request_id,),
    ).fetchone()

    broker_message = reply["snippet"] if reply else payload.get("template", "")
    original_request_date = last_event.get("occurred_at", "")

    profile = load_profile() if profile_exists() else None

    result = generate_rebuttal(
        broker_name=broker_name,
        broker_website=broker_website,
        broker_message=broker_message or "",
        original_request_template=payload.get("template", ""),
        original_request_date=original_request_date,
        profile=profile,
        api_key=api_key,
        model=model,
    )

    if ctx.obj.get("output") == "json":
        import json as _json

        output = {
            "request_id": request_id,
            "template_name": result.template_name,
            "label": result.label,
            "jurisdiction": result.jurisdiction,
            "rejection_classification": result.rejection_classification,
            "confidence": result.confidence,
            "needs_human_review": result.needs_human_review,
            "llm_used": result.llm_used,
            "rebuttal_body": result.rebuttal_body,
        }
        if result.usage_record:
            output["usage"] = result.usage_record.record()
        typer.echo(_json.dumps(output, indent=2))
        return

    typer.echo(f"Rebuttal for request #{request_id}:")
    typer.echo(f"  Template:   {result.label}")
    typer.echo(f"  Jurisdiction: {result.jurisdiction}")
    typer.echo(f"  Confidence: {result.confidence:.2f}")
    typer.echo(f"  LLM used:   {result.llm_used}")
    if result.needs_human_review:
        typer.echo("  *** Needs human review ***")
    if result.usage_record:
        usage = result.usage_record.record()
        typer.echo(
            f"  Cost:       ${usage['cost']:.6f}"
            f" ({usage['input_tokens']} in / {usage['output_tokens']} out)"
        )
    typer.echo("")
    typer.echo(result.rebuttal_body)

    if save:
        append_event(
            request_id,
            "REBUTTAL_SENT",
            payload={
                "template_name": result.template_name,
                "rejection_classification": result.rejection_classification,
                "confidence": result.confidence,
                "llm_used": result.llm_used,
                "broker_message_snippet": (broker_message or "")[:200],
            },
            source="system",
        )
        upsert_state(request_id)
        typer.echo("REBUTTAL_SENT event saved to database.")


# ---------------------------------------------------------------------------
# solve-captcha
# ---------------------------------------------------------------------------


@app.command(name="solve-captcha")
def solve_captcha_cmd(
    ctx: typer.Context,
    provider: str = typer.Option(
        "capsolver", "--provider", help="Captcha provider: capsolver or twocaptcha"
    ),
    api_key: str = typer.Option(
        None, "--api-key", envvar="CAPSOLVER_API_KEY", help="API key (or set CAPSOLVER_API_KEY)"
    ),
    site_key: str = typer.Option(..., "--site-key", prompt=True, help="reCAPTCHA site key"),
    page_url: str = typer.Option(
        ..., "--page-url", prompt=True,
        help="Page URL where captcha appears",
    ),
    action: str = typer.Option("verify", "--action", help="reCAPTCHA action"),
) -> None:
    """Solve a captcha using CapSolver or 2Captcha."""
    from openeraseme.adapters.web.captcha_solver import CaptchaError, create_solver

    typer.echo(f"Solving captcha via {provider}...")

    try:
        solver = create_solver(provider, api_key=api_key)
        result = solver.solve_recaptcha_v2(
            site_key=site_key,
            page_url=page_url,
        )
    except CaptchaError as e:
        typer.echo(f"Captcha solving failed: {e}", err=True)
        raise typer.Exit(1) from e

    if ctx.obj.get("output") == "json":
        import json as _json

        typer.echo(
            _json.dumps(
                {
                    "provider": provider,
                    "task_id": result.task_id,
                    "token": result.token,
                },
                indent=2,
            )
        )
        return

    typer.echo(f"Captcha solved (task: {result.task_id})")
    typer.echo(f"Token: {result.token[:50]}...")


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
