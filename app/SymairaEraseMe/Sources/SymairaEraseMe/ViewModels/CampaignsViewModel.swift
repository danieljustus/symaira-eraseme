import Foundation
import SwiftUI

/// View model for the Campaigns view.
@MainActor
final class CampaignsViewModel: ObservableObject {
    @Published var state: ViewState<[DashboardCampaign]> = .idle
    @Published var planResult: PlanCreateResponse?
    @Published var executeResult: ExecuteResponse?

    /// New campaign form fields.
    @Published var newCampaignId: String = ""
    @Published var newCampaignJurisdiction: String = ""
    @Published var newCampaignLaw: String = ""
    @Published var newCampaignPriority: String = ""
    @Published var newCampaignMaxBrokers: Int = 30

    /// Values the filter menus offer, taken from the registry itself (#1318).
    @Published var facets = RegistryFacets()
    /// Active brokers the current filters select, counted by the server's own
    /// filter; nil while unknown.
    @Published var matchCount: Int?

    var campaigns: [DashboardCampaign] { state.value ?? [] }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    /// Whether the Create Campaign button may be used: the campaign id must
    /// be non-empty after trimming whitespace (issue #642).
    /// A filter combination that matches no broker cannot be created either:
    /// the server would still record an empty campaign (#1318).
    var canCreateCampaign: Bool {
        !newCampaignId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && matchCount != 0
    }

    private var filterArguments: [String: Any] {
        var args: [String: Any] = [:]
        if !newCampaignJurisdiction.isEmpty { args["jurisdiction"] = newCampaignJurisdiction }
        if !newCampaignLaw.isEmpty { args["law"] = newCampaignLaw }
        if !newCampaignPriority.isEmpty { args["priority"] = newCampaignPriority }
        return args
    }

    /// Loads the menu values once, then counts the brokers the filters select.
    func refreshMatches() async {
        do {
            if facets == RegistryFacets() {
                let all: BrokerListResponse = try await MCPClient.shared.callTool("list_brokers")
                facets = RegistryFacets(all.brokers)
            }
            let matches: BrokerListResponse = try await MCPClient.shared.callTool(
                "list_brokers", arguments: filterArguments)
            matchCount = matches.count
        } catch {
            matchCount = nil
        }
    }

    func refresh() async {
        state = .loading

        do {
            let dashboardData: DashboardData = try await MCPClient.shared.callTool("get_dashboard_data")
            state = .loaded(dashboardData.campaigns)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// Returns false, without creating anything, when the filters match no
    /// broker; the form then stays open with its warning.
    @discardableResult
    func createCampaign() async -> Bool {
        await refreshMatches()
        guard matchCount != 0 else { return false }
        state = .loading

        var args = filterArguments
        args["campaign_id"] = newCampaignId
        if newCampaignMaxBrokers > 0 { args["max_brokers"] = newCampaignMaxBrokers }

        do {
            planResult = try await MCPClient.shared.callTool("plan_create", arguments: args)
            // The next New Campaign starts empty; a failed create keeps the
            // input for correction (#1316).
            clearFormFields()
            // Refresh the campaign list
            await refresh()
        } catch {
            state = .failed(error.localizedDescription)
        }
        return true
    }

    func executeCampaign(_ campaignId: String) async {
        state = .loading

        do {
            executeResult = try await MCPClient.shared.callTool("execute", arguments: [
                "campaign_id": campaignId
            ])
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func resetForm() {
        clearFormFields()
        planResult = nil
        executeResult = nil
    }

    private func clearFormFields() {
        newCampaignId = ""
        newCampaignJurisdiction = ""
        newCampaignLaw = ""
        newCampaignPriority = ""
        newCampaignMaxBrokers = 30
        matchCount = nil
    }
}
