import Foundation

/// View model for the Manual Tasks view.
@MainActor
final class ManualTasksViewModel: ObservableObject {
    @Published var tasks: [ManualTask] = []
    @Published var isLoading = false
    @Published var errorMessage: String?
    @Published var successMessage: String?

    /// Filter by status.
    @Published var filterStatus: String = ""

    var pendingTasks: Int { tasks.filter { $0.status == "pending" }.count }
    var completedTasks: Int { tasks.filter { $0.status == "completed" }.count }

    func refresh() async {
        isLoading = true
        errorMessage = nil
        successMessage = nil
        defer { isLoading = false }

        var args: [String: Any] = [:]
        if !filterStatus.isEmpty { args["status"] = filterStatus }

        do {
            let response: ManualTaskListResponse = try await MCPClient.shared.callTool("manual_tasks_list", arguments: args)
            tasks = response.tasks
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func completeTask(_ task: ManualTask, notes: String = "") async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        do {
            let result = try await MCPClient.shared.callToolRaw("manual_tasks_complete", arguments: [
                "task_id": task.taskId,
                "notes": notes
            ])
            if let msg = result["message"] as? String {
                successMessage = msg
            }
            await refresh()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
