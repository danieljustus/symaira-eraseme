import SwiftUI
import SymairaTheme

/// Calendar view: open requests grouped by the ISO week of their next
/// action or deadline within the selected horizon.
struct CalendarView: View {
    @StateObject private var vm = CalendarViewModel()

    var body: some View {
        SymairaPage(title: "Calendar", subtitle: "Upcoming deadlines and next actions") {
            SymairaSegmentedControl(
                "Weeks", selection: $vm.weeks,
                options: [(2, "2 weeks"), (4, "4 weeks"), (8, "8 weeks"), (12, "12 weeks")])
            .frame(width: 420)
            Spacer()
            PageRefreshButton(title: "Calendar", refresh: vm.refresh)
        } content: {
            SymairaPageState(state: vm.state.pageState, loading: "Loading calendar…", retry: { Task { await vm.refresh() } }) { data in
                totalsSummary(data.totals)
                if data.weeks.isEmpty {
                    SymairaPageEmpty(
                        icon: .calendar,
                        title: "Nothing Due",
                        message: "No open request has a deadline or next action in the next \(data.horizonWeeks.plainDigits) weeks."
                    )
                } else {
                    ForEach(data.weeks) { week in
                        weekSection(week)
                    }
                }
            }
        }
        .task(id: vm.weeks) { await vm.refresh() }
    }

    /// Human-readable label for an escalation category identifier returned
    /// by the reporting backend. Unknown identifiers fall back to
    /// the raw key so the UI never crashes on a new backend category.
    static func escalationLabel(for key: String) -> String {
        switch key {
        case "dpa_pending":
            return "DPA pending"
        case "reminder":
            return "Reminder"
        case "none", "":
            return "No escalation"
        default:
            return key
        }
    }

    /// Readable marker kind: `next_action` -> "Next action".
    static func markerLabel(for marker: String) -> String {
        switch marker {
        case "next_action": return "Next action"
        case "deadline": return "Deadline"
        default: return marker.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    /// Short date for a marker as stored: RFC 3339 or SQLite's UTC
    /// `yyyy-MM-dd HH:mm:ss`; the raw value when neither parses.
    static func markerDate(_ value: String) -> String {
        let sqlite = DateFormatter()
        sqlite.locale = Locale(identifier: "en_US_POSIX")
        sqlite.timeZone = TimeZone(identifier: "UTC")
        sqlite.dateFormat = "yyyy-MM-dd HH:mm:ss"
        guard let date = ISO8601DateFormatter().date(from: value) ?? sqlite.date(from: value) else {
            return value
        }
        return date.formatted(.dateTime.day().month(.abbreviated).hour().minute())
    }

    private func totalsSummary(_ totals: CalendarTotals) -> some View {
        LazyVGrid(columns: [
            GridItem(.flexible()),
            GridItem(.flexible()),
            GridItem(.flexible()),
        ], spacing: SymairaSpacing.medium) {
            StatCard(title: "Entries", value: totals.entries)
            StatCard(title: "Overdue", value: totals.overdue)
            StatCard(title: "Weeks With Actions", value: totals.weeksWithActions)
        }
    }

    private func weekSection(_ week: CalendarWeek) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text(week.week)
                .symairaText(.subheading)
                .foregroundStyle(BrandColors.textPrimary)
            Table(week.entries) {
                TableColumn("Request") { Text("\($0.requestId.plainDigits)") }
                    .width(64)
                TableColumn("Broker") { Text($0.brokerId) }
                TableColumn("Due") { entry in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(Self.markerDate(entry.markerAt))
                        Text("\(Self.markerLabel(for: entry.marker)) · \(entry.relativeDay)")
                            .symairaText(.caption)
                            .foregroundStyle(entry.overdue ? BrandColors.overdue : BrandColors.textMuted)
                    }
                }
                .width(min: 190)
                TableColumn("Status") { StatusBadge(status: $0.currentStatus.flatMap { $0.isEmpty ? nil : $0 } ?? "UNKNOWN") }
                    .width(min: 90, ideal: 110)
                TableColumn("Escalation") { Text(Self.escalationLabel(for: $0.escalationKey)) }
                    .width(min: 110)
            }
            .frame(minHeight: 100, idealHeight: CGFloat(min(week.entries.count, 8)) * 40 + 40)
        }
        .symairaCard()
    }
}
