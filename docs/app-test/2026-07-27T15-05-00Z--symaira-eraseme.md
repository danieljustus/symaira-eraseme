<!-- apptest: timestamp=2026-07-27T15:05:00Z  repo=danieljustus/symaira-eraseme  head=b7dce45 -->
<!-- build: app=SymairaEraseMe  version=1.0.0 (1)  source=installed  path=/Applications/SymairaEraseMe.app  driver=computer-use -->

# App Test — Symaira EraseMe 1.0.0 — 2026-07-27

## Session

| Field | Value |
|---|---|
| App | SymairaEraseMe (com.symaira.eraseme.app) |
| Build tested | 1.0.0 (1) — installed at `/Applications/SymairaEraseMe.app`, bundle binary dated 2026-07-27 14:12 |
| Corresponds to HEAD | yes — the app sources under `app/SymairaEraseMe/` last changed 2026-07-06 (d727067) and are unmodified in the worktree |
| Driver | computer-use (native macOS), plus `sample`, crash reports and `curl` for passive evidence |
| Environment | production — data: the user's real EraseMe data directory (`~/.symeraseme`); the MCP engine was started read-only from the CLI for part of the run and stopped afterwards |
| Scope | all 7 areas, default depth |
| Areas visited | 7 of 7 |
| Duration | ~25 min |
| Screenshots | `docs/app-test/2026-07-27T15-05-00Z--symaira-eraseme/` (3 evidence files, local only) |

## Verdict

The app does not work. Three independent defects each on their own prevent it from being usable: any MCP
call that *succeeds* aborts the process (SIGABRT, reproduced twice with a crash report), the "Start Server"
button deadlocks the main thread permanently so the engine can never be started from the GUI, and five of
the seven screens call backend tools the shipped MCP server does not expose, so they fail with
`-32601 Method not found` even against a healthy server. The net effect is that no screen in the app has
ever been able to display data: the ones wired to missing tools show a red banner, and the one wired to a
tool that exists (Manual Tasks) crashes the app on arrival. The riskiest untested surface is the
campaign-creation and execute path, which was deliberately not exercised — it writes to the user's real
data and sends real removal requests.

## Findings

- [ ] **[App/BROKEN][urgent] App aborts with SIGABRT whenever an MCP tool call returns a successful result**
  - **Status quo:** Opening Manual Tasks with a reachable MCP engine terminates the app immediately (Abort trap: 6). The crash report faulting thread is `ManualTasksViewModel.refresh()` → `MCPClient.callTool` → `+[NSJSONSerialization dataWithJSONObject:options:error:]` → `objc_exception_throw`. The cause is `MCPClient.swift:47`, which passes `result.raw` — a `[String: AnyCodable]` dictionary of Swift values — straight into `JSONSerialization.data(withJSONObject:)`, which requires JSON-legal Foundation objects and raises an uncatchable Objective-C exception when it does not get them. Because every data-bearing screen goes through `callTool`, this crashes on any tool that actually resolves; the screens wired to non-existent tools are only spared because they fail earlier. Crash report copied to `docs/app-test/2026-07-27T15-05-00Z--symaira-eraseme/03-manual-tasks-crash.ips`.
  - **Repro:** 1. Start the MCP engine so it is reachable on `127.0.0.1:8000` (`symeraseme serve --host 127.0.0.1 --port 8000`). 2. Launch Symaira EraseMe. 3. Click "Manual Tasks" in the sidebar. → The app disappears and macOS writes a crash report; reproduced 2 of 2 attempts. Expected: the manual-task list renders (the same call over HTTP returns 12 pending tasks successfully).
  - **Proposed solution:** Do not hand `AnyCodable` values to `JSONSerialization`. Re-encode the decoded payload with `JSONEncoder` (`AnyCodable` is `Encodable`), or keep the original response `Data` from the transport and decode `T` straight out of it instead of round-tripping through a dictionary. Either way the call must not be able to raise an ObjC exception on a well-formed server response.
  - **Effort/Impact:** Low effort / critical impact — one conversion at a single call site; today it makes every working backend call fatal.

- [ ] **[App/BROKEN][urgent] "Start Server" deadlocks the main thread and freezes the app permanently**
  - **Status quo:** Pressing "Start Server" in Settings leaves the status on "Stopped" with no PID and no error, no `symeraseme serve` process is spawned, and the whole window stops responding — sidebar clicks no longer change the screen and the app must be force-quit. A `sample` of the process shows the main thread parked in `DaemonSupervisor.start(...)` → `DaemonSupervisor.stopInternal()` → `_pthread_mutex_firstfit_lock_wait`: `start()` takes the supervisor's non-recursive `NSLock` and then calls `stopInternal()`, which takes it again. Called from `app/SymairaEraseMe/Sources/SymairaEraseMe/Services/ServerManager.swift:120`; the lock is in the pinned dependency `symaira-appkit 0.2.0`, `Sources/SymairaDaemonKit/DaemonSupervisor.swift`. Sample stack: `docs/app-test/2026-07-27T15-05-00Z--symaira-eraseme/01-settings-start-server-hang-sample.txt`.
  - **Repro:** 1. Launch Symaira EraseMe with no MCP engine running. 2. Go to Settings. 3. Press "Start Server". 4. Wait 5s, then click "Dashboard" in the sidebar. → Status still reads "Stopped", no error is shown, no server process exists, and the sidebar click has no effect — the app is frozen. Expected: the status flips to "Running" with a PID, or an error banner names why it could not start.
  - **Proposed solution:** Make `DaemonSupervisor.start()` not call the locking `stopInternal()` while it already holds the lock — either factor out an unlocked `stopInternalLocked()` or switch the supervisor to a recursive lock. Additionally, `ServerManager.start()` should not run supervisor work synchronously on the main actor, so a future blocking call degrades to a slow start rather than a frozen window.
  - **Effort/Impact:** Low effort / critical impact — the documented way to start the engine from the app is unusable and costs the user their session.

- [ ] **[App/BROKEN] Five of seven screens call MCP tools the shipped server does not expose, so they can never load data**
  - **Status quo:** With a healthy engine on `127.0.0.1:8000`, Dashboard, Campaigns, Requests, Brokers and Calendar all show `JSON-RPC error -32601: Method not found`. The view models request `get_dashboard_data` (Dashboard and Campaigns), `list_requests` and `get_events` (Requests), `list_brokers` (Brokers) and `get_calendar` (Calendar); the server's `tools/list` advertises 22 tools and none of those five are among them (it exposes `plan_create`, `plan_show`, `execute`, `manual_tasks_list`, `generate_dashboard`, `generate_report`, … ). Verified directly: `tools/call` for `manual_tasks_list` returns a result, `tools/call` for `get_dashboard_data` returns `-32601`. Call sites: `ViewModels/DashboardViewModel.swift:17`, `ViewModels/CampaignsViewModel.swift:26`, `ViewModels/RequestsViewModel.swift:41,68`, `ViewModels/BrokersViewModel.swift:43`, `ViewModels/CalendarViewModel.swift:19`. Screenshot: `docs/app-test/2026-07-27T15-05-00Z--symaira-eraseme/02-campaigns-rpc-error.png`.
  - **Repro:** 1. Start the MCP engine on `127.0.0.1:8000`. 2. Launch the app. 3. Visit Dashboard, Campaigns, Requests, Brokers and Calendar in turn. → Every one shows `JSON-RPC error -32601: Method not found` and an empty body. Expected: each screen renders the corresponding data, as the CLI does for the same account.
  - **Proposed solution:** Reconcile the app's tool names against the server's actual tool surface and pin that contract — either add the read-only aggregation tools the app expects to the MCP server, or rewrite the view models onto the tools that exist. Whichever direction is chosen, add a check that fails CI when a tool name referenced in the Swift sources is absent from the server's `tools/list` (the repo already runs an MCP schema-sync check that can carry this).
  - **Effort/Impact:** Medium effort / critical impact — without it the app has no working screen at all.

- [ ] **[App/BUG] Failed loads render as a confident empty state, so "no data" and "the request failed" look identical**
  - **Status quo:** When the backend call fails, each screen still draws its zero state as fact: the Dashboard shows TOTAL/IN PROGRESS/CONFIRMED/REJECTED/OVERDUE all as `0`, Brokers shows "0 brokers in registry" and "No Brokers Found — Adjust filters or check the broker registry", Requests shows "0 total requests", Campaigns shows "No campaigns yet". All of these are false: the account has 12 pending manual tasks and the registry ships 1,200+ brokers. The error banner is present, but it sits above a screen that otherwise reads as a healthy empty account, and the Brokers copy actively misdirects the user to their filters. Same for the counters on Manual Tasks.
  - **Repro:** 1. Leave the MCP engine stopped (or start it, either failure mode works). 2. Launch the app and open Brokers. → "0 brokers in registry" and "No Brokers Found — Adjust filters or check the broker registry", alongside a red error banner. Expected: when the load failed, the counters and the empty-state copy are suppressed or replaced by an explicit error state that says the data could not be loaded and offers a retry.
  - **Proposed solution:** Give each view model three distinct states — loading, loaded (possibly empty), failed — and render the zero counters and the "No …" copy only in the loaded state. In the failed state show the reason plus a retry action instead.
  - **Effort/Impact:** Low effort / high impact — today a user cannot tell a broken backend from a finished job, which for a removal tool is the difference between "nothing to do" and "nothing happened".

- [ ] **[App/A11Y] Escape does not dismiss the New Campaign sheet**
  - **Status quo:** The New Campaign modal (`Views/CampaignsView.swift:43`, presented via `.sheet`) ignores the Escape key; only the "Cancel" button closes it. Pressed three times across two focus positions (with focus in the Campaign ID field and with focus on the sheet), the sheet stayed open every time. Escape-to-cancel is the standard macOS dismissal for a modal and is the only pointer-free way out of this one.
  - **Repro:** 1. Open the app and go to Campaigns. 2. Press "New Campaign". 3. Press Escape. → The sheet stays open. 4. Press "Cancel". → The sheet closes. Expected: Escape dismisses the sheet exactly as Cancel does.
  - **Proposed solution:** Bind Escape to the same dismissal path as Cancel — give the Cancel button `.keyboardShortcut(.cancelAction)` so it becomes the sheet's cancel action.
  - **Effort/Impact:** Low effort / medium impact — one modifier; without it keyboard users have no way out of the only creation flow in the app.

- [ ] **[App/UX] Sidebar reports "MCP Engine Offline" while the app is talking to a running engine**
  - **Status quo:** The sidebar footer indicator is driven by `serverManager.isRunning` (`Views/SymairaEraseMeApp.swift:109-113`), which only tracks a subprocess this app spawned itself. With the engine started outside the app and answering requests on `127.0.0.1:8000`, the footer still shows a red dot and "MCP Engine Offline" on every screen — while those same screens are round-tripping to that engine. The Settings screen has a separate, correct reachability check ("Connected"/"Not connected"), so the app contradicts itself.
  - **Repro:** 1. Start the engine outside the app (`symeraseme serve --host 127.0.0.1 --port 8000`). 2. Launch the app. 3. Look at the sidebar footer, then open Settings and press "Test Connection". → Footer: "MCP Engine Offline". Settings: a live connection result for the same engine. Expected: one consistent indicator, based on reachability rather than on who owns the process.
  - **Proposed solution:** Drive the footer indicator from the same reachability check Settings uses (`MCPClient.ping()`), and use the spawned-process state only to decide whether "Stop Server" is offered.
  - **Effort/Impact:** Low effort / medium impact — the one always-visible health signal in the app is wrong in the normal setup where the engine runs outside it.

- [ ] **[App/UX] The port field renders as "8.000", which does not read as a port number**
  - **Status quo:** Settings → Connection formats the port with `TextField("Port", value: $serverManager.port, format: .number)` (`Views/SettingsView.swift:104`), so the locale's grouping separator is applied to it. On a German-locale Mac the default port 8000 is displayed as "8.000" and 8080 as "8.080". The value round-trips correctly — typing `8080` stores `8080` — but the field reads as a decimal number rather than the port that the error line right below it prints as `8000`.
  - **Repro:** 1. Open Settings on a Mac whose region uses `.` as a grouping separator (e.g. Germany). 2. Look at the port field next to the host field. → It shows "8.000" while the status line below reads "Server not reachable at 127.0.0.1:8000". Expected: "8000", matching how the port is written everywhere else.
  - **Proposed solution:** Format the port without grouping — `format: .number.grouping(.never)`, or bind a plain `String` and validate it as a port on commit.
  - **Effort/Impact:** Low effort / low impact — cosmetic, but it sits in the one field a user has to edit to point the app at a non-default engine.

## Already tracked

<The repository has no open issues, so nothing here is a duplicate.>

## Observed but not reproducible

<Nothing. Both crashes and the deadlock reproduced on every attempt.>

## Considered and dropped

- The About section shows no version or build number — a missing detail, not a defect; the window and bundle identify the app correctly.
- The app bundle carries no code-signing entitlements and is unsigned — a packaging concern, not something the running app got wrong, and out of scope for a UI test.
- The error banner text is the raw JSON-RPC error string rather than a user-facing sentence — subsumed by the empty-state finding above; fixing the state machine is the change that matters.
- "Jurisdiction", "Law" and "Priority" in the New Campaign sheet are free-text fields with no list of accepted values — real, but the creation path could not be exercised end to end (see "Not covered"), so there is no evidence yet about how the backend treats a bad value.
- Layout, spacing and colour of the dark theme — no functional or clarity impact.

## Not covered

- **Campaign creation and execute** — "Create Campaign" writes into the user's real `~/.symeraseme` data directory and "Execute" sends actual removal requests to brokers. Both are outward-facing or data-writing and were not pressed; the New Campaign sheet was opened and cancelled only.
- **Manual task completion** — `manual_tasks_complete` mutates real records; not exercised. Manual Tasks could not be viewed at all in any case (finding 1).
- **Settings → "Generate & Open Dashboard"** — calls `generate_dashboard`, a tool the server does expose, so it is expected to hit the same abort as finding 1; not pressed, to avoid a further forced restart.
- **API key storage** — `ServerManager` persists the Anthropic API key to `UserDefaults` in plaintext (`Services/ServerManager.swift:24-26`) rather than the Keychain, while the CLI side of the project uses `keyring`. Testing this would have required typing a real credential into the app, which was out of bounds for this run; it is worth confirming in a source review.
- **Full-data and performance passes** — no screen ever rendered a populated list, so list behaviour at scale, sorting, filtering, scrolling and cold-start timing could not be assessed.
- **Keyboard-only and dark/light passes beyond the sheet** — only the modal dismissal was checked; the accessibility tree could not be enumerated through `System Events` during this run, so a11y coverage is limited to what was observable by keyboard and screenshot.
