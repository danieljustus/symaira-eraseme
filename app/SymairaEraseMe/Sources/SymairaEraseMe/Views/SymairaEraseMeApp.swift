import SwiftUI
import SymairaTheme

@main
struct SymairaEraseMeApp: App {
    @StateObject private var serverManager = ServerManager()
    @AppStorage("symeraseme_appearance") private var appearance: SymairaAppearance = .system

    var body: some Scene {
        WindowGroup("Symaira EraseMe") {
            ContentView()
                .environmentObject(serverManager)
                .frame(minWidth: 900, minHeight: 600)
                .onAppear { appearance.apply() }
                .onChange(of: appearance) { _, value in value.apply() }
        }
        .symairaWorkspaceWindow()
        .defaultSize(width: 1200, height: 800)

        Settings {
            SettingsView(serverManager: serverManager)
                .environmentObject(serverManager)
                .frame(width: 720, height: 680)
        }
    }
}

/// Root view with sidebar navigation.
struct ContentView: View {
    @EnvironmentObject var serverManager: ServerManager
    @Environment(\.openSettings) private var openSettings
    @State private var selectedNav: NavItem
    @State private var sidebarLayout = SymairaSidebarState()
    @State private var topography = SymairaTopoModel()

    init(initialNavigation: NavItem = .dashboard) {
        _selectedNav = State(initialValue: initialNavigation)
    }

    enum NavItem: String, CaseIterable, Identifiable {
        case dashboard = "Dashboard"
        case campaigns = "Campaigns"
        case requests = "Requests"
        case brokers = "Brokers"
        case calendar = "Calendar"
        case manualTasks = "Manual Tasks"

        var id: String { rawValue }

        // Settings opens a separate native window, never a sidebar destination.
        static var sidebarItems: [Self] { allCases }

        /// Accessibility name surfaced as the sidebar button's label so
        /// VoiceOver users can distinguish the product destinations.
        var accessibilityName: String { rawValue }

        var icon: String {
            switch self {
            case .dashboard: return "chart.bar"
            case .campaigns: return "megaphone"
            case .requests: return "envelope"
            case .brokers: return "person.2"
            case .calendar: return "calendar"
            case .manualTasks: return "checklist"
            }
        }
    }

    var body: some View {
        SymairaWorkspace(
            sidebar: $sidebarLayout,
            topography: selectedNav == .dashboard ? topography : nil,
            navigation: { showsLabels in sidebar(showsLabels: showsLabels) },
            content: {
                detail.font(SymairaTypography.body)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("eraseme-content-\(selectedNav.id)")
            }
        )
        .tint(SymairaTokens.selected)
        .toolbar {
            SymairaWorkspaceToolbar(sidebar: $sidebarLayout) {
                openSettings()
            }
        }
    }

    private func sidebar(showsLabels: Bool) -> some View {
        VStack(alignment: showsLabels ? .leading : .center, spacing: SymairaSpacing.large) {
            HStack(spacing: SymairaSpacing.small) {
                Image(systemName: "shield.checkerboard")
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(SymairaTokens.ink2)
                    .frame(width: 32, height: 36)
                    .accessibilityHidden(true)
                if showsLabels {
                    Text("EraseMe")
                        .symairaText(.heading)
                }
            }
            .padding(.bottom, SymairaSpacing.small)

            ScrollView {
                VStack(spacing: SymairaSpacing.xSmall) {
                    ForEach(NavItem.sidebarItems) { item in
                        SymairaNavigationButton(
                            item.accessibilityName, systemImage: item.icon,
                            isSelected: selectedNav == item, showsLabel: showsLabels
                        ) {
                            selectedNav = item
                        }
                        .accessibilityIdentifier("eraseme-nav-\(item.id)")
                    }
                }
                .padding(3)
            }

            Divider().overlay(SymairaTokens.line)
            // Shared reachability remains authoritative, including external servers.
            if showsLabels {
                SymairaStatusLabel(
                    serverManager.mcpReachable ? "MCP Engine Active" : "MCP Engine Offline",
                    tone: serverManager.mcpReachable ? .positive : .critical
                )
                .padding(.horizontal, SymairaSpacing.small)
            } else {
                SymairaStatusDot(
                    tone: serverManager.mcpReachable ? .positive : .critical,
                    accessibilityLabel: serverManager.mcpReachable ? "MCP Engine Active" : "MCP Engine Offline"
                )
            }
        }
        .padding(SymairaSpacing.medium)
        .frame(maxHeight: .infinity)
    }

    @ViewBuilder
    private var detail: some View {
        switch selectedNav {
        case .dashboard:
            DashboardView()
        case .campaigns:
            CampaignsView()
        case .requests:
            RequestsView()
        case .brokers:
            BrokersView()
        case .calendar:
            CalendarView()
        case .manualTasks:
            ManualTasksView()
        }
    }
}
