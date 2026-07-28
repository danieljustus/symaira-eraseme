import Foundation

/// View model for the Brokers view.
@MainActor
final class BrokersViewModel: ObservableObject {
    @Published var state: ViewState<[Broker]> = .idle
    @Published var total: Int = 0

    /// Filters
    @Published var filterJurisdiction: String = ""
    @Published var filterLaw: String = ""
    @Published var filterPriority: String = ""
    @Published var filterCategory: String = ""
    @Published var searchText: String = ""

    /// Selected broker for detail sheet.
    @Published var selectedBroker: Broker?

    var brokers: [Broker] { state.value ?? [] }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    var filteredBrokers: [Broker] {
        if searchText.isEmpty { return brokers }
        let q = searchText.lowercased()
        return brokers.filter {
            $0.name.lowercased().contains(q) ||
            $0.brokerId.lowercased().contains(q) ||
            ($0.category ?? "").lowercased().contains(q)
        }
    }

    func refresh() async {
        state = .loading

        var args: [String: Any] = [:]
        if !filterJurisdiction.isEmpty { args["jurisdiction"] = filterJurisdiction }
        if !filterLaw.isEmpty { args["law"] = filterLaw }
        if !filterPriority.isEmpty { args["priority"] = filterPriority }
        if !filterCategory.isEmpty { args["category"] = filterCategory }

        do {
            let response: BrokerListResponse = try await MCPClient.shared.callTool("list_brokers", arguments: args)
            state = .loaded(response.brokers)
            total = response.total
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func resetFilters() {
        filterJurisdiction = ""
        filterLaw = ""
        filterPriority = ""
        filterCategory = ""
        searchText = ""
        Task { await refresh() }
    }
}
