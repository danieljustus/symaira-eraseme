import Foundation

/// View model for the Requests view.
@MainActor
final class RequestsViewModel: ObservableObject {
    @Published var state: ViewState<[RemovalRequest]> = .idle
    @Published var total: Int = 0
    @Published var page: Int = 1
    @Published var pageSize: Int = 50

    /// Filters
    @Published var filterCampaignId: String = ""
    @Published var filterStatus: String = ""
    @Published var filterBrokerId: String = ""

    /// Selected request for detail view.
    @Published var selectedRequest: RemovalRequest?
    @Published var requestEvents: [RequestEvent] = []
    @Published var eventsState: ViewState<[RequestEvent]> = .idle

    var requests: [RemovalRequest] { state.value ?? [] }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    var totalPages: Int { max(1, Int(ceil(Double(total) / Double(pageSize)))) }
    var hasPrevious: Bool { page > 1 }
    var hasNext: Bool { page < totalPages }

    func refresh() async {
        state = .loading

        var args: [String: Any] = [
            "page": page,
            "page_size": pageSize
        ]
        if !filterCampaignId.isEmpty { args["campaign_id"] = filterCampaignId }
        if !filterStatus.isEmpty { args["status"] = filterStatus }
        if !filterBrokerId.isEmpty { args["broker_id"] = filterBrokerId }

        do {
            let response: RequestListResponse = try await MCPClient.shared.callTool("list_requests", arguments: args)
            state = .loaded(response.items)
            total = response.total
        } catch {
            state = .failed(error.localizedDescription)
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
        eventsState = .loading
        requestEvents = []

        do {
            let response: EventListResponse = try await MCPClient.shared.callTool("get_events", arguments: [
                "request_id": request.id
            ])
            requestEvents = response.events
            eventsState = .loaded(response.events)
        } catch {
            eventsState = .failed(error.localizedDescription)
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
