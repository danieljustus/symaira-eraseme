import SwiftUI

/// Manual tasks view — list and complete tasks requiring human intervention.
struct ManualTasksView: View {
    @StateObject private var vm = ManualTasksViewModel()
    @State private var showCompleteSheet = false
    @State private var selectedTask: ManualTask?
    @State private var completionNotes: String = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                if let error = vm.errorMessage {
                    ErrorBanner(message: error) { vm.errorMessage = nil }
                }
                if let success = vm.successMessage {
                    HStack {
                        Image(systemName: "checkmark.circle.fill")
                            .foregroundStyle(BrandColors.confirmed)
                        Text(success)
                            .font(.subheadline)
                            .foregroundStyle(BrandColors.textPrimary)
                        Spacer()
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 8).fill(BrandColors.confirmedBg))
                }

                summaryCards

                if vm.isLoading && vm.tasks.isEmpty {
                    LoadingOverlay(message: "Loading tasks…")
                        .frame(height: 200)
                } else if vm.tasks.isEmpty {
                    EmptyStateView(
                        icon: "checklist",
                        title: "No Manual Tasks",
                        message: "All broker forms are handled automatically. Manual tasks appear when automation fails."
                    )
                    .frame(height: 200)
                } else {
                    tasksTable
                }
            }
            .padding(24)
        }
        .background(Color.clear)
        .task { await vm.refresh() }
        .sheet(isPresented: $showCompleteSheet) {
            completeSheet
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Manual Tasks")
                    .font(.largeTitle.bold())
                    .foregroundStyle(BrandColors.textPrimary)
                Text("Tasks requiring human intervention")
                    .font(.caption)
                    .foregroundStyle(BrandColors.textMuted)
            }
            Spacer()
            Picker("Filter", selection: $vm.filterStatus) {
                Text("All").tag("")
                Text("Pending").tag("pending")
                Text("Completed").tag("completed")
            }
            .pickerStyle(.segmented)
            .frame(width: 200)
            .onChange(of: vm.filterStatus) { _, _ in
                Task { await vm.refresh() }
            }
            Button {
                Task { await vm.refresh() }
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.title3)
            }
            .buttonStyle(.plain)
            .foregroundStyle(BrandColors.goldPrimary)
        }
    }

    private var summaryCards: some View {
        LazyVGrid(columns: [
            GridItem(.flexible()),
            GridItem(.flexible()),
        ], spacing: 12) {
            StatCard(title: "Pending", value: vm.pendingTasks, color: BrandColors.pending)
            StatCard(title: "Completed", value: vm.completedTasks, color: BrandColors.confirmed)
        }
    }

    private var tasksTable: some View {
        VStack(alignment: .leading, spacing: 8) {
            Table(vm.tasks) {
                TableColumn("ID") { Text("\($0.taskId)") }
                TableColumn("Broker") { Text($0.brokerName).foregroundStyle(BrandColors.goldPrimary) }
                TableColumn("Reason") { Text($0.reason.replacingOccurrences(of: "_", with: " ").capitalized) }
                TableColumn("Status") {
                    StatusBadge(status: $0.status.uppercased() == "PENDING" ? "SENT" : "CONFIRMED")
                }
                TableColumn("Created") {
                    if let date = $0.createdDate {
                        Text(date.formatted(.dateTime.month().day()))
                    }
                }
                TableColumn("Actions") { task in
                    if task.status == "pending" {
                        Button("Complete") {
                            selectedTask = task
                            completionNotes = ""
                            showCompleteSheet = true
                        }
                        .buttonStyle(.bordered)
                        .tint(BrandColors.confirmed)
                        .controlSize(.small)
                    } else {
                        Text("Done")
                            .font(.caption)
                            .foregroundStyle(BrandColors.textMuted)
                    }
                }
            }
            .frame(minHeight: 150)
        }
        .interactiveGlassCard()
    }

    private var completeSheet: some View {
        NavigationStack {
            Form {
                Section("Task Details") {
                    if let task = selectedTask {
                        LabeledContent("Broker", value: task.brokerName)
                        LabeledContent("Reason", value: task.reason)
                        LabeledContent("URL") {
                            Link(task.formUrl, destination: URL(string: task.formUrl) ?? URL(string: "about:blank")!)
                                .font(.caption)
                        }
                        if !task.instructions.isEmpty {
                            Text(task.instructions)
                                .font(.subheadline)
                                .foregroundStyle(BrandColors.textSecondary)
                        }
                    }
                }

                Section("Completion Notes") {
                    TextEditor(text: $completionNotes)
                        .frame(height: 80)
                        .textFieldStyle(.roundedBorder)
                }

                Section {
                    Button("Mark Complete") {
                        if let task = selectedTask {
                            Task {
                                await vm.completeTask(task, notes: completionNotes)
                                showCompleteSheet = false
                                selectedTask = nil
                            }
                        }
                    }
                    .tint(BrandColors.confirmed)
                }
            }
            .formStyle(.grouped)
            .background(BrandColors.bgDark)
            .navigationTitle("Complete Task")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        showCompleteSheet = false
                        selectedTask = nil
                    }
                }
            }
        }
        .frame(width: 500, height: 450)
    }
}
