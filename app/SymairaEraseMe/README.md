# SymairaEraseMe

Native SwiftUI macOS app for the Symaira EraseMe dashboard. Connects to the
self-contained Rust MCP JSON-RPC server (`symeraseme mcp`) over HTTP in this
source checkout.

This source prepares the Rust-backed app and CLI at version 0.14.1.
Choose a published version and its channel on
[GitHub releases](https://github.com/danieljustus/symaira-eraseme/releases).
The Rust-only layout was introduced in the
[`v0.14.0` canary prerelease](https://github.com/danieljustus/symaira-eraseme/releases/tag/v0.14.0).
Its DMG completed Developer ID signing, notarization, stapling and attestation;
local builds below may be ad-hoc signed. The historical `v0.13.0` prerelease
remains dual-backend and unchanged; `v0.12.1` is the last Go-only stable release.
Canary publication does not complete stable or Go-retirement technical
acceptance. The maintainer authorized Go-source retirement and waived only
the seven-day post-stable observation period on 2026-10-12.

## Requirements

- macOS 14+ (Sonoma)
- Swift 6+ with full Xcode installed (SwiftUI macros and Metal compiler required)
- Rust 1.98.0 for the bundled CLI/MCP server
- Python 3 for the local backend-staging script

Go is not needed to build or run the app or execute the repository's normal
Rust tests. Go implementation and module files have been removed; frozen
observations and immutable source history remain for conformance checks.

## Build

```bash
# Build the Swift app and stage its Rust CLI/MCP server
./build.sh

# Or build the SwiftUI app by itself when using an existing Rust server
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer swift build --build-system native

# Or open in Xcode
open Package.swift
```

## Run

```bash
# From the project directory (after ./build.sh)
"$(swift build --build-system native --show-bin-path)/SymairaEraseMe"

# Or open the generated app bundle from the release packaging script
open .build/dmg-stage/"Symaira EraseMe.app"
```

## Architecture

```
Sources/SymairaEraseMe/
├── Models/         Codable structs matching MCP API response shapes
│   ├── MCPResponse.swift    JSON-RPC 2.0 envelope + AnyCodable
│   ├── Dashboard.swift      DashboardData, BrokerStatus, RecentEvent
│   ├── Request.swift        RemovalRequest, RequestListResponse
│   ├── Event.swift          RequestEvent, EventListResponse
│   ├── Broker.swift         Broker, BrokerOptOut, BrokerListResponse
│   ├── Calendar.swift       CalendarData, TickAction
│   ├── ManualTask.swift     ManualTask
│   └── Profile.swift        IdentityProfile, ExecuteResponse
├── Services/
│   ├── MCPClient.swift      JSON-RPC 2.0 HTTP actor (tools/call, tools/list)
│   └── ServerManager.swift  Bundled/Dev/Homebrew Rust server process manager
├── ViewModels/     @MainActor ObservableObject view models
│   ├── DashboardViewModel.swift
│   ├── CampaignsViewModel.swift
│   ├── RequestsViewModel.swift
│   ├── BrokersViewModel.swift
│   ├── CalendarViewModel.swift
│   ├── ManualTasksViewModel.swift
│   └── SettingsViewModel.swift
├── Views/          SwiftUI views
│   ├── SymairaEraseMeApp.swift   App entry + sidebar navigation
│   ├── DashboardView.swift         Summary cards, chart, tables, grid, timeline
│   ├── CampaignsView.swift         List, create sheet, execute confirmation
│   ├── RequestsView.swift          Paginated list, filters, event detail panel
│   ├── BrokersView.swift           Filterable grid, detail sheet
│   ├── CalendarView.swift          Deadlines summary, tick actions table
│   ├── ManualTasksView.swift       Task list, complete sheet
│   └── SettingsView.swift          Server start/stop, config, HTML fallback
└── Theme/
    ├── BrandColors.swift           Product status aliases for shared adaptive tokens
    └── Glassmorphism.swift         Product status mapping and dashboard stat composition
```

## How It Works

1. The app starts and shows the sidebar navigation.
2. Click the toolbar gear to open the settings window directly, then click **Start Server** to spawn the bundled Rust `symeraseme mcp` binary.
3. The app connects to `http://127.0.0.1:8000` via JSON-RPC 2.0.
4. Each view fetches data from the appropriate MCP tool.
5. The app parses `result.content[0].text` → JSON → Swift models.

## Liquid Topography GUI preview

The native GUI adopts the same shared AppKit workspace as the Cockpit pilot:
resizable/collapsible sidebar, a direct settings-window button, Google Sans Flex,
neutral surfaces and restrained champagne selection. The settings window contains
the shared System/Light/Dark picker alongside the other preferences. Cmd-comma
opens the same native settings window; the workspace keeps its current destination.
Appearance persists under `symeraseme_appearance`; existing preferences and
the `com.symaira.eraseme` bundle identity remain unchanged.

Only Dashboard mounts the decorative topography. Text and cards register
protection regions; dense lists and Settings use a static canvas. Content
cards are opaque shared `symairaCard` surfaces, not local glass copies.
Shared controls honor the AppKit accessibility and older-macOS fallbacks.

The manifest pins the exact AppKit release
[`0.16.0`](https://github.com/danieljustus/symaira-appkit/releases/tag/0.16.0),
including the shared workspace, packaged resources and the `SymairaIcon` set.
Sidebar, empty states, badges, notices and actions all use Symaira icons; no
SF Symbols remain in the sidebar pages. Every page uses the shared `Page`
scaffold (one header, width and state height), filters use
`SymairaFilterMenu` or segments, and champagne marks only selection, active
controls and focus.
A fresh checkout needs no local AppKit worktree or SwiftPM edit mode. Existing
preview workspaces that used edit mode should use a fresh scratch path and
verify `show-dependencies --format json` before building. No public GUI
release, notarization or completed VoiceOver acceptance is implied.

### Shared-component inventory

| Area | Shared implementation |
| --- | --- |
| Native window/toolbar, sidebar resize and collapse | `symairaWorkspaceWindow`, `SymairaWorkspaceToolbar`, `SymairaWorkspace`, `SymairaSidebarState` |
| Six navigation destinations, selected/focus state, icon rail | `SymairaNavigationButton` |
| Direct settings gear; persisted System/Light/Dark inside settings | `SymairaSettingsButton`, `SymairaAppearancePicker`, `SymairaAppearance` |
| Content cards, typography, actions, search and filters | `symairaCard`, `SymairaTypography`, `symairaText`, `symairaButtonStyle`, `SymairaTextFieldStyle`, `SymairaSegmentedControl` |
| Settings and creation forms | `SymairaFormSection`, `SymairaFormRow`, `SymairaFormDivider`, `.symaira` text-field style |
| Status, failures, empty/loading states | `SymairaBadge`, `SymairaStatusLabel`, `SymairaStatusDot`, `SymairaNotice`, `SymairaEmptyState`, `SymairaLoadingState` |
| Dashboard decoration/readability | `SymairaTopoModel`, `topoProtected` |
| Existing backend foundations | `SymairaToolKit`, `SymairaDaemonKit` |

Domain models, request actions, retry wording, chart/stat composition and
status-to-tone mapping remain local. `BrandColors` contains only shared-token
aliases, not another palette. Provider setup, credential storage, updater and
MCP transport migration are separate functional/security changes, not design work.

`scripts/package-dmg.sh` includes SwiftPM font/license and Metal resources.
AppKit resolves the bundle from the packaged app's Resources directory before
SwiftPM's development fallback. Verify a packaged launch with its build-cache
resource bundle unavailable; inspecting resource files alone does not prove
the production font/shader loader uses them.
Set `SWIFT_SCRATCH_PATH`, `STAGE_DIR` and `DIST_DIR` to keep local builds outside
the source tree. A local preview can be assembled from the repository root:

```bash
VERSION=0.14.1-design.3 ./scripts/package-dmg.sh --app-only --non-release-test
```

The build scripts select native SwiftPM while Swift Build incorrectly records
the deployment target as the linked SDK on this toolchain. `vtool -show-build`
must show the real Xcode SDK (27.0 on the verified host), while `minos` stays 14.0.
A successful compile with `sdk 14.0` does not enable modern native toolbar glass.
Remove the deprecated-backend workaround once the SDK metadata is correct with
Swift Build; do not raise the minimum macOS version just to change chrome.

Run `swift test --build-system native` for regressions. `ERASEME_RENDER_DIR=/path/to/evidence` also
exports native renders of the six offline destinations and separate settings
view at 900/1200-point widths in both appearances. These renders contain no real removal data and
do not establish live mouse or screen-reader acceptance. Set
`SYMERASEME_RUST_TEST_APP_BUNDLE` and `SYMERASEME_RUST_TEST_BINARY` to exercise
the actual packaged/backend authentication, tool catalogue and shutdown paths.

## Limitations

- SwiftUI apps require Xcode (or Xcode-beta) for the macro plugins that power
  `@State`, `@StateObject`, `@Binding`, etc. Building with plain `swift build`
  from CommandLineTools alone will fail.
- The current Rust-only source-tree candidate stages its Rust MCP server through
  `build.sh`, which uses Python 3 to locate the compiler output. Running the staged
  Rust backend requires no Go or Python runtime. This does not describe the already-published dual-backend
  `v0.13.0` prerelease assets.
- No external Swift dependencies beyond the Symaira AppKit packages declared
  in `Package.swift`.
