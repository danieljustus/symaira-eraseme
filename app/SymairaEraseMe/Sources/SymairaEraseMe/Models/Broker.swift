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
    let count: Int
}

/// The filter values the registry actually uses. The server matches filters
/// exactly, so menus built from these never offer a value that matches no
/// broker (#1313, #1318).
struct RegistryFacets: Equatable {
    var jurisdictions: [String] = []
    var laws: [String] = []
    var categories: [String] = []
    var priorities: [String] = []

    init() {}

    init(_ brokers: [Broker]) {
        jurisdictions = Set(brokers.flatMap { $0.jurisdictions ?? [] }).sorted()
        laws = Set(brokers.flatMap { $0.laws ?? [] }).sorted()
        categories = Set(brokers.compactMap(\.category)).sorted()
        let rank = ["high": 0, "medium": 1, "low": 2]
        priorities = Set(brokers.compactMap(\.priority)).sorted {
            (rank[$0] ?? rank.count, $0) < (rank[$1] ?? rank.count, $1)
        }
    }

    /// Menu options: the unfiltered default first, then every registry value.
    static func options(_ values: [String], all: String) -> [(value: String, label: String)] {
        [("", all)] + values.map { ($0, displayName($0)) }
    }

    /// Codes stay as they are ("EU", "CCPA"); slugs read as words ("People Search").
    static func displayName(_ value: String) -> String {
        if value == value.uppercased() { return value }
        return value.split(separator: "-").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }
}
