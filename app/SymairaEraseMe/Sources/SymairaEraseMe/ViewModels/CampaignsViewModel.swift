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

    var campaigns: [DashboardCampaign] { state.value ?? [] }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    /// Whether the Create Campaign button may be used: the campaign id must
    /// be non-empty after trimming whitespace (issue #642).
    var canCreateCampaign: Bool {
        !newCampaignId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
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

    func createCampaign() async {
        state = .loading

        var args: [String: Any] = ["campaign_id": newCampaignId]
        if !newCampaignJurisdiction.isEmpty { args["jurisdiction"] = newCampaignJurisdiction }
        if !newCampaignLaw.isEmpty { args["law"] = newCampaignLaw }
        if !newCampaignPriority.isEmpty { args["priority"] = newCampaignPriority }
        if newCampaignMaxBrokers > 0 { args["max_brokers"] = newCampaignMaxBrokers }

        do {
            planResult = try await MCPClient.shared.callTool("plan_create", arguments: args)
            // Refresh the campaign list
            await refresh()
        } catch {
            state = .failed(error.localizedDescription)
        }
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
        newCampaignId = ""
        newCampaignJurisdiction = ""
        newCampaignLaw = ""
        newCampaignPriority = ""
        newCampaignMaxBrokers = 30
        planResult = nil
        executeResult = nil
    }
}
