import Foundation

/// View model for the Manual Tasks view.
@MainActor
final class ManualTasksViewModel: ObservableObject {
    @Published var state: ViewState<[ManualTask]> = .idle
    @Published var successMessage: String?

    /// Filter by status.
    @Published var filterStatus: String = ""

    var tasks: [ManualTask] { state.value ?? [] }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }
    var pendingTasks: Int { tasks.filter { $0.status == "pending" }.count }
    var completedTasks: Int { tasks.filter { $0.status == "completed" }.count }

    func refresh() async {
        state = .loading
        successMessage = nil

        var args: [String: Any] = [:]
        if !filterStatus.isEmpty { args["status"] = filterStatus }

        do {
            let response: ManualTaskListResponse = try await MCPClient.shared.callTool("manual_tasks_list", arguments: args)
            state = .loaded(response.tasks)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func completeTask(_ task: ManualTask, notes: String = "") async {
        state = .loading

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
            state = .failed(error.localizedDescription)
        }
    }
}
