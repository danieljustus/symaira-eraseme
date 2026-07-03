import Foundation
import SwiftUI

/// View model for the Campaigns view.
@MainActor
final class CampaignsViewModel: ObservableObject {
    @Published var campaigns: [DashboardCampaign] = []
    @Published var isLoading = false
    @Published var errorMessage: String?
    @Published var planResult: PlanCreateResponse?
    @Published var executeResult: ExecuteResponse?

    /// New campaign form fields.
    @Published var newCampaignId: String = ""
    @Published var newCampaignJurisdiction: String = ""
    @Published var newCampaignLaw: String = ""
    @Published var newCampaignPriority: String = ""
    @Published var newCampaignMaxBrokers: Int = 30

    func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            let dashboardData: DashboardData = try await MCPClient.shared.callTool("get_dashboard_data")
            campaigns = dashboardData.campaigns
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func createCampaign() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

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
            errorMessage = error.localizedDescription
        }
    }

    func executeCampaign(_ campaignId: String) async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            executeResult = try await MCPClient.shared.callTool("execute", arguments: [
                "campaign_id": campaignId
            ])
        } catch {
            errorMessage = error.localizedDescription
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
