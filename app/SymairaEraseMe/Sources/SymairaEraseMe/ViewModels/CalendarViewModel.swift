import Foundation

/// View model for the Calendar view.
@MainActor
final class CalendarViewModel: ObservableObject {
    @Published var calendarData: CalendarData?
    @Published var isLoading = false
    @Published var errorMessage: String?

    @Published var weeks: Int = 4

    func refresh() async {
        isLoading = true
        errorMessage = nil
        defer { isLoading = false }

        let args: [String: Any] = ["weeks": weeks]
        do {
            calendarData = try await MCPClient.shared.callTool("get_calendar", arguments: args)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var tickActions: [TickAction] { calendarData?.tickActions ?? [] }
    var deadlines: UpcomingDeadlines? { calendarData?.upcomingDeadlines }
}
