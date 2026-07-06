import Foundation
import SwiftUI

/// View model for the main Dashboard view.
@MainActor
final class DashboardViewModel: ObservableObject {
    @Published var data: DashboardData?
    @Published var isLoading = false
    @Published var errorMessage: String?

    func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            data = try await MCPClient.shared.callTool("get_dashboard_data")
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var totalRequests: Int { data?.totalRequests ?? 0 }
    var inProgress: Int {
        guard let d = data else { return 0 }
        return d.sent + d.awaitingAck + d.awaitingResponse
    }
    var confirmed: Int { data?.confirmed ?? 0 }
    var rejected: Int { data?.rejected ?? 0 }
    var overdue: Int { data?.overdue ?? 0 }
    var planned: Int { data?.planned ?? 0 }

    /// Status counts for the bar chart, ordered.
    var statusBreakdown: [(label: String, count: Int, color: Color)] {
        guard let d = data else { return [] }
        return [
            ("Planned", d.planned, BrandColors.planned),
            ("Sent", d.sent, BrandColors.pending),
            ("Awaiting ACK", d.awaitingAck, BrandColors.pending),
            ("Awaiting Response", d.awaitingResponse, BrandColors.pending),
            ("Confirmed", d.confirmed, BrandColors.confirmed),
            ("Rejected", d.rejected, BrandColors.rejected),
            ("Overdue", d.overdue, BrandColors.overdue),
        ]
    }
}
