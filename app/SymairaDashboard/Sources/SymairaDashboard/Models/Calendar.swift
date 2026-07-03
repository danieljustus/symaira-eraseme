import Foundation

/// Response from `get_calendar`.
struct CalendarData: Codable {
    let weeks: Int
    let upcomingDeadlines: UpcomingDeadlines?
    let tickActions: [TickAction]?

    enum CodingKeys: String, CodingKey {
        case weeks
        case upcomingDeadlines = "upcoming_deadlines"
        case tickActions = "tick_actions"
    }
}

/// Aggregated deadline summary.
struct UpcomingDeadlines: Codable {
    let schemaVersion: Int?
    let asOf: String?
    let scope: CalendarScope?
    let totals: DeadlineTotals?
    let byStatus: [String: Int]?
    let byChannel: [String: Int]?
    let escalation: [String: Int]?
    let upcoming: UpcomingCounts?

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case asOf = "as_of"
        case scope, totals
        case byStatus = "by_status"
        case byChannel = "by_channel"
        case escalation, upcoming
    }
}

struct CalendarScope: Codable {
    let campaignId: String?
    enum CodingKeys: String, CodingKey {
        case campaignId = "campaign_id"
    }
}

struct DeadlineTotals: Codable {
    let requests: Int?
    let resolved: Int?
    let open: Int?
}

struct UpcomingCounts: Codable {
    let overdue: Int?
    let deadlineDueWithin7d: Int?
    let deadlineDueWithin30d: Int?
    let tickActionsReady: Int?

    enum CodingKeys: String, CodingKey {
        case overdue
        case deadlineDueWithin7d = "deadline_due_within_7d"
        case deadlineDueWithin30d = "deadline_due_within_30d"
        case tickActionsReady = "tick_actions_ready"
    }
}

/// A tick action to be taken by the scheduler.
struct TickAction: Codable, Identifiable {
    var id: String { "\(requestId)-\(actionType)" }
    let requestId: Int
    let brokerId: String
    let campaignId: String
    let currentStatus: String
    let actionType: String
    let eventType: String
    let description: String
    let payload: [String: AnyCodable]?
    let dryRun: Bool?

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
        case brokerId = "broker_id"
        case campaignId = "campaign_id"
        case currentStatus = "current_status"
        case actionType = "action_type"
        case eventType = "event_type"
        case description, payload
        case dryRun = "dry_run"
    }
}
