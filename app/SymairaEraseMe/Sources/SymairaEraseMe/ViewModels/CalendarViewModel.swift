import Foundation

/// View model for the Calendar view.
@MainActor
final class CalendarViewModel: ObservableObject {
    @Published var state: ViewState<CalendarData> = .idle

    @Published var weeks: Int = 4

    var calendarData: CalendarData? { state.value }
    var isLoading: Bool { state.isLoading }
    var errorMessage: String? { state.errorMessage }

    func refresh() async {
        state = .loading

        let args: [String: Any] = ["weeks": weeks]
        do {
            let data: CalendarData = try await MCPClient.shared.callTool("get_calendar", arguments: args)
            state = .loaded(data)
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    var tickActions: [TickAction] { calendarData?.tickActions ?? [] }
    var deadlines: UpcomingDeadlines? { calendarData?.upcomingDeadlines }
}
