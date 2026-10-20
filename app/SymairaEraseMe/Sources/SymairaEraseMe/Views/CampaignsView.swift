import SwiftUI
import SymairaTheme

/// Campaigns management view — list, create, and execute campaigns.
struct CampaignsView: View {
    @StateObject private var vm = CampaignsViewModel()
    @State private var showCreateSheet = false
    @State private var showExecuteAlert = false
    @State private var pendingExecuteId: String?

    var body: some View {
        Page(title: "Campaigns", subtitle: "Removal campaigns and their progress", refresh: vm.refresh) {
            Spacer()
            Button {
                showCreateSheet = true
            } label: {
                Label("New Campaign", symairaIcon: .add)
            }
            .symairaButtonStyle(.primary)
        } content: {
            if let plan = vm.planResult {
                SymairaNotice(message: plan.message ?? "Campaign created", tone: .positive)
            }
            if let exec = vm.executeResult {
                SymairaNotice(message: exec.message ?? "Execution complete", tone: .positive)
            }
            PageState(state: vm.state, loading: "Loading campaigns…", retry: vm.refresh) { campaigns in
                if campaigns.isEmpty {
                    PageEmpty(
                        icon: .announcement,
                        title: "No Campaigns",
                        message: "Create your first campaign to start removing data from brokers."
                    )
                } else {
                    campaignsTable(campaigns)
                }
            }
        }
        .task { await vm.refresh() }
        .sheet(isPresented: $showCreateSheet) {
            createSheet
        }
        .alert("Execute Campaign", isPresented: $showExecuteAlert) {
            Button("Cancel", role: .cancel) { pendingExecuteId = nil }
            Button("Execute") {
                if let id = pendingExecuteId {
                    Task {
                        await vm.executeCampaign(id)
                        pendingExecuteId = nil
                    }
                }
            }
        } message: {
            if let id = pendingExecuteId {
                Text("This will send opt-out requests for campaign \"\(id)\". Are you sure you want to proceed?")
            }
        }
    }

    private func campaignsTable(_ campaigns: [DashboardCampaign]) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Table(campaigns) {
                TableColumn("ID") { Text($0.id) }
                TableColumn("Kind") { Text($0.kind ?? "—") }
                TableColumn("Total") { Text("\($0.total.plainDigits)") }
                TableColumn("Confirmed") { Text("\($0.confirmed.plainDigits)").foregroundStyle(BrandColors.confirmed) }
                TableColumn("Rejected") { Text("\($0.rejected.plainDigits)").foregroundStyle(BrandColors.rejected) }
                TableColumn("Overdue") { Text("\($0.overdue.plainDigits)").foregroundStyle(BrandColors.overdue) }
                TableColumn("Actions") { campaign in
                    Button("Execute") {
                        pendingExecuteId = campaign.id
                        showExecuteAlert = true
                    }
                    .symairaButtonStyle(.secondary)
                    .controlSize(.small)
                }
            }
            .frame(minHeight: 200)
        }
        .symairaCard()
    }


    private var createSheet: some View {
        SheetScaffold(
            title: "New Campaign",
            dismiss: {
                showCreateSheet = false
                vm.resetForm()
            },
            primaryTitle: "Create Campaign", primaryDisabled: !vm.canCreateCampaign,
            primary: {
                Task {
                    await vm.createCampaign()
                    showCreateSheet = false
                }
            }
        ) {
            SymairaFormSection("Campaign Details") {
                field("Campaign ID", "e.g. q4-gdpr", text: $vm.newCampaignId)
                SymairaFormDivider()
                field("Jurisdiction", "Optional", text: $vm.newCampaignJurisdiction)
                SymairaFormDivider()
                field("Law", "Optional", text: $vm.newCampaignLaw)
                SymairaFormDivider()
                field("Priority", "Optional", text: $vm.newCampaignPriority)
                SymairaFormDivider()
                SymairaFormRow("Max Brokers") {
                    Stepper(
                        "\(vm.newCampaignMaxBrokers.plainDigits)", value: $vm.newCampaignMaxBrokers, in: 1...500)
                        .symairaText(.bodyEmphasized)
                }
            }
        }
        .frame(width: 520, height: 500)
    }

    private func field(_ label: String, _ prompt: String, text: Binding<String>) -> some View {
        SymairaFormRow(label) {
            TextField(label, text: text, prompt: Text(prompt))
                .labelsHidden()
                .textFieldStyle(.symaira)
                .frame(width: 260)
        }
    }


}
