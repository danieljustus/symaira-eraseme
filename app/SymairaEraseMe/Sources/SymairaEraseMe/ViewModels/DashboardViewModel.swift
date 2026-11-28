import Foundation
import SwiftUI
import SymairaTheme

/// One dashboard summary value. Attention tiles carry a tone and an icon so
/// the count never relies on colour alone (GUIDELINES NN-6).
struct DashboardSummaryTile: Identifiable {
    let id: String
    let title: String
    let value: Int
    let tone: SymairaTone
    let icon: SymairaIcon
    let detail: String?
    /// Backend status to preselect when the tile is activated, if any.
    let statusFilter: String?

    var isAttention: Bool { tone == .critical }
}

extension DashboardSummaryTile: Equatable {
    static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.id == rhs.id && lhs.title == rhs.title && lhs.value == rhs.value
            && lhs.detail == rhs.detail && lhs.statusFilter == rhs.statusFilter
    }
}

/// One status row in the dashboard breakdown. `status` is retained for
/// colour and filters; `label` is the plain-language text shown to users.
struct DashboardStatusCount: Identifiable, Equatable {
    let status: String
    let label: String
    let count: Int
    var id: String { status }
}

/// View model for the main Dashboard view.
@MainActor
final class DashboardViewModel: ObservableObject {
    @Published var state: ViewState<DashboardData> = .idle

    /// Backend status -> plain label. Unknown values keep their raw name
    /// (with underscores turned into spaces) instead of being dropped.
    private static let statusOrder = [
        "PLANNED", "SENT", "ACK", "AWAITING_ACK", "AWAITING_RESPONSE",
        "AWAITING_USER_ACTION", "RE_SCAN_DUE", "CONFIRMED", "REJECTED",
        "REJECTED_FINAL", "SEND_FAILED", "BOUNCE", "ESCALATED", "DPA_FILED",
        "PENDING", "COMPLETED",
    ]

    func refresh() async {
        state = .loading

        do {
            let data: DashboardData = try await MCPClient.shared.callTool("get_dashboard_data")
            state = .loaded(data)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Reload when the server becomes reachable so a stale "Server
    /// unreachable" error clears without a manual refresh.
    func reachabilityChanged(_ reachable: Bool) async {
        guard reachable, !isLoading else { return }
        await refresh()
    }

    var data: DashboardData? { state.value }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    /// Important: the backend's `by_status` is authoritative. The flat
    /// counters omit statuses such as SEND_FAILED, so they are only a fallback
    /// for older payloads that do not carry `by_status` (#1340).
    var statusCounts: [String: Int] {
        if let byStatus = data?.byStatus { return byStatus }
        guard let d = data else { return [:] }
        return [
            "PLANNED": d.planned,
            "SENT": d.sent,
            "AWAITING_ACK": d.awaitingAck,
            "AWAITING_RESPONSE": d.awaitingResponse,
            "CONFIRMED": d.confirmed,
            "REJECTED_FINAL": d.rejected,
        ]
    }

    var totalRequests: Int { data?.totalRequests ?? 0 }
    /// Sent and unresolved requests, from `by_status` when present: the flat
    /// counters omit ACK and other open statuses.
    var inProgress: Int {
        guard let d = data else { return 0 }
        return RequestProgress.inProgress(byStatus: d.byStatus, fallback: d.sent + d.awaitingAck + d.awaitingResponse)
    }
    var confirmed: Int { data?.confirmed ?? 0 }
    var rejected: Int { data?.rejected ?? 0 }
    var overdue: Int { data?.overdue ?? 0 }
    var planned: Int { data?.planned ?? 0 }
    var failedSends: Int { statusCounts["SEND_FAILED"] ?? 0 }

    static func label(for status: String) -> String {
        StatusBadge.vocabulary[status.uppercased()]?.label
            ?? status.replacingOccurrences(of: "_", with: " ").capitalized
    }

    /// Status breakdown for the bar chart. Overdue is an attention flag
    /// derived from deadlines, not a backend status, so it is never drawn as
    /// a status bar. Unknown backend statuses are displayed, not dropped.
    var statusBreakdown: [DashboardStatusCount] {
        let counts = statusCounts
        let ordered = Self.statusOrder.filter { counts[$0] != nil }
        let unknown = counts.keys
            .filter { !Self.statusOrder.contains($0) && $0.uppercased() != "OVERDUE" }
            .sorted()
        return (ordered + unknown).compactMap { status in
            guard status.uppercased() != "OVERDUE", let count = counts[status] else { return nil }
            return DashboardStatusCount(status: status, label: Self.label(for: status), count: count)
        }
    }

    /// Six tiles in a three-column grid: never more than five in one row.
    var summaryTiles: [DashboardSummaryTile] {
        [
            DashboardSummaryTile(
                id: "total", title: "Total", value: totalRequests, tone: .neutral,
                icon: .chart, detail: "requests", statusFilter: nil),
            DashboardSummaryTile(
                id: "inProgress", title: "In Progress", value: inProgress, tone: .warning,
                icon: .pending, detail: nil, statusFilter: nil),
            DashboardSummaryTile(
                id: "confirmed", title: "Confirmed", value: confirmed, tone: .positive,
                icon: .success, detail: nil, statusFilter: "CONFIRMED"),
            DashboardSummaryTile(
                id: "rejected", title: "Rejected", value: rejected, tone: .critical,
                icon: .failure, detail: nil, statusFilter: "REJECTED_FINAL"),
            DashboardSummaryTile(
                id: "sendFailed", title: "Send Failed", value: failedSends, tone: .critical,
                icon: .failure, detail: "needs attention", statusFilter: "SEND_FAILED"),
            DashboardSummaryTile(
                id: "overdue", title: "Overdue", value: overdue, tone: .critical,
                icon: .warning, detail: "needs attention", statusFilter: nil),
        ]
    }
}
