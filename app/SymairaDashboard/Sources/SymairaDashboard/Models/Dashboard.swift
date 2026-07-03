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
    let requests: [RemovalRequest]?

    enum CodingKeys: String, CodingKey {
        case id
        case createdAt = "created_at"
        case kind, total, planned, sent
        case awaitingAck = "awaiting_ack"
        case awaitingResponse = "awaiting_response"
        case confirmed, rejected, overdue
        case requests
    }

    var createdDate: Date? {
        guard let createdAt else { return nil }
        return ISO8601DateFormatter().date(from: createdAt)
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

    var occurredDate: Date? {
        ISO8601DateFormatter().date(from: occurredAt)
    }
}
