import SwiftUI
import SymairaTheme
import Charts

/// Main dashboard view showing summary cards, status breakdown, campaigns, broker grid, and events.
struct DashboardView: View {
    @EnvironmentObject private var serverManager: ServerManager
    @StateObject private var vm = DashboardViewModel()
    /// Product-owned navigation: a tile can open Requests with a status filter.
    private var openStatus: ((String) -> Void)?

    init(openStatus: ((String) -> Void)? = nil) {
        self.openStatus = openStatus
    }

    var body: some View {
        SymairaPage(
            title: "Dashboard",
            subtitle: vm.data.map { "Generated \($0.generatedAt.formattedDate)" } ?? "Overview of your removal requests"
        ) {
            Spacer()
            PageRefreshButton(title: "Dashboard", refresh: vm.refresh)
        } content: {
            SymairaPageState(state: vm.state.pageState, loading: "Loading dashboard…", retry: { Task { await vm.refresh() } }) { data in
                if data.totalRequests == 0 {
                    SymairaPageEmpty(
                        icon: .chart,
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
        .onChange(of: serverManager.mcpReachable) { _, reachable in
            Task { await vm.reachabilityChanged(reachable) }
        }
    }

    // MARK: - Summary Tiles

    private var summaryCards: some View {
        // Three columns: the six tiles form two rows of three (LAYOUT-8).
        LazyVGrid(
            columns: Array(repeating: GridItem(.flexible(), spacing: SymairaSpacing.medium), count: 3),
            spacing: SymairaSpacing.medium
        ) {
            ForEach(vm.summaryTiles) { tile in
                DashboardTileView(tile: tile) {
                    guard let status = tile.statusFilter else { return }
                    openStatus?(status)
                }
            }
        }
    }

    // MARK: - Status Breakdown Chart

    private var statusBreakdownChart: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack {
                Text("Status Breakdown")
                    .symairaText(.subheading)
                Spacer()
                // Overdue is a deadline flag, never a status bar in this chart.
                if vm.overdue > 0 {
                    SymairaBadge(
                        "\(vm.overdue.plainDigits) overdue",
                        tone: .critical, icon: .warning
                    )
                }
            }

            let visible = vm.statusBreakdown.filter { $0.count > 0 }
            Chart(visible) { item in
                BarMark(
                    x: .value("Count", item.count),
                    y: .value("Status", item.label)
                )
                .foregroundStyle(BrandColors.color(for: item.status))
                .annotation(position: .trailing, alignment: .leading, spacing: 4) {
                    Text(item.count.plainDigits)
                        .symairaText(.caption)
                        .monospacedDigit()
                }
            }
            .chartXAxis(.hidden)
            .chartYAxis {
                AxisMarks { value in
                    AxisValueLabel()
                        .foregroundStyle(BrandColors.textSecondary)
                }
            }
            .frame(height: CGFloat(max(1, visible.count) * 34 + 20))
        }
        .symairaCard()
        .topoProtected(hill: true)
    }

    // MARK: - Campaigns Table

    private var campaignsSection: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text("Campaigns")
                .symairaText(.subheading)

            if let campaigns = vm.data?.campaigns, !campaigns.isEmpty {
                // Five narrow columns instead of seven: the old table clipped
                // at the minimum window width (#1341). Counts are merged and
                // every figure keeps its text label.
                Table(campaigns) {
                    TableColumn("Campaign") { Text($0.id).lineLimit(1) }
                        .width(min: 80, ideal: 110)
                    TableColumn("Total") { Text("\($0.total.plainDigits)").monospacedDigit() }
                        .width(min: 50, ideal: 60, max: 80)
                    TableColumn("In Flight") {
                        Text("\($0.inProgress.plainDigits)")
                            .monospacedDigit()
                    }
                    .width(min: 60, ideal: 70, max: 90)
                    TableColumn("Confirmed") {
                        Text("\($0.confirmed.plainDigits)")
                            .foregroundStyle(BrandColors.confirmed)
                            .monospacedDigit()
                    }
                    .width(min: 70, ideal: 80, max: 100)
                    TableColumn("Needs Attention") { campaign in
                        HStack(spacing: SymairaSpacing.xSmall) {
                            if campaign.rejected > 0 {
                                Label("\(campaign.rejected.plainDigits) rejected", symairaIcon: .failure, size: 12)
                                    .foregroundStyle(BrandColors.rejected)
                            }
                            if campaign.overdue > 0 {
                                Label("\(campaign.overdue.plainDigits) overdue", symairaIcon: .warning, size: 12)
                                    .foregroundStyle(BrandColors.overdue)
                            }
                            if campaign.rejected == 0 && campaign.overdue == 0 {
                                Text("—").foregroundStyle(BrandColors.textMuted)
                            }
                        }
                        .symairaText(.caption)
                    }
                    .width(min: 210)
                }
                // Inside the page's scroll view a Table without a height
                // collapses to nothing: header and rows were clipped away.
                // Same sizing as the calendar's week tables.
                .frame(minHeight: 60, idealHeight: CGFloat(min(campaigns.count, 8)) * 24 + 36)
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

            if let events = vm.data?.recentEvents, !events.isEmpty {
                // The backend labels nearly every event `system`; show the
                // column only when it actually distinguishes rows (#1341).
                let showsSource = events.contains { $0.source.lowercased() != "system" }
                VStack(spacing: 0) {
                    ForEach(Array(events.prefix(15))) { event in
                        EventRow(event: event, showsSource: showsSource)
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

/// A native `LabeledContent` summary value. CoreKit 0.28.0 deprecates
/// `SymairaStatTile` (UI catalog E33), and `gui-review.py` flags every use.
struct DashboardTileView: View {
    let tile: DashboardSummaryTile
    var action: (() -> Void)?

    var body: some View {
        let content = VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            LabeledContent {
                Text(tile.value.plainDigits)
                    .font(SymairaTypography.stat)
                    .foregroundStyle(tile.tone.foreground)
                    .monospacedDigit()
            } label: {
                HStack(spacing: SymairaSpacing.xSmall) {
                    if tile.isAttention {
                        SymairaIconView(tile.icon, size: 12)
                            .foregroundStyle(tile.tone.foreground)
                    }
                    Text(tile.title)
                        .symairaText(.caption, respectsForeground: false)
                        .foregroundStyle(tile.isAttention ? tile.tone.foreground : BrandColors.textSecondary)
                }
            }
            Text(tile.detail ?? " ")
                .symairaText(.caption, respectsForeground: false)
                .foregroundStyle(tile.isAttention ? tile.tone.foreground : BrandColors.textMuted)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard()
        .topoProtected(hill: true)

        if let action {
            Button(action: action) { content }
                .buttonStyle(.plain)
                .accessibilityLabel("\(tile.title): \(tile.value.plainDigits). Show \(tile.title.lowercased()) requests")
        } else {
            content
                .accessibilityElement(children: .combine)
                .accessibilityLabel("\(tile.title): \(tile.value.plainDigits)")
        }
    }
}

struct BrokerStatusCard: View {
    let broker: BrokerStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // The dashboard payload carries IDs only; slug names become
            // readable words, and raw opaque IDs stay as-is.
            Text(RegistryFacets.displayName(broker.brokerId))
                .symairaText(.bodyEmphasized)
                .lineLimit(1)
            HStack(spacing: SymairaSpacing.small) {
                if broker.confirmed > 0 {
                    Label("\(broker.confirmed.plainDigits) confirmed", symairaIcon: .success, size: 12)
                        .foregroundStyle(BrandColors.confirmed)
                        .symairaText(.caption)
                }
                if broker.pending > 0 {
                    Label("\(broker.pending.plainDigits) in progress", symairaIcon: .pending, size: 12)
                        .foregroundStyle(BrandColors.pending)
                        .symairaText(.caption)
                }
                if broker.overdue > 0 {
                    Label("\(broker.overdue.plainDigits) overdue", symairaIcon: .warning, size: 12)
                        .foregroundStyle(BrandColors.overdue)
                        .symairaText(.caption)
                }
                if broker.rejected > 0 {
                    Label("\(broker.rejected.plainDigits) rejected", symairaIcon: .failure, size: 12)
                        .foregroundStyle(BrandColors.rejected)
                        .symairaText(.caption)
                }
            }
            Text("\(broker.total.plainDigits) total")
                .symairaText(.caption)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard(padding: SymairaSpacing.medium)
    }
}

struct EventRow: View {
    let event: RecentEvent
    var showsSource: Bool = true

    var body: some View {
        HStack(spacing: SymairaSpacing.medium) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: SymairaSpacing.xSmall) {
                    if event.isSendFailure {
                        SymairaIconView(.failure, size: 12)
                            .foregroundStyle(SymairaTokens.critical)
                    }
                    Text(event.displayLabel)
                        .symairaText(.callout, respectsForeground: false)
                        .foregroundStyle(event.isSendFailure ? SymairaTokens.critical : BrandColors.textPrimary)
                }
                if let brokerId = event.brokerId {
                    Text(RegistryFacets.displayName(brokerId))
                        .symairaText(.caption, respectsForeground: false)
                        .foregroundStyle(BrandColors.textPrimary)
                }
            }
            Spacer()
            VStack(alignment: .trailing, spacing: 2) {
                if showsSource {
                    Text(event.source)
                        .symairaText(.caption)
                }
                if let date = event.occurredDate {
                    Text(date.formatted(.relative(presentation: .named)))
                        .symairaText(.caption)
                } else {
                    Text(event.occurredAt.formattedDate)
                        .symairaText(.caption)
                }
            }
        }
        .padding(.vertical, 6)
    }
}

// MARK: - Date Formatting Helpers

extension String {
    var formattedDate: String {
        guard let date = BackendDate.parse(self) else { return self }
        return date.formatted(.dateTime.month().day().hour().minute())
    }
}
