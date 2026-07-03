import Foundation

/// A removal request — used by `list_requests`, `plan_show`, and dashboard campaigns.
struct RemovalRequest: Codable, Identifiable {
    let id: Int
    let brokerId: String
    let channel: String
    let campaignId: String
    let createdAt: String
    let jurisdiction: String
    let templateId: String
    let identitySnapshotHash: String?
    let currentStatus: String?
    let lastEventAt: String?
    let sentAt: String?
    let acknowledgedAt: String?
    let resolvedAt: String?
    let deadlineAt: String?
    let nextActionAt: String?
    let remindersSent: Int?
    let escalationLevel: Int?

    enum CodingKeys: String, CodingKey {
        case id
        case brokerId = "broker_id"
        case channel
        case campaignId = "campaign_id"
        case createdAt = "created_at"
        case jurisdiction
        case templateId = "template_id"
        case identitySnapshotHash = "identity_snapshot_hash"
        case currentStatus = "current_status"
        case lastEventAt = "last_event_at"
        case sentAt = "sent_at"
        case acknowledgedAt = "acknowledged_at"
        case resolvedAt = "resolved_at"
        case deadlineAt = "deadline_at"
        case nextActionAt = "next_action_at"
        case remindersSent = "reminders_sent"
        case escalationLevel = "escalation_level"
    }

    var createdDate: Date? { ISO8601DateFormatter().date(from: createdAt) }
    var deadlineDate: Date? { deadlineAt.flatMap { ISO8601DateFormatter().date(from: $0) } }
    var statusDisplay: String { currentStatus ?? "UNKNOWN" }
}

/// Paginated response from `list_requests`.
struct RequestListResponse: Codable {
    let page: Int
    let pageSize: Int
    let total: Int
    let items: [RemovalRequest]

    enum CodingKeys: String, CodingKey {
        case page
        case pageSize = "page_size"
        case total, items
    }
}
