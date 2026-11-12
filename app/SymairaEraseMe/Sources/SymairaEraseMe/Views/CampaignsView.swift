import SwiftUI
import SymairaTheme

/// Campaigns management view — list, create, and execute campaigns.
struct CampaignsView: View {
    @StateObject private var vm = CampaignsViewModel()
    @State private var showCreateSheet = false
    @State private var showExecuteAlert = false
    @State private var pendingExecuteId: String?

    var body: some View {
        SymairaPage(title: "Campaigns", subtitle: "Removal campaigns and their progress") {
            Spacer()
            Button {
                showCreateSheet = true
            } label: {
                Label("New Campaign", symairaIcon: .add)
            }
            .symairaButtonStyle(.primary)
            PageRefreshButton(title: "Campaigns", refresh: vm.refresh)
        } content: {
            if let plan = vm.planResult {
                SymairaNotice(message: plan.message ?? "Campaign created", tone: .positive)
            }
            if let exec = vm.executeResult {
                SymairaNotice(message: exec.message ?? "Execution complete", tone: .positive)
            }
            SymairaPageState(state: vm.state.pageState, loading: "Loading campaigns…", retry: { Task { await vm.refresh() } }) { campaigns in
                if campaigns.isEmpty {
                    SymairaPageEmpty(
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
        SymairaSheetScaffold(
            title: "New Campaign",
            dismiss: {
                showCreateSheet = false
                vm.resetForm()
            },
            primaryTitle: "Create Campaign", primaryDisabled: !vm.canCreateCampaign,
            primary: {
                Task {
                    if await vm.createCampaign() { showCreateSheet = false }
                }
            }
        ) {
            SymairaFormSection("Campaign Details") {
                field("Campaign ID", "e.g. q4-gdpr", text: $vm.newCampaignId)
                SymairaFormDivider()
                // Menus offer only registry values; free text matched exactly and
                // silently planned nothing for "eu" or "Germany" (#1318).
                filterRow("Jurisdiction", selection: $vm.newCampaignJurisdiction, values: vm.facets.jurisdictions)
                SymairaFormDivider()
                filterRow("Law", selection: $vm.newCampaignLaw, values: vm.facets.laws)
                SymairaFormDivider()
                filterRow("Priority", selection: $vm.newCampaignPriority, values: vm.facets.priorities)
                SymairaFormDivider()
                SymairaFormRow("Max Brokers") {
                    Stepper(
                        "\(vm.newCampaignMaxBrokers.plainDigits)", value: $vm.newCampaignMaxBrokers, in: 1...500)
                        .symairaText(.bodyEmphasized)
                        .accessibilityLabel("Max Brokers")
                        .accessibilityValue(vm.newCampaignMaxBrokers.plainDigits)
                }
                SymairaFormDivider()
                SymairaFormRow("Matching Brokers") {
                    Text(vm.matchCount.map(\.plainDigits) ?? "—")
                        .symairaText(.bodyEmphasized)
                }
                if vm.matchCount == 0 {
                    Text("No active broker matches these filters. Choose other filters to create a campaign.")
                        .symairaText(.caption, respectsForeground: false)
                        .foregroundStyle(BrandColors.rejected)
                }
            }
        }
        .frame(width: 520, height: 500)
        .task(id: "\(vm.newCampaignJurisdiction)|\(vm.newCampaignLaw)|\(vm.newCampaignPriority)") {
            await vm.refreshMatches()
        }
    }

    private func field(_ label: String, _ prompt: String, text: Binding<String>) -> some View {
        SymairaFormRow(label) {
            TextField(label, text: text, prompt: Text(prompt))
                .labelsHidden()
                .textFieldStyle(.symaira)
                .frame(width: 260)
                // labelsHidden() drops the field's name; VoiceOver read only the placeholder (#1317).
                .accessibilityLabel(label)
        }
    }

    private func filterRow(_ label: String, selection: Binding<String>, values: [String]) -> some View {
        SymairaFormRow(label) {
            SymairaFilterMenu(label, selection: selection, options: RegistryFacets.options(values, all: "Any"))
                .frame(width: 260)
        }
    }


}
