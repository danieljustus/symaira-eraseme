import SwiftUI
import SymairaTheme

/// Campaigns management view — list, create, and execute campaigns.
struct CampaignsView: View {
    @StateObject private var vm = CampaignsViewModel()
    @State private var showCreateSheet = false
    @State private var showExecuteAlert = false
    @State private var pendingExecuteId: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                if let plan = vm.planResult {
                    SymairaNotice(message: plan.message ?? "Campaign created", tone: .positive)
                }
                if let exec = vm.executeResult {
                    SymairaNotice(message: exec.message ?? "Execution complete", tone: .positive)
                }

                switch vm.state {
                case .idle, .loading:
                    SymairaLoadingState("Loading campaigns…")
                        .frame(height: 200)

                case .failed(let message):
                    ErrorStateView(message: message) {
                        Task { await vm.refresh() }
                    }
                    .frame(height: 200)

                case .loaded(let campaigns):
                    if campaigns.isEmpty {
                        SymairaEmptyState(
                            systemImage: "megaphone.fill",
                            title: "No Campaigns",
                            message: "Create your first campaign to start removing data from brokers."
                        )
                        .frame(height: 200)
                    } else {
                        campaignsTable(campaigns)
                    }
                }
            }
            .padding(24)
        }
        .background(Color.clear)
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

    private var header: some View {
        HStack {
            Text("Campaigns")
                .symairaText(.display)
                .foregroundStyle(BrandColors.textPrimary)
            Spacer()
            Button {
                showCreateSheet = true
            } label: {
                Label("New Campaign", symairaIcon: .add)
            }
            .symairaButtonStyle(.primary)
        }
    }

    private func campaignsTable(_ campaigns: [DashboardCampaign]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Table(campaigns) {
                TableColumn("ID") { Text($0.id).foregroundStyle(BrandColors.goldPrimary) }
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
        NavigationStack {
            Form {
                Section("Campaign Details") {
                    TextField("Campaign ID", text: $vm.newCampaignId)
                        .textFieldStyle(.symaira)
                    TextField("Jurisdiction (optional)", text: $vm.newCampaignJurisdiction)
                        .textFieldStyle(.symaira)
                    TextField("Law (optional)", text: $vm.newCampaignLaw)
                        .textFieldStyle(.symaira)
                    TextField("Priority (optional)", text: $vm.newCampaignPriority)
                        .textFieldStyle(.symaira)
                    Stepper("Max Brokers: \(vm.newCampaignMaxBrokers.plainDigits)", value: $vm.newCampaignMaxBrokers, in: 1...500)
                }

                Section {
                    Button("Create Campaign") {
                        Task {
                            await vm.createCampaign()
                            showCreateSheet = false
                        }
                    }
                    .disabled(!vm.canCreateCampaign)
                    .symairaButtonStyle(.primary)
                }
            }
            .formStyle(.grouped)
            .background(BrandColors.bgDark)
            .navigationTitle("New Campaign")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        showCreateSheet = false
                        vm.resetForm()
                    }
                    .keyboardShortcut(.cancelAction)
                }
            }
        }
        .frame(width: 500, height: 460)
        .onExitCommand {
            showCreateSheet = false
            vm.resetForm()
        }
    }


}
