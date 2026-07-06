import SwiftUI

@main
struct SymairaEraseMeApp: App {
    @StateObject private var serverManager = ServerManager()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(serverManager)
                .frame(minWidth: 900, minHeight: 600)
                .background(BrandColors.bgDark.ignoresSafeArea())
        }
        .windowStyle(.titleBar)
        .defaultSize(width: 1200, height: 800)
    }
}

/// Root view with sidebar navigation.
struct ContentView: View {
    @EnvironmentObject var serverManager: ServerManager
    @State private var selectedNav: NavItem = .dashboard

    enum NavItem: String, CaseIterable, Identifiable {
        case dashboard = "Dashboard"
        case campaigns = "Campaigns"
        case requests = "Requests"
        case brokers = "Brokers"
        case calendar = "Calendar"
        case manualTasks = "Manual Tasks"
        case settings = "Settings"

        var id: String { rawValue }

        var icon: String {
            switch self {
            case .dashboard: return "chart.bar.fill"
            case .campaigns: return "megaphone.fill"
            case .requests: return "envelope.fill"
            case .brokers: return "person.2.fill"
            case .calendar: return "calendar"
            case .manualTasks: return "checklist"
            case .settings: return "gearshape.fill"
            }
        }
    }

    var body: some View {
        HStack(spacing: 0) {
            sidebar
                .frame(width: 230)
            
            Divider()
                .background(Color.white.opacity(0.08))
            
            ZStack {
                BlueprintBackground()
                
                detail
                    .transition(.opacity.combined(with: .move(edge: .trailing)))
            }
        }
        .background(BrandColors.bgDark.ignoresSafeArea())
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header Logo
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 10) {
                    Image(systemName: "shield.checkerboard")
                        .font(.title2)
                        .foregroundStyle(BrandColors.goldPrimary)
                    
                    Text("SYMAIRA")
                        .font(.system(size: 20, weight: .bold))
                        .foregroundStyle(BrandColors.textPrimary)
                        .kerning(2.0)
                }
                Text("EraseMe Portal")
                    .font(.caption2)
                    .foregroundStyle(BrandColors.textMuted)
                    .kerning(1.0)
                    .padding(.leading, 30)
            }
            .padding(.horizontal, 20)
            .padding(.top, 32)
            .padding(.bottom, 36)

            // Navigation list
            ScrollView {
                VStack(spacing: 6) {
                    ForEach(NavItem.allCases) { item in
                        SidebarButton(item: item, isSelected: selectedNav == item) {
                            withAnimation(.spring(response: 0.25, dampingFraction: 0.85)) {
                                selectedNav = item
                            }
                        }
                    }
                }
                .padding(.horizontal, 10)
            }
            
            Spacer()
            
            // Server Quick Status in Footer
            HStack(spacing: 8) {
                Circle()
                    .fill(serverManager.isRunning ? BrandColors.confirmed : BrandColors.rejected)
                    .frame(width: 8, height: 8)
                Text(serverManager.isRunning ? "MCP Engine Active" : "MCP Engine Offline")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(BrandColors.textSecondary)
            }
            .padding(18)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.black.opacity(0.2))
        }
        .frame(maxHeight: .infinity)
        .background(BrandColors.bgDarker.ignoresSafeArea())
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
        case .settings:
            SettingsView(serverManager: serverManager)
        }
    }
}

struct SidebarButton: View {
    let item: ContentView.NavItem
    let isSelected: Bool
    let action: () -> Void
    
    @State private var isHovered = false
    
    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                // Left indicator bar
                RoundedRectangle(cornerRadius: 2)
                    .fill(isSelected ? BrandColors.goldPrimary : (isHovered ? BrandColors.goldPrimary.opacity(0.4) : Color.clear))
                    .frame(width: 3, height: 16)
                
                Image(systemName: item.icon)
                    .font(.body)
                    .foregroundStyle(isSelected ? BrandColors.goldPrimary : (isHovered ? BrandColors.textPrimary : BrandColors.textSecondary))
                    .frame(width: 20, alignment: .center)
                
                Text(item.rawValue)
                    .font(.subheadline)
                    .fontWeight(isSelected ? .semibold : .medium)
                    .foregroundStyle(isSelected ? BrandColors.textPrimary : (isHovered ? BrandColors.textPrimary : BrandColors.textSecondary))
                
                Spacer()
            }
            .padding(.vertical, 8)
            .padding(.trailing, 10)
            .contentShape(Rectangle())
            .background(
                RoundedRectangle(cornerRadius: 6)
                    .fill(isSelected ? Color.white.opacity(0.04) : (isHovered ? Color.white.opacity(0.02) : Color.clear))
            )
        }
        .buttonStyle(.plain)
        .onHover { hovering in
            withAnimation(.easeOut(duration: 0.15)) {
                isHovered = hovering
            }
        }
    }
}

