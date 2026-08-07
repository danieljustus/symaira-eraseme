import SwiftUI
import SymairaTheme

/// Calendar view showing upcoming deadlines and tick actions.
struct CalendarView: View {
    @StateObject private var vm = CalendarViewModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                switch vm.state {
                case .idle, .loading:
                    LoadingOverlay(message: "Loading calendar…")
                        .frame(height: 200)

                case .failed(let message):
                    ErrorStateView(message: message) {
                        Task { await vm.refresh() }
                    }
                    .frame(height: 200)

                case .loaded(let data):
                    if let deadlines = data.upcomingDeadlines {
                        deadlinesSummary(deadlines)
                    }
                    tickActionsSection
                    if data.upcomingDeadlines == nil && vm.tickActions.isEmpty {
                        EmptyStateView(
                            icon: "calendar",
                            title: "No Calendar Data",
                            message: "Create a campaign to see upcoming deadlines."
                        )
                        .frame(height: 200)
                    }
                }
            }
            .padding(24)
        }
        .background(Color.clear)
        .task { await vm.refresh() }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Calendar")
                    .symairaText(.display)
                    .foregroundStyle(BrandColors.textPrimary)
                Text("Upcoming deadlines and tick actions")
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.textMuted)
            }
            Spacer()
            Picker("Weeks", selection: $vm.weeks) {
                Text("2 weeks").tag(2)
                Text("4 weeks").tag(4)
                Text("8 weeks").tag(8)
                Text("12 weeks").tag(12)
            }
            .pickerStyle(.segmented)
            .frame(width: 280)
            .onChange(of: vm.weeks) { _, _ in
                Task { await vm.refresh() }
            }
            Button {
                Task { await vm.refresh() }
            } label: {
                Image(systemName: "arrow.clockwise")
                    .symairaText(.heading)
            }
            .buttonStyle(.plain)
            .foregroundStyle(BrandColors.goldPrimary)
            .accessibilityLabel("Refresh calendar")
        }
    }

    /// Human-readable label for an escalation category identifier returned
    /// by the backend (`src/symeraseme/core/reports/data.py` escalation dict:
    /// "none", "reminder", "dpa_pending"). Unknown identifiers fall back to
    /// the raw key so the UI never crashes on a new backend category.
    static func escalationLabel(for key: String) -> String {
        switch key {
        case "dpa_pending":
            return "DPA pending"
        case "reminder":
            return "Reminder"
        case "none", "":
            return "No escalation"
        default:
            return key
        }
    }

    private func deadlinesSummary(_ deadlines: UpcomingDeadlines) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Deadline Summary")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let totals = deadlines.totals {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: 12) {
                    StatCard(title: "Total Requests", value: totals.requests ?? 0, color: BrandColors.textPrimary)
                    StatCard(title: "Resolved", value: totals.resolved ?? 0, color: BrandColors.confirmed)
                    StatCard(title: "Open", value: totals.open ?? 0, color: BrandColors.pending)
                }
            }

            if let upcoming = deadlines.upcoming {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: 12) {
                    StatCard(title: "Overdue", value: upcoming.overdue ?? 0, color: BrandColors.overdue)
                    StatCard(title: "Due in 7d", value: upcoming.deadlineDueWithin7d ?? 0, color: BrandColors.pending)
                    StatCard(title: "Due in 30d", value: upcoming.deadlineDueWithin30d ?? 0, color: BrandColors.planned)
                    StatCard(title: "Tick Actions", value: upcoming.tickActionsReady ?? 0, color: BrandColors.goldPrimary)
                }
            }

            if let byStatus = deadlines.byStatus, !byStatus.isEmpty {
                Text("By Status")
                    .symairaText(.bodyEmphasized)
                    .foregroundStyle(BrandColors.textSecondary)
                    .padding(.top, 4)
                HStack(spacing: 16) {
                    ForEach(byStatus.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                        VStack(spacing: 4) {
                            Text("\(value.plainDigits)")
                                .symairaText(.heading)
                                .foregroundStyle(BrandColors.color(for: key))
                            Text(key.replacingOccurrences(of: "_", with: " "))
                                .symairaText(.caption)
                                .foregroundStyle(BrandColors.textMuted)
                        }
                    }
                }
            }

            if let escalation = deadlines.escalation, !escalation.isEmpty {
                Text("Escalation")
                    .symairaText(.bodyEmphasized)
                    .foregroundStyle(BrandColors.textSecondary)
                    .padding(.top, 4)
                HStack(spacing: 16) {
                    ForEach(escalation.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                        HStack(spacing: 4) {
                            Text("\(value.plainDigits)")
                                .symairaText(.bodyEmphasized)
                                .foregroundStyle(BrandColors.textPrimary)
                            Text(Self.escalationLabel(for: key))
                                .symairaText(.caption)
                                .foregroundStyle(BrandColors.textMuted)
                        }
                    }
                }
            }
        }
        .interactiveGlassCard()
    }

    private var tickActionsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Tick Actions")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if vm.tickActions.isEmpty {
                Text("No tick actions pending")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            } else {
                Table(vm.tickActions) {
                    TableColumn("Request") { Text("\($0.requestId.plainDigits)") }
                    TableColumn("Broker") { Text($0.brokerId).foregroundStyle(BrandColors.goldPrimary) }
                    TableColumn("Action") {
                        Text($0.actionType.replacingOccurrences(of: "_", with: " ").capitalized)
                    }
                    TableColumn("Status") { StatusBadge(status: $0.currentStatus) }
                    TableColumn("Description") { Text($0.description).lineLimit(2) }
                }
                .frame(minHeight: 100, idealHeight: CGFloat(min(vm.tickActions.count, 8)) * 36 + 40)
            }
        }
        .interactiveGlassCard()
    }
}
