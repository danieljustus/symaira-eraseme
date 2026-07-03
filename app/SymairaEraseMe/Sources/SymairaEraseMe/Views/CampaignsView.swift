import SwiftUI

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

                if let error = vm.errorMessage {
                    ErrorBanner(message: error) { vm.errorMessage = nil }
                }
                if let plan = vm.planResult {
                    successBanner(plan.message ?? "Campaign created")
                }
                if let exec = vm.executeResult {
                    successBanner(exec.message ?? "Execution complete")
                }

                if vm.isLoading && vm.campaigns.isEmpty {
                    LoadingOverlay(message: "Loading campaigns…")
                        .frame(height: 200)
                } else if vm.campaigns.isEmpty {
                    EmptyStateView(
                        icon: "megaphone.fill",
                        title: "No Campaigns",
                        message: "Create your first campaign to start removing data from brokers."
                    )
                    .frame(height: 200)
                } else {
                    campaignsTable
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
                .font(.largeTitle.bold())
                .foregroundStyle(BrandColors.textPrimary)
            Spacer()
            Button {
                showCreateSheet = true
            } label: {
                Label("New Campaign", systemImage: "plus")
            }
            .buttonStyle(.borderedProminent)
            .tint(BrandColors.goldPrimary)
            .foregroundStyle(BrandColors.bgDark)
        }
    }

    private var campaignsTable: some View {
        VStack(alignment: .leading, spacing: 8) {
            Table(vm.campaigns) {
                TableColumn("ID") { Text($0.id).foregroundStyle(BrandColors.goldPrimary) }
                TableColumn("Kind") { Text($0.kind ?? "—") }
                TableColumn("Total") { Text("\($0.total)") }
                TableColumn("Confirmed") { Text("\($0.confirmed)").foregroundStyle(BrandColors.confirmed) }
                TableColumn("Rejected") { Text("\($0.rejected)").foregroundStyle(BrandColors.rejected) }
                TableColumn("Overdue") { Text("\($0.overdue)").foregroundStyle(BrandColors.overdue) }
                TableColumn("Actions") { campaign in
                    Button("Execute") {
                        pendingExecuteId = campaign.id
                        showExecuteAlert = true
                    }
                    .buttonStyle(.bordered)
                    .tint(BrandColors.goldPrimary)
                    .controlSize(.small)
                }
            }
            .frame(minHeight: 200)
        }
        .interactiveGlassCard()
    }


    private var createSheet: some View {
        NavigationStack {
            Form {
                Section("Campaign Details") {
                    TextField("Campaign ID", text: $vm.newCampaignId)
                        .textFieldStyle(.roundedBorder)
                    TextField("Jurisdiction (optional)", text: $vm.newCampaignJurisdiction)
                        .textFieldStyle(.roundedBorder)
                    TextField("Law (optional)", text: $vm.newCampaignLaw)
                        .textFieldStyle(.roundedBorder)
                    TextField("Priority (optional)", text: $vm.newCampaignPriority)
                        .textFieldStyle(.roundedBorder)
                    Stepper("Max Brokers: \(vm.newCampaignMaxBrokers)", value: $vm.newCampaignMaxBrokers, in: 1...500)
                }

                Section {
                    Button("Create Campaign") {
                        Task {
                            await vm.createCampaign()
                            showCreateSheet = false
                        }
                    }
                    .disabled(vm.newCampaignId.isEmpty)
                    .tint(BrandColors.goldPrimary)
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
                }
            }
        }
        .frame(width: 450, height: 380)
    }

    private func successBanner(_ message: String) -> some View {
        HStack {
            Image(systemName: "checkmark.circle.fill")
                .foregroundStyle(BrandColors.confirmed)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(BrandColors.textPrimary)
            Spacer()
        }
        .padding(10)
        .background(
            RoundedRectangle(cornerRadius: 8)
                .fill(BrandColors.confirmedBg)
        )
    }
}
