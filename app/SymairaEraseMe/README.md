# SymairaEraseMe

Native SwiftUI macOS app for the Symaira EraseMe dashboard. Connects to the
self-contained Go MCP JSON-RPC server (`symeraseme mcp`) over HTTP.

## Requirements

- macOS 14+ (Sonoma)
- Swift 5.10+ with Xcode or Xcode-beta installed (SwiftUI macro plugins required)
- Go 1.26+ for development builds (the release app bundles the Go server)

## Build

```bash
# Using the build script (builds Swift + Go and colocates both binaries)
./build.sh

# Or manually with Xcode (then build the Go server next to the Swift binary)
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift build
GO_BIN="$(swift build --show-bin-path)/symeraseme"
(cd ../.. && CGO_ENABLED=0 go build -trimpath -o "$GO_BIN" ./cmd/symeraseme)

# Or open in Xcode
open Package.swift
```

## Run

```bash
# From the project directory (after ./build.sh)
"$(swift build --show-bin-path)/SymairaEraseMe"

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
│   └── ServerManager.swift  Bundled/Dev/Homebrew Go server process manager
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
    ├── BrandColors.swift           Color tokens matching HTML dashboard
    └── Glassmorphism.swift         Glass cards, badges, stat cards, error banners
```

## How It Works

1. The app starts and shows the sidebar navigation.
2. Go to **Settings** and click **Start Server** to spawn the bundled `symeraseme mcp` binary.
3. The app connects to `http://127.0.0.1:8000` via JSON-RPC 2.0.
4. Each view fetches data from the appropriate MCP tool.
5. The app parses `result.content[0].text` → JSON → Swift models.

## Brand Colors

| Token | Hex | Usage |
|-------|-----|-------|
| bgDark | `#0D0C0A` | Main background |
| goldPrimary | `#E5C397` | Accents, links, buttons |
| confirmed | `#A7F3D0` | Confirmed status |
| pending | `#FDE68A` | Sent/awaiting status |
| rejected | `#FCA5A5` | Rejected status |
| overdue | `#FECACA` | Overdue status |
| planned | `#DBEAFE` | Planned status |

## Limitations

- SwiftUI apps require Xcode (or Xcode-beta) for the macro plugins that power
  `@State`, `@StateObject`, `@Binding`, etc. Building with plain `swift build`
  from CommandLineTools alone will fail.
- Release bundles include the Go MCP server and need no Python installation or
  external runtime. Development builds use the sibling Go binary produced by
  `build.sh`; a Homebrew `symeraseme` or configured Binary Path is supported as
  a fallback.
- No external Swift dependencies beyond the Symaira AppKit packages declared
  in `Package.swift`.
