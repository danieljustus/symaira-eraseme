import Foundation

/// Response from `get_calendar`: unresolved requests grouped by the ISO week
/// of their next marker (next action, else deadline) within the horizon.
/// Pinned by `tests/fixtures/event-store/golden-reporting.json` (`calendar`).
struct CalendarData: Codable {
    let schemaVersion: Int?
    let asOf: String?
    let horizonWeeks: Int
    let horizonUntil: String?
    let scope: CalendarScope?
    let totals: CalendarTotals
    let weeks: [CalendarWeek]

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case asOf = "as_of"
        case horizonWeeks = "horizon_weeks"
        case horizonUntil = "horizon_until"
        case scope, totals, weeks
    }
}

struct CalendarScope: Codable {
    let campaignId: String?
    enum CodingKeys: String, CodingKey {
        case campaignId = "campaign_id"
    }
}

struct CalendarTotals: Codable {
    let entries: Int
    let overdue: Int
    let weeksWithActions: Int

    enum CodingKeys: String, CodingKey {
        case entries, overdue
        case weeksWithActions = "weeks_with_actions"
    }
}

/// One ISO week (`2026-W32`) and its entries.
struct CalendarWeek: Codable, Identifiable {
    var id: String { week }
    let week: String
    let entries: [CalendarEntry]
}

/// One request marker in a calendar week.
struct CalendarEntry: Codable, Identifiable {
    var id: String { "\(requestId)-\(marker)" }
    let requestId: Int
    let brokerId: String
    let campaignId: String
    let jurisdiction: String?
    let currentStatus: String?
    /// `next_action` or `deadline`.
    let marker: String
    let markerAt: String
    let daysFromNow: Int
    let overdue: Bool
    let deadlineAt: String?
    let nextActionAt: String?
    let escalationLevel: Int?
    let remindersSent: Int?

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
        case brokerId = "broker_id"
        case campaignId = "campaign_id"
        case jurisdiction
        case currentStatus = "current_status"
        case marker
        case markerAt = "marker_at"
        case daysFromNow = "days_from_now"
        case overdue
        case deadlineAt = "deadline_at"
        case nextActionAt = "next_action_at"
        case escalationLevel = "escalation_level"
        case remindersSent = "reminders_sent"
    }

    /// The backend's escalation category key for `escalation_level`
    /// (0 none, 1 reminder, 2 DPA pending), as named in the status report.
    var escalationKey: String {
        switch escalationLevel ?? 0 {
        case 0: return "none"
        case 1: return "reminder"
        case 2: return "dpa_pending"
        case let level: return "level \(level)"
        }
    }

    /// "today", "in 3 days", "2 days overdue".
    var relativeDay: String {
        switch daysFromNow {
        case 0: return "today"
        case 1: return "in 1 day"
        case -1: return "1 day overdue"
        case let days where days > 1: return "in \(days) days"
        case let days: return "\(-days) days overdue"
        }
    }
}
