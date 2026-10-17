import SwiftUI
import SymairaTheme
import Charts

/// Main dashboard view showing summary cards, status breakdown, campaigns, broker grid, and events.
struct DashboardView: View {
    @StateObject private var vm = DashboardViewModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                switch vm.state {
                case .idle, .loading:
                    SymairaLoadingState("Loading dashboard…")
                        .frame(height: 300)

                case .failed(let message):
                    ErrorStateView(message: message) {
                        Task { await vm.refresh() }
                    }
                    .frame(height: 300)

                case .loaded(let data):
                    if data.totalRequests == 0 {
                        SymairaEmptyState(
                            systemImage: "chart.bar.fill",
                            title: "No Data Yet",
                            message: "Create a campaign to start removing your data from brokers."
                        )
                        .frame(height: 300)
                    } else {
                        summaryCards
                        statusBreakdownChart
                        campaignsSection
                        brokerGrid
                        recentEvents
                    }
                }
            }
            .padding(24)
            .frame(maxWidth: 1008)
            .frame(maxWidth: .infinity)
        }
        .background(Color.clear)
        .task { await vm.refresh() }
    }

    // MARK: - Header

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Dashboard")
                    .symairaText(.display)
                    .topoProtected()
                if let data = vm.data {
                    Text("Generated \(data.generatedAt.formattedDate)")
                        .symairaText(.caption)
                        .topoProtected()
                }
            }
            Spacer()
            Button {
                Task { await vm.refresh() }
            } label: {
                Image(systemName: "arrow.clockwise")
                    .symairaText(.heading)
            }
            .symairaButtonStyle(.secondary)
            .accessibilityLabel("Refresh dashboard")
            .topoProtected()
        }
    }

    // MARK: - Summary Cards

    private var summaryCards: some View {
        LazyVGrid(columns: [
            GridItem(.adaptive(minimum: 150))
        ], spacing: 12) {
            StatCard(title: "Total", value: vm.totalRequests)
            StatCard(title: "In Progress", value: vm.inProgress)
            StatCard(title: "Confirmed", value: vm.confirmed)
            StatCard(title: "Rejected", value: vm.rejected)
            StatCard(title: "Overdue", value: vm.overdue)
        }
    }

    // MARK: - Status Breakdown Chart

    private var statusBreakdownChart: some View {
        VStack(alignment: .leading, spacing: 8) {
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
        VStack(alignment: .leading, spacing: 8) {
            Text("Campaigns")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let campaigns = vm.data?.campaigns, !campaigns.isEmpty {
                Table(campaigns) {
                    TableColumn("ID") { Text($0.id).foregroundStyle(BrandColors.goldPrimary) }
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
        VStack(alignment: .leading, spacing: 8) {
            Text("Broker Status")
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)

            if let brokerStatus = vm.data?.brokerStatus, !brokerStatus.isEmpty {
                let columns = [
                    GridItem(.adaptive(minimum: 180, maximum: 280), spacing: 12)
                ]
                LazyVGrid(columns: columns, spacing: 12) {
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
        VStack(alignment: .leading, spacing: 8) {
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
                .foregroundStyle(BrandColors.goldPrimary)
                .lineLimit(1)
            HStack(spacing: 8) {
                if broker.confirmed > 0 {
                    Label("\(broker.confirmed.plainDigits)", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(BrandColors.confirmed)
                        .symairaText(.caption)
                }
                if broker.pending > 0 {
                    Label("\(broker.pending.plainDigits)", systemImage: "clock.fill")
                        .foregroundStyle(BrandColors.pending)
                        .symairaText(.caption)
                }
                if broker.overdue > 0 {
                    Label("\(broker.overdue.plainDigits)", systemImage: "exclamationmark.circle.fill")
                        .foregroundStyle(BrandColors.overdue)
                        .symairaText(.caption)
                }
                if broker.rejected > 0 {
                    Label("\(broker.rejected.plainDigits)", systemImage: "xmark.circle.fill")
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
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(event.eventType.replacingOccurrences(of: "_", with: " ").capitalized)
                    .symairaText(.callout)
                    .foregroundStyle(BrandColors.textPrimary)
                if let brokerId = event.brokerId {
                    Text(brokerId)
                        .symairaText(.caption)
                        .foregroundStyle(BrandColors.goldPrimary)
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
                Label("Retry", systemImage: "arrow.clockwise")
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
