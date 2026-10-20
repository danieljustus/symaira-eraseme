import SwiftUI
import SymairaTheme

/// Manual tasks view — list and complete tasks requiring human intervention.
struct ManualTasksView: View {
    @StateObject private var vm = ManualTasksViewModel()
    @State private var showCompleteSheet = false
    @State private var selectedTask: ManualTask?
    @State private var completionNotes: String = ""

    var body: some View {
        Page(title: "Manual Tasks", subtitle: "Tasks requiring human intervention", refresh: vm.refresh) {
            SymairaSegmentedControl(
                "Filter", selection: $vm.filterStatus,
                options: [("", "All"), ("pending", "Pending"), ("completed", "Completed")])
        } content: {
            if let success = vm.successMessage {
                SymairaNotice(message: success, tone: .positive)
            }
            summaryCards
            PageState(state: vm.state, loading: "Loading tasks…", retry: vm.refresh) { tasks in
                if tasks.isEmpty {
                    PageEmpty(
                        icon: .checklist,
                        title: "No Manual Tasks",
                        message: "No matching tasks. Broker web forms and verification steps are handled here manually."
                    )
                } else {
                    tasksTable
                }
            }
        }
        .task(id: vm.filterStatus) { await vm.refresh() }
        .sheet(isPresented: $showCompleteSheet) {
            completeSheet
        }
    }

    private var summaryCards: some View {
        LazyVGrid(columns: [
            GridItem(.flexible()),
            GridItem(.flexible()),
        ], spacing: SymairaSpacing.medium) {
            StatCard(title: "Pending", value: vm.pendingTasks)
            StatCard(title: "Completed", value: vm.completedTasks)
        }
    }

    private var tasksTable: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Table(vm.tasks) {
                TableColumn("ID") { Text("\($0.taskId.plainDigits)") }
                TableColumn("Broker") { Text($0.brokerName) }
                TableColumn("Reason") { Text($0.reason.replacingOccurrences(of: "_", with: " ").capitalized) }
                TableColumn("Status") {
                    StatusBadge(status: $0.status)
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
                        .symairaButtonStyle(.secondary)
                        .controlSize(.small)
                    } else {
                        Text("Done")
                            .symairaText(.caption)
                            .foregroundStyle(BrandColors.textMuted)
                    }
                }
            }
            .frame(minHeight: 150)
        }
        .symairaCard()
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
                                .symairaText(.caption)
                        }
                        if !task.instructions.isEmpty {
                            Text(task.instructions)
                                .symairaText(.callout)
                                .foregroundStyle(BrandColors.textSecondary)
                        }
                    }
                }

                Section("Completion Notes") {
                    TextEditor(text: $completionNotes)
                        .frame(height: 80)
                        .textFieldStyle(.symaira)
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
                    .symairaButtonStyle(.primary)
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
                    .keyboardShortcut(.cancelAction)
                }
            }
        }
        .frame(width: 500, height: 450)
        .onExitCommand {
            showCompleteSheet = false
            selectedTask = nil
        }
    }
}
