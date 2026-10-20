import SwiftUI
import SymairaTheme

/// Calendar view showing upcoming deadlines and tick actions.
struct CalendarView: View {
    @StateObject private var vm = CalendarViewModel()

    var body: some View {
        Page(title: "Calendar", subtitle: "Upcoming deadlines and tick actions", refresh: vm.refresh) {
            SymairaSegmentedControl(
                "Weeks", selection: $vm.weeks,
                options: [(2, "2 weeks"), (4, "4 weeks"), (8, "8 weeks"), (12, "12 weeks")])
        } content: {
            PageState(state: vm.state, loading: "Loading calendar…", retry: vm.refresh) { data in
                if data.upcomingDeadlines == nil && vm.tickActions.isEmpty {
                    PageEmpty(
                        icon: .calendar,
                        title: "No Calendar Data",
                        message: "Create a campaign to see upcoming deadlines."
                    )
                } else {
                    if let deadlines = data.upcomingDeadlines {
                        deadlinesSummary(deadlines)
                    }
                    tickActionsSection
                }
            }
        }
        .task(id: vm.weeks) { await vm.refresh() }
    }

    /// Human-readable label for an escalation category identifier returned
    /// by the reporting backend. Unknown identifiers fall back to
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
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            Text("Deadline Summary")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let totals = deadlines.totals {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: SymairaSpacing.medium) {
                    StatCard(title: "Total Requests", value: totals.requests ?? 0)
                    StatCard(title: "Resolved", value: totals.resolved ?? 0)
                    StatCard(title: "Open", value: totals.open ?? 0)
                }
            }

            if let upcoming = deadlines.upcoming {
                LazyVGrid(columns: [
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                    GridItem(.flexible()),
                ], spacing: SymairaSpacing.medium) {
                    StatCard(title: "Overdue", value: upcoming.overdue ?? 0)
                    StatCard(title: "Due in 7d", value: upcoming.deadlineDueWithin7d ?? 0)
                    StatCard(title: "Due in 30d", value: upcoming.deadlineDueWithin30d ?? 0)
                    StatCard(title: "Tick Actions", value: upcoming.tickActionsReady ?? 0)
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
        .symairaCard()
    }

    private var tickActionsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
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
                    TableColumn("Broker") { Text($0.brokerId) }
                    TableColumn("Action") {
                        Text($0.actionType.replacingOccurrences(of: "_", with: " ").capitalized)
                    }
                    TableColumn("Status") { StatusBadge(status: $0.currentStatus) }
                    TableColumn("Description") { Text($0.description).lineLimit(2) }
                }
                .frame(minHeight: 100, idealHeight: CGFloat(min(vm.tickActions.count, 8)) * 36 + 40)
            }
        }
        .symairaCard()
    }
}
