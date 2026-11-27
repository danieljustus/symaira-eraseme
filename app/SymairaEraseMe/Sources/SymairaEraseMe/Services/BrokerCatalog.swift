import Foundation

/// #1342: `list_brokers` now answers with a bounded page. The app still needs
/// the whole registry (detail sheet plus registry facets), so it walks the
/// opaque cursor with full records and a server-side page size of 500.
enum BrokerCatalogError: LocalizedError {
    case repeatedCursor(String)

    var errorDescription: String? {
        switch self {
        case .repeatedCursor(let cursor):
            return "The broker registry paging returned the same cursor twice: \(cursor)"
        }
    }
}

enum BrokerCatalog {
    /// Within the MCP server's 1..500 limit (#1342).
    static let pageSize = 500

    struct Loaded {
        let brokers: [Broker]
        /// Number matching the filters, as reported by the server.
        let total: Int
    }

    /// Injectable so the paging path itself can be tested without a server.
    typealias PageLoader = ([String: Any]) async throws -> BrokerListResponse

    static func load(
        arguments: [String: Any] = [:],
        loader: PageLoader = { arguments in
            try await MCPClient.shared.callTool("list_brokers", arguments: arguments)
        }
    ) async throws -> Loaded {
        var brokers: [Broker] = []
        var total = 0
        var cursor: String?
        var seenCursors = Set<String>()

        while true {
            var pageArguments = arguments
            pageArguments["detail"] = "full"
            pageArguments["limit"] = pageSize
            if let cursor { pageArguments["cursor"] = cursor }

            let page = try await loader(pageArguments)
            if brokers.isEmpty {
                total = page.total
            }
            brokers.append(contentsOf: page.brokers)

            guard let next = page.nextCursor, !next.isEmpty else { break }
            guard seenCursors.insert(next).inserted else {
                throw BrokerCatalogError.repeatedCursor(next)
            }
            cursor = next
        }

        return Loaded(brokers: brokers, total: total)
    }
}
