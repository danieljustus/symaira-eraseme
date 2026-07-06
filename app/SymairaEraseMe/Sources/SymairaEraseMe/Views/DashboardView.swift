import SwiftUI
import Charts

/// Main dashboard view showing summary cards, status breakdown, campaigns, broker grid, and events.
struct DashboardView: View {
    @StateObject private var vm = DashboardViewModel()

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                if let error = vm.errorMessage {
                    ErrorBanner(message: error) { vm.errorMessage = nil }
                }
                if vm.isLoading && vm.data == nil {
                    LoadingOverlay(message: "Loading dashboard…")
                        .frame(height: 300)
                } else if let data = vm.data, data.totalRequests == 0 {
                    EmptyStateView(
                        icon: "chart.bar.fill",
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
            .padding(24)
        }
        .background(Color.clear)
        .task { await vm.refresh() }
    }

    // MARK: - Header

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Dashboard")
                    .font(.largeTitle.bold())
                    .foregroundStyle(BrandColors.textPrimary)
                if let data = vm.data {
                    Text("Generated \(data.generatedAt.formattedDate)")
                        .font(.caption)
                        .foregroundStyle(BrandColors.textMuted)
                }
            }
            Spacer()
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

    // MARK: - Summary Cards

    private var summaryCards: some View {
        LazyVGrid(columns: [
            GridItem(.flexible()),
            GridItem(.flexible()),
            GridItem(.flexible()),
            GridItem(.flexible()),
            GridItem(.flexible()),
        ], spacing: 12) {
            StatCard(title: "Total", value: vm.totalRequests, color: BrandColors.textPrimary)
            StatCard(title: "In Progress", value: vm.inProgress, color: BrandColors.pending)
            StatCard(title: "Confirmed", value: vm.confirmed, color: BrandColors.confirmed)
            StatCard(title: "Rejected", value: vm.rejected, color: BrandColors.rejected)
            StatCard(title: "Overdue", value: vm.overdue, color: BrandColors.overdue)
        }
    }

    // MARK: - Status Breakdown Chart

    private var statusBreakdownChart: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Status Breakdown")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            Chart(vm.statusBreakdown, id: \.label) { item in
                BarMark(
                    x: .value("Count", item.count),
                    y: .value("Status", item.label)
                )
                .foregroundStyle(LinearGradient(
                    colors: [item.color, item.color.opacity(0.6)],
                    startPoint: .leading,
                    endPoint: .trailing
                ))
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
        .interactiveGlassCard()
    }

    // MARK: - Campaigns Table

    private var campaignsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Campaigns")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            if let campaigns = vm.data?.campaigns, !campaigns.isEmpty {
                Table(campaigns) {
                    TableColumn("ID") { Text($0.id).foregroundStyle(BrandColors.goldPrimary) }
                    TableColumn("Kind") { Text($0.kind ?? "—") }
                    TableColumn("Total") { Text("\($0.total)") }
                    TableColumn("Confirmed") { Text("\($0.confirmed)").foregroundStyle(BrandColors.confirmed) }
                    TableColumn("Rejected") { Text("\($0.rejected)").foregroundStyle(BrandColors.rejected) }
                    TableColumn("Overdue") { Text("\($0.overdue)").foregroundStyle(BrandColors.overdue) }
                    TableColumn("Pending") {
                        Text("\($0.sent + $0.awaitingAck + $0.awaitingResponse)")
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
        .interactiveGlassCard()
    }

    // MARK: - Broker Status Grid

    private var brokerGrid: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Broker Status")
                .font(.headline)
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
        .interactiveGlassCard()
    }

    // MARK: - Recent Events Timeline

    private var recentEvents: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Recent Events")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            if let events = vm.data?.recentEvents, !events.isEmpty {
                VStack(spacing: 0) {
                    ForEach(events.prefix(15)) { event in
                        EventRow(event: event)
                        if event.id != events.prefix(15).last?.id {
                            Divider()
                                .background(BrandColors.textMuted.opacity(0.2))
                        }
                    }
                }
            } else {
                Text("No recent events")
                    .foregroundStyle(BrandColors.textMuted)
                    .padding(.vertical, 8)
            }
        }
        .interactiveGlassCard()
    }
}

// MARK: - Subviews

struct BrokerStatusCard: View {
    let broker: BrokerStatus
    @State private var isHovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(broker.brokerId)
                .font(.subheadline.bold())
                .foregroundStyle(BrandColors.goldPrimary)
                .lineLimit(1)
            HStack(spacing: 8) {
                if broker.confirmed > 0 {
                    Label("\(broker.confirmed)", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(BrandColors.confirmed)
                        .font(.caption)
                }
                if broker.pending > 0 {
                    Label("\(broker.pending)", systemImage: "clock.fill")
                        .foregroundStyle(BrandColors.pending)
                        .font(.caption)
                }
                if broker.overdue > 0 {
                    Label("\(broker.overdue)", systemImage: "exclamationmark.circle.fill")
                        .foregroundStyle(BrandColors.overdue)
                        .font(.caption)
                }
                if broker.rejected > 0 {
                    Label("\(broker.rejected)", systemImage: "xmark.circle.fill")
                        .foregroundStyle(BrandColors.rejected)
                        .font(.caption)
                }
            }
            Text("\(broker.total) total")
                .font(.caption2)
                .foregroundStyle(BrandColors.textMuted)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(isHovered ? BrandColors.bgCardHover : BrandColors.bgCard)
                .overlay(
                    RoundedRectangle(cornerRadius: 8)
                        .stroke(isHovered ? BrandColors.goldPrimary.opacity(0.2) : Color.white.opacity(0.05), lineWidth: 1)
                )
        )
        .scaleEffect(isHovered ? 1.01 : 1.0)
        .onHover { hovering in
            withAnimation(.spring(response: 0.2, dampingFraction: 0.8)) {
                isHovered = hovering
            }
        }
    }
}

struct EventRow: View {
    let event: RecentEvent

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(event.eventType.replacingOccurrences(of: "_", with: " ").capitalized)
                    .font(.subheadline)
                    .foregroundStyle(BrandColors.textPrimary)
                if let brokerId = event.brokerId {
                    Text(brokerId)
                        .font(.caption)
                        .foregroundStyle(BrandColors.goldPrimary)
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                Text(event.source)
                    .font(.caption2)
                    .foregroundStyle(BrandColors.textMuted)
                if let date = event.occurredDate {
                    Text(date.formatted(.relative(presentation: .named)))
                        .font(.caption2)
                        .foregroundStyle(BrandColors.textMuted)
                }
            }
        }
        .padding(.vertical, 6)
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
