import SwiftUI

/// Calendar view showing upcoming deadlines and tick actions.
struct CalendarView: View {
    @StateObject private var vm = CalendarViewModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                if let error = vm.errorMessage {
                    ErrorBanner(message: error) { vm.errorMessage = nil }
                }

                if vm.isLoading && vm.calendarData == nil {
                    LoadingOverlay(message: "Loading calendar…")
                        .frame(height: 200)
                } else if let data = vm.calendarData {
                    if let deadlines = data.upcomingDeadlines {
                        deadlinesSummary(deadlines)
                    }
                    tickActionsSection
                } else {
                    EmptyStateView(
                        icon: "calendar",
                        title: "No Calendar Data",
                        message: "Create a campaign to see upcoming deadlines."
                    )
                    .frame(height: 200)
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
                    .font(.largeTitle.bold())
                    .foregroundStyle(BrandColors.textPrimary)
                Text("Upcoming deadlines and tick actions")
                    .font(.caption)
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
                    .font(.title3)
            }
            .buttonStyle(.plain)
            .foregroundStyle(BrandColors.goldPrimary)
        }
    }

    private func deadlinesSummary(_ deadlines: UpcomingDeadlines) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Deadline Summary")
                .font(.headline)
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
                    .font(.subheadline.bold())
                    .foregroundStyle(BrandColors.textSecondary)
                    .padding(.top, 4)
                HStack(spacing: 16) {
                    ForEach(byStatus.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                        VStack(spacing: 4) {
                            Text("\(value)")
                                .font(.title3.bold())
                                .foregroundStyle(BrandColors.color(for: key))
                            Text(key.replacingOccurrences(of: "_", with: " "))
                                .font(.caption2)
                                .foregroundStyle(BrandColors.textMuted)
                        }
                    }
                }
            }

            if let escalation = deadlines.escalation, !escalation.isEmpty {
                Text("Escalation")
                    .font(.subheadline.bold())
                    .foregroundStyle(BrandColors.textSecondary)
                    .padding(.top, 4)
                HStack(spacing: 16) {
                    ForEach(escalation.sorted(by: { $0.key < $1.key }), id: \.key) { key, value in
                        HStack(spacing: 4) {
                            Text("\(value)")
                                .font(.subheadline.bold())
                                .foregroundStyle(BrandColors.textPrimary)
                            Text(key)
                                .font(.caption)
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
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            if vm.tickActions.isEmpty {
                Text("No tick actions pending")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            } else {
                Table(vm.tickActions) {
                    TableColumn("Request") { Text("\($0.requestId)") }
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
