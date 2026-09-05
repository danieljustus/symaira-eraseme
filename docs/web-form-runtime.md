# Web-form runtime boundary

EraseMe keeps web-form execution behind a local `FormExecutor` interface. The
production Go-to-Rust slice currently provides a durable manual-task fallback,
not a browser process driver.

This is deliberate. `symbrowse` does not expose a stable, externally consumable
atomic formflow command, and calling its MCP server from the EraseMe MCP server
would create a nested protocol/session boundary. The accepted Rust migration
proposal also places new browser automation outside this slice. EraseMe must
not compile against `symaira-browse`, discover a sibling checkout, or invent a
private daemon protocol.

## Behavior

- `run_web_form` with `dry_run=true` only validates and previews the registry
  form. It makes no browser or subprocess call.
- Non-dry `run_web_form` without an injected executor creates a pending
  `manual_tasks` row with `reason=dynamic_form`, `success=false`, and queue
  `status=pending`; the result envelope carries `status=manual_action_required`.
- Campaign `execute` projects `HUMAN_ACTION_REQUIRED` before `SEND_FAILED` and
  returns the task identifier. It never emits `SENT` for the fallback path.
- `auto_confirm` still filters reply links to known broker/sender domains. With
  no clicker it creates a linked manual task and records a note; it never
  emits `CONFIRMATION_LINK_CLICKED` or reports success.
- Injected executors remain bounded by the caller context and their result,
  reason, URL, and evidence are mapped without claiming success on failure.

A future runtime driver may use a stable `symbrowse --json` contract only after
that contract is formally owned and tested. Until then, do not pass field
values or authentication material to a child process. In particular, values
in a hypothetical `fill` CLI would appear in child argv; authentication
secrets must never use that path.
