import Foundation

/// Identity profile from `get_profile`.
struct IdentityProfile: Codable {
    let fullName: String
    let nameVariants: [String]
    let dateOfBirth: String?
    let addresses: [ProfileAddress]
    let emailAddresses: [String]
    let phoneNumbers: [String]
    let jurisdictions: [String]

    enum CodingKeys: String, CodingKey {
        case fullName = "full_name"
        case nameVariants = "name_variants"
        case dateOfBirth = "date_of_birth"
        case addresses
        case emailAddresses = "email_addresses"
        case phoneNumbers = "phone_numbers"
        case jurisdictions
    }
}

struct ProfileAddress: Codable {
    let street: String
    let city: String
    let postalCode: String
    let country: String
    let state: String?
    let validFrom: String?
    let validTo: String?

    enum CodingKeys: String, CodingKey {
        case street, city
        case postalCode = "postal_code"
        case country, state
        case validFrom = "valid_from"
        case validTo = "valid_to"
    }
}

/// Response from `plan_create`.
struct PlanCreateResponse: Codable {
    let message: String?
    let campaignId: String?
    let totalBrokers: Int?
    let planned: Int?
    let requests: [PlanRequest]?
    /// Brokers left out because the campaign already has a request for them;
    /// absent when none were.
    let skipped: Int?

    enum CodingKeys: String, CodingKey {
        case message
        case campaignId = "campaign_id"
        case totalBrokers = "total_brokers"
        case planned, requests, skipped
    }

    /// The notice after planning: how many requests were planned and, for a
    /// repeated plan, how many brokers were already in the campaign.
    var summary: String {
        if let message { return message }
        guard let planned else { return "Campaign created" }
        let base = "Planned \(planned) request\(planned == 1 ? "" : "s")"
        guard let skipped, skipped > 0 else { return base }
        return "\(base); \(skipped) broker\(skipped == 1 ? " was" : "s were") already in the campaign"
    }
}

struct PlanRequest: Codable, Identifiable {
    var id: Int { requestId }
    let requestId: Int
    let brokerId: String
    let brokerName: String
    let channel: String
    let template: String?

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
        case brokerId = "broker_id"
        case brokerName = "broker_name"
        case channel, template
    }
}

/// Response from `execute`.
struct ExecuteResponse: Codable {
    let message: String?
    let campaignId: String?
    let totalPlanned: Int?
    let batchSize: Int?
    let results: [ExecuteResult]?

    enum CodingKeys: String, CodingKey {
        case message
        case campaignId = "campaign_id"
        case totalPlanned = "total_planned"
        case batchSize = "batch_size"
        case results
    }
}

struct ExecuteResult: Codable, Identifiable {
    var id: Int { requestId }
    let requestId: Int
    let success: Bool
    let dryRun: Bool?
    let error: String?
    let to: String?

    enum CodingKeys: String, CodingKey {
        case requestId = "request_id"
        case success
        case dryRun = "dry_run"
        case error, to
    }
}

