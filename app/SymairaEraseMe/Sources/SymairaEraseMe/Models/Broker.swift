import Foundation

/// A data broker from `list_brokers`.
struct Broker: Codable, Identifiable {
    var id: String { brokerId }
    let brokerId: String
    let name: String
    let website: String?
    let category: String?
    let jurisdictions: [String]?
    let laws: [String]?
    let dataSensitivity: Int?
    let priority: String?
    let optOut: [BrokerOptOut]?
    let verification: BrokerVerification?
    let disabled: Bool?
    let notes: String?

    enum CodingKeys: String, CodingKey {
        case brokerId = "id"
        case name, website, category, jurisdictions, laws
        case dataSensitivity = "data_sensitivity"
        case priority
        case optOut = "opt_out"
        case verification, disabled, notes
    }
}

/// Opt-out channel for a broker.
struct BrokerOptOut: Codable {
    let type: String // "email" or "web_form"
    let endpoint: String?
    let url: String?
    let template: String?
    let locale: String?
    let requiredFields: [String]?
    let supportsSuppression: Bool?
    let expectedResponseDays: Int?

    enum CodingKeys: String, CodingKey {
        case type, endpoint, url, template, locale
        case requiredFields = "required_fields"
        case supportsSuppression = "supports_suppression"
        case expectedResponseDays = "expected_response_days"
    }
}

/// Verification keywords for a broker.
struct BrokerVerification: Codable {
    let ackKeywords: [String]?
    let rejectionKeywords: [String]?
    let humanRequiredKeywords: [String]?

    enum CodingKeys: String, CodingKey {
        case ackKeywords = "ack_keywords"
        case rejectionKeywords = "rejection_keywords"
        case humanRequiredKeywords = "human_required_keywords"
    }
}

/// Response from `list_brokers`.
struct BrokerListResponse: Codable {
    let brokers: [Broker]
    let total: Int
}
