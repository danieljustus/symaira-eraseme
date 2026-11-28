import Foundation

/// Campaign data from `get_dashboard_data` and `plan_create`.
struct DashboardCampaign: Codable, Identifiable {
    let id: String
    let createdAt: String?
    let kind: String?
    let total: Int
    let planned: Int
    let sent: Int
    let awaitingAck: Int
    let awaitingResponse: Int
    let confirmed: Int
    let rejected: Int
    let overdue: Int
    /// This campaign's authoritative status counts; absent from older payloads.
    let byStatus: [String: Int]?
    let requests: [RemovalRequest]?

    enum CodingKeys: String, CodingKey {
        case id
        case createdAt = "created_at"
        case kind, total, planned, sent
        case awaitingAck = "awaiting_ack"
        case awaitingResponse = "awaiting_response"
        case confirmed, rejected, overdue
        case byStatus = "by_status"
        case requests
    }

    var createdDate: Date? { BackendDate.parse(createdAt) }

    /// Sent and unresolved requests of this campaign ("In Flight").
    var inProgress: Int {
        RequestProgress.inProgress(byStatus: byStatus, fallback: sent + awaitingAck + awaitingResponse)
    }
}

/// Which request statuses count as in progress: sent and still open.
enum RequestProgress {
    /// Not yet sent, resolved, failed to send, or a confirmed request whose
    /// re-scan is due. Every other status, including unknown ones, is open work.
    static let excluded: Set<String> = [
        "PLANNED", "CONFIRMED", "REJECTED", "REJECTED_FINAL", "SEND_FAILED", "BOUNCE", "RE_SCAN_DUE",
    ]

    /// Sums `byStatus` without the excluded statuses. Payloads without a
    /// breakdown fall back to the flat counters, which omit ACK and others.
    static func inProgress(byStatus: [String: Int]?, fallback: Int) -> Int {
        guard let byStatus else { return fallback }
        return byStatus.reduce(0) { sum, entry in
            excluded.contains(entry.key.uppercased()) ? sum : sum + entry.value
        }
    }
}

/// Full dashboard data from `get_dashboard_data`.
struct DashboardData: Codable {
    let campaigns: [DashboardCampaign]
    let totalRequests: Int
    let planned: Int
    let sent: Int
    let awaitingAck: Int
    let awaitingResponse: Int
    let confirmed: Int
    let rejected: Int
    let overdue: Int
    /// Authoritative status counts from the backend. Optional so older or
    /// compact payloads without `by_status` still decode.
    let byStatus: [String: Int]?
    let brokerStatus: [BrokerStatus]
    let recentEvents: [RecentEvent]
    let generatedAt: String

    enum CodingKeys: String, CodingKey {
        case campaigns
        case totalRequests = "total_requests"
        case planned, sent
        case awaitingAck = "awaiting_ack"
        case awaitingResponse = "awaiting_response"
        case confirmed, rejected, overdue
        case byStatus = "by_status"
        case brokerStatus = "broker_status"
        case recentEvents = "recent_events"
        case generatedAt = "generated_at"
    }
}

/// Status summary for a broker in the dashboard.
struct BrokerStatus: Codable, Identifiable {
    var id: String { brokerId }
    let brokerId: String
    let total: Int
    let confirmed: Int
    let rejected: Int
    let overdue: Int
    let pending: Int

    enum CodingKeys: String, CodingKey {
        case brokerId = "broker_id"
        case total, confirmed, rejected, overdue, pending
    }
}

/// A recent event from the dashboard timeline.
struct RecentEvent: Codable, Identifiable {
    var id: Int { eventId }
    let eventId: Int
    let requestId: Int
    let occurredAt: String
    let eventType: String
    let source: String
    let brokerId: String?

    enum CodingKeys: String, CodingKey {
        case eventId = "id"
        case requestId = "request_id"
        case occurredAt = "occurred_at"
        case eventType = "event_type"
        case source
        case brokerId = "broker_id"
    }

    var occurredDate: Date? { BackendDate.parse(occurredAt) }

    /// A failed send is labelled and coloured as an error in the event log.
    var isSendFailure: Bool { eventType.uppercased().contains("SEND_FAILED") }

    /// Plain-language event name for the timeline.
    var displayLabel: String {
        StatusBadge.vocabulary[eventType.uppercased()]?.label
            ?? eventType.replacingOccurrences(of: "_", with: " ").capitalized
    }
}
