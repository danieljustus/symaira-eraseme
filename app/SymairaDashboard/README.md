# SymairaDashboard

Native SwiftUI macOS app for the Symaira EraseMe dashboard. Connects to the
Python MCP JSON-RPC server (`symeraseme serve`) over HTTP.

## Requirements

- macOS 14+ (Sonoma)
- Swift 5.10+ with Xcode or Xcode-beta installed (SwiftUI macro plugins required)
- Python `symeraseme` package installed (`pip install symeraseme`)

## Build

```bash
# Using the build script (auto-detects Xcode)
./build.sh

# Or manually with Xcode
DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer swift build

# Or open in Xcode
open Package.swift
```

## Run

```bash
# From the project directory
swift run SymairaDashboard

# Or from the built binary
.build/debug/SymairaDashboard
```

## Architecture

```
Sources/SymairaDashboard/
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
│   └── ServerManager.swift  Process spawner for `symeraseme serve`
├── ViewModels/     @MainActor ObservableObject view models
│   ├── DashboardViewModel.swift
│   ├── CampaignsViewModel.swift
│   ├── RequestsViewModel.swift
│   ├── BrokersViewModel.swift
│   ├── CalendarViewModel.swift
│   ├── ManualTasksViewModel.swift
│   └── SettingsViewModel.swift
├── Views/          SwiftUI views
│   ├── SymairaDashboardApp.swift   App entry + sidebar navigation
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
2. Go to **Settings** and click **Start Server** to spawn `symeraseme serve`.
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
- The app does not bundle the Python server — it spawns `symeraseme serve` as a
  subprocess. Ensure `symeraseme` is installed and on PATH, or configure the
  binary path in Settings.
- No external dependencies — uses only Apple frameworks (SwiftUI, Foundation,
  Swift Charts, AppKit).
