# SymairaEraseMe

Native SwiftUI macOS app for the Symaira EraseMe dashboard. Connects to the
self-contained Rust MCP JSON-RPC server (`symeraseme mcp`) over HTTP.

## Requirements

- macOS 14+ (Sonoma)
- Swift 5.10+ with Xcode or Xcode-beta installed (SwiftUI macro plugins required)
- Rust 1.98.0 for the bundled CLI/MCP server

Go 1.26.6 is not needed to build or run the app. It is retained only for the
repository's temporary Go reference/oracle development checks until the
separate CUT-005 retirement.

## Build

```bash
# Build the Swift app and stage its Rust CLI/MCP server
./build.sh

# Or build the SwiftUI app by itself when using an existing Rust server
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift build

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
    ├── BrandColors.swift           Color tokens matching HTML dashboard
    └── Glassmorphism.swift         Glass cards, badges, stat cards, error banners
```

## How It Works

1. The app starts and shows the sidebar navigation.
2. Go to **Settings** and click **Start Server** to spawn the bundled Rust `symeraseme mcp` binary.
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
- Rust-only release bundles include the Rust MCP server and need no Go, Python,
  or other external runtime. Development builds stage the Rust server through
  `build.sh`; a Homebrew `symeraseme` or configured Binary Path is also supported.
- No external Swift dependencies beyond the Symaira AppKit packages declared
  in `Package.swift`.
