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
        SheetScaffold(
            title: "Complete Task",
            dismiss: {
                showCompleteSheet = false
                selectedTask = nil
            },
            primaryTitle: "Mark Complete", primaryDisabled: selectedTask == nil,
            primary: {
                if let task = selectedTask {
                    Task {
                        await vm.completeTask(task, notes: completionNotes)
                        showCompleteSheet = false
                        selectedTask = nil
                    }
                }
            }
        ) {
            if let task = selectedTask {
                SymairaFormSection("Task Details") {
                    SymairaFormRow("Broker") { Text(task.brokerName).symairaText(.callout) }
                    SymairaFormDivider()
                    SymairaFormRow("Reason") { Text(task.reason).symairaText(.callout) }
                    SymairaFormDivider()
                    SymairaFormRow("Form") {
                        Link(task.formUrl, destination: URL(string: task.formUrl) ?? URL(string: "about:blank")!)
                            .symairaText(.caption)
                            .lineLimit(1)
                    }
                    if !task.instructions.isEmpty {
                        SymairaFormDivider()
                        Text(task.instructions)
                            .symairaText(.callout)
                            .foregroundStyle(BrandColors.textSecondary)
                    }
                }
            }
            SymairaFormSection("Completion Notes") {
                TextField("Completion notes", text: $completionNotes, prompt: Text("Optional"), axis: .vertical)
                    .labelsHidden()
                    .lineLimit(3...6)
                    .textFieldStyle(.symaira)
            }
        }
        .frame(width: 520, height: 500)
    }
}
