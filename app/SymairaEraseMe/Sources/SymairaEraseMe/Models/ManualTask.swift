import Foundation

/// A manual task requiring human intervention.
struct ManualTask: Codable, Identifiable {
    var id: Int { taskId }
    let taskId: Int
    let requestId: Int?
    let brokerId: String
    let brokerName: String
    let formUrl: String
    let reason: String
    let instructions: String
    let screenshotPath: String?
    let htmlSnapshotPath: String?
    let formFieldsJson: String?
    let status: String
    let createdAt: String
    let completedAt: String?
    let notes: String?

    enum CodingKeys: String, CodingKey {
        case taskId = "id"
        case requestId = "request_id"
        case brokerId = "broker_id"
        case brokerName = "broker_name"
        case formUrl = "form_url"
        case reason, instructions
        case screenshotPath = "screenshot_path"
        case htmlSnapshotPath = "html_snapshot_path"
        case formFieldsJson = "form_fields_json"
        case status
        case createdAt = "created_at"
        case completedAt = "completed_at"
        case notes
    }

    var createdDate: Date? { ISO8601DateFormatter().date(from: createdAt) }
}

/// Response from `manual_tasks_list`.
struct ManualTaskListResponse: Codable {
    let tasks: [ManualTask]
}
