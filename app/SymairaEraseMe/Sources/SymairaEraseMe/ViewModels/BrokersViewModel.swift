import Foundation

/// View model for the Brokers view.
@MainActor
final class BrokersViewModel: ObservableObject {
    @Published var state: ViewState<[Broker]> = .idle
    /// Active brokers in the registry, independent of the filters; nil until loaded.
    @Published var registryTotal: Int?
    /// Values the filter menus offer, taken from the registry itself (#1313).
    @Published var facets = RegistryFacets()

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

    /// The registry total stays visible; filters and search add the share they show.
    var subtitle: String {
        guard let total = registryTotal, state.value != nil else { return "Broker registry" }
        let shown = filteredBrokers.count
        if shown == total && searchText.isEmpty { return "\(total.plainDigits) brokers in registry" }
        return "\(shown.plainDigits) of \(total.plainDigits) brokers"
    }

    func refresh() async {
        state = .loading

        var args: [String: Any] = [:]
        if !filterJurisdiction.isEmpty { args["jurisdiction"] = filterJurisdiction }
        if !filterLaw.isEmpty { args["law"] = filterLaw }
        if !filterPriority.isEmpty { args["priority"] = filterPriority }
        if !filterCategory.isEmpty { args["category"] = filterCategory }

        do {
            // The unfiltered list supplies the registry total and the menu values.
            if args.isEmpty || registryTotal == nil {
                let all: BrokerListResponse = try await MCPClient.shared.callTool("list_brokers")
                registryTotal = all.count
                facets = RegistryFacets(all.brokers)
                if args.isEmpty {
                    state = .loaded(all.brokers)
                    return
                }
            }
            let response: BrokerListResponse = try await MCPClient.shared.callTool("list_brokers", arguments: args)
            state = .loaded(response.brokers)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }
}
