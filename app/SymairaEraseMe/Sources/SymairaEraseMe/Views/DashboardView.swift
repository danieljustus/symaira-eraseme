import SwiftUI
import SymairaTheme
import Charts

/// Main dashboard view showing summary cards, status breakdown, campaigns, broker grid, and events.
struct DashboardView: View {
    @StateObject private var vm = DashboardViewModel()

    var body: some View {
        Page(
            title: "Dashboard",
            subtitle: vm.data.map { "Generated \($0.generatedAt.formattedDate)" } ?? "Overview of your removal requests",
            refresh: vm.refresh
        ) {
            PageState(state: vm.state, loading: "Loading dashboard…", retry: vm.refresh) { data in
                if data.totalRequests == 0 {
                    PageEmpty(
                        systemImage: "chart.bar.fill",
                        title: "No Data Yet",
                        message: "Create a campaign to start removing your data from brokers."
                    )
                } else {
                    summaryCards
                    statusBreakdownChart
                    campaignsSection
                    brokerGrid
                    recentEvents
                }
            }
        }
        .task { await vm.refresh() }
    }

    // MARK: - Summary Cards

    private var summaryCards: some View {
        LazyVGrid(columns: [
            GridItem(.adaptive(minimum: 150))
        ], spacing: SymairaSpacing.medium) {
            StatCard(title: "Total", value: vm.totalRequests)
            StatCard(title: "In Progress", value: vm.inProgress)
            StatCard(title: "Confirmed", value: vm.confirmed)
            StatCard(title: "Rejected", value: vm.rejected)
            StatCard(title: "Overdue", value: vm.overdue)
        }
    }

    // MARK: - Status Breakdown Chart

    private var statusBreakdownChart: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Status Breakdown")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            Chart(vm.statusBreakdown, id: \.label) { item in
                BarMark(
                    x: .value("Count", item.count),
                    y: .value("Status", item.label)
                )
                .foregroundStyle(item.color)
                .cornerRadius(4)
            }
            .chartXAxis(.hidden)
            .chartYAxis {
                AxisMarks { value in
                    AxisValueLabel()
                        .foregroundStyle(BrandColors.textSecondary)
                }
            }
            .frame(height: CGFloat(vm.statusBreakdown.count * 32 + 20))
        }
        .symairaCard()
        .topoProtected(hill: true)
    }

    // MARK: - Campaigns Table

    private var campaignsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Campaigns")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let campaigns = vm.data?.campaigns, !campaigns.isEmpty {
                Table(campaigns) {
                    TableColumn("ID") { Text($0.id) }
                    TableColumn("Kind") { Text($0.kind ?? "—") }
                    TableColumn("Total") { Text("\($0.total.plainDigits)") }
                    TableColumn("Confirmed") { Text("\($0.confirmed.plainDigits)").foregroundStyle(BrandColors.confirmed) }
                    TableColumn("Rejected") { Text("\($0.rejected.plainDigits)").foregroundStyle(BrandColors.rejected) }
                    TableColumn("Overdue") { Text("\($0.overdue.plainDigits)").foregroundStyle(BrandColors.overdue) }
                    TableColumn("Pending") {
                        Text("\(($0.sent + $0.awaitingAck + $0.awaitingResponse).plainDigits)")
                            .foregroundStyle(BrandColors.pending)
                    }
                }
                .frame(height: min(CGFloat(campaigns.count) * 40 + 40, 280))
            } else {
                Text("No campaigns yet")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            }
        }
        .symairaCard()
        .topoProtected(hill: true)
    }

    // MARK: - Broker Status Grid

    private var brokerGrid: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Broker Status")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let brokerStatus = vm.data?.brokerStatus, !brokerStatus.isEmpty {
                let columns = [
                    GridItem(.adaptive(minimum: 180, maximum: 280), spacing: SymairaSpacing.medium)
                ]
                LazyVGrid(columns: columns, spacing: SymairaSpacing.medium) {
                    ForEach(brokerStatus.prefix(20)) { broker in
                        BrokerStatusCard(broker: broker)
                    }
                }
            } else {
                Text("No broker data")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            }
        }
        .symairaCard()
        .topoProtected(hill: true)
    }

    // MARK: - Recent Events Timeline

    private var recentEvents: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Recent Events")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let events = vm.data?.recentEvents, !events.isEmpty {
                VStack(spacing: 0) {
                    ForEach(events.prefix(15)) { event in
                        EventRow(event: event)
                        if event.id != events.prefix(15).last?.id {
                            SymairaFormDivider()
                        }
                    }
                }
            } else {
                Text("No recent events")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            }
        }
        .symairaCard()
        .topoProtected(hill: true)
    }
}

// MARK: - Subviews

struct BrokerStatusCard: View {
    let broker: BrokerStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(broker.brokerId)
                .symairaText(.bodyEmphasized)
                .foregroundStyle(BrandColors.textPrimary)
                .lineLimit(1)
            HStack(spacing: SymairaSpacing.small) {
                if broker.confirmed > 0 {
                    Label("\(broker.confirmed.plainDigits)", symairaIcon: .success, size: 12)
                        .foregroundStyle(BrandColors.confirmed)
                        .symairaText(.caption)
                }
                if broker.pending > 0 {
                    Label("\(broker.pending.plainDigits)", symairaIcon: .pending, size: 12)
                        .foregroundStyle(BrandColors.pending)
                        .symairaText(.caption)
                }
                if broker.overdue > 0 {
                    Label("\(broker.overdue.plainDigits)", symairaIcon: .warning, size: 12)
                        .foregroundStyle(BrandColors.overdue)
                        .symairaText(.caption)
                }
                if broker.rejected > 0 {
                    Label("\(broker.rejected.plainDigits)", symairaIcon: .failure, size: 12)
                        .foregroundStyle(BrandColors.rejected)
                        .symairaText(.caption)
                }
            }
            Text("\(broker.total.plainDigits) total")
                .symairaText(.caption)
                .foregroundStyle(BrandColors.textMuted)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard(padding: SymairaSpacing.medium)
    }
}

struct EventRow: View {
    let event: RecentEvent

    var body: some View {
        HStack(spacing: SymairaSpacing.medium) {
            VStack(alignment: .leading, spacing: 2) {
                Text(event.eventType.replacingOccurrences(of: "_", with: " ").capitalized)
                    .symairaText(.callout)
                    .foregroundStyle(BrandColors.textPrimary)
                if let brokerId = event.brokerId {
                    Text(brokerId)
                        .symairaText(.caption)
                        .foregroundStyle(BrandColors.textPrimary)
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                Text(event.source)
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.textMuted)
                if let date = event.occurredDate {
                    Text(date.formatted(.relative(presentation: .named)))
                        .symairaText(.caption)
                        .foregroundStyle(BrandColors.textMuted)
                }
            }
        }
        .padding(.vertical, 6)
    }
}

// MARK: - Error State View

struct ErrorStateView: View {
    let message: String
    let onRetry: () -> Void

    var body: some View {
        SymairaEmptyState(
            systemImage: "exclamationmark.triangle.fill", title: "Failed to Load", message: message
        ) {
            Button(action: onRetry) {
                Label("Retry", symairaIcon: .refresh)
            }
            .symairaButtonStyle(.secondary)
        }
    }
}

// MARK: - Date Formatting Helpers

extension String {
    var formattedDate: String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = formatter.date(from: self) {
            return date.formatted(.dateTime.month().day().hour().minute())
        }
        let simple = ISO8601DateFormatter()
        if let date = simple.date(from: self) {
            return date.formatted(.dateTime.month().day().hour().minute())
        }
        return self
    }
}
