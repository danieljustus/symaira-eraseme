import Foundation

/// View model for the Requests view.
@MainActor
final class RequestsViewModel: ObservableObject {
    @Published var requests: [RemovalRequest] = []
    @Published var total: Int = 0
    @Published var page: Int = 1
    @Published var pageSize: Int = 50
    @Published var isLoading = false
    @Published var errorMessage: String?

    /// Filters
    @Published var filterCampaignId: String = ""
    @Published var filterStatus: String = ""
    @Published var filterBrokerId: String = ""

    /// Selected request for detail view.
    @Published var selectedRequest: RemovalRequest?
    @Published var requestEvents: [RequestEvent] = []
    @Published var isLoadingEvents = false

    var totalPages: Int { max(1, Int(ceil(Double(total) / Double(pageSize)))) }
    var hasPrevious: Bool { page > 1 }
    var hasNext: Bool { page < totalPages }

    func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        var args: [String: Any] = [
            "page": page,
            "page_size": pageSize
        ]
        if !filterCampaignId.isEmpty { args["campaign_id"] = filterCampaignId }
        if !filterStatus.isEmpty { args["status"] = filterStatus }
        if !filterBrokerId.isEmpty { args["broker_id"] = filterBrokerId }

        do {
            let response: RequestListResponse = try await MCPClient.shared.callTool("list_requests", arguments: args)
            requests = response.items
            total = response.total
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func nextPage() {
        guard hasNext else { return }
        page += 1
        Task { await refresh() }
    }

    func previousPage() {
        guard hasPrevious else { return }
        page -= 1
        Task { await refresh() }
    }

    func loadEvents(for request: RemovalRequest) async {
        selectedRequest = request
        isLoadingEvents = true
        requestEvents = []
        defer { isLoadingEvents = false }

        do {
            let response: EventListResponse = try await MCPClient.shared.callTool("get_events", arguments: [
                "request_id": request.id
            ])
            requestEvents = response.events
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func resetFilters() {
        filterCampaignId = ""
        filterStatus = ""
        filterBrokerId = ""
        page = 1
        Task { await refresh() }
    }
}
