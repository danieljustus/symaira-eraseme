import Foundation

/// An event recorded against a removal request.
struct RequestEvent: Codable, Identifiable {
    var id: Int { eventId }
    let eventId: Int
    let requestId: Int
    let occurredAt: String
    let recordedAt: String?
    let eventType: String
    let payloadJson: [String: AnyCodable]?
    let source: String

    enum CodingKeys: String, CodingKey {
        case eventId = "id"
        case requestId = "request_id"
        case occurredAt = "occurred_at"
        case recordedAt = "recorded_at"
        case eventType = "event_type"
        case payloadJson = "payload_json"
        case source
    }

    var occurredDate: Date? { ISO8601DateFormatter().date(from: occurredAt) }
}

/// Response from `get_events`.
struct EventListResponse: Codable {
    let requestId: Int
    let events: [RequestEvent]

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
        case events
    }
}
