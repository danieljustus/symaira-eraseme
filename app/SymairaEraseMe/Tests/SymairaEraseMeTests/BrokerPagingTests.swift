import XCTest
@testable import SymairaEraseMe

/// #1342: the MCP tool answers with a bounded page; the app's catalogue loader
/// must request full records, walk every cursor and keep the server filters.
final class BrokerPagingTests: XCTestCase {
    private func brokerJSON(_ id: String) -> String {
        """
        {"id":"\(id)","name":"Broker \(id)","website":"https://example.invalid",\
        "category":"other","jurisdictions":["DE"],"laws":["GDPR"],\
        "data_sensitivity":3,"priority":"high","opt_out":[],"status":"active"}
        """
    }

    private func pageJSON(_ ids: [String], total: Int, next: String?) throws -> BrokerListResponse {
        let brokers = ids.map(brokerJSON).joined(separator: ",")
        let nextValue = next.map { "\"\($0)\"" } ?? "null"
        let json = """
        {"brokers":[\(brokers)],"count":\(ids.count),"total":\(total),\
        "next_cursor":\(nextValue),"detail":"full",\
        "filters":{"include_disabled":false,"status":"active"},"schema_version":1}
        """
        return try JSONDecoder().decode(BrokerListResponse.self, from: Data(json.utf8))
    }

    func testPagedEnvelopeDecodesTotalAndCursor() throws {
        let page = try pageJSON(["b1", "b2"], total: 1273, next: "brokers:2")
        XCTAssertEqual(page.count, 2)
        XCTAssertEqual(page.total, 1273)
        XCTAssertEqual(page.nextCursor, "brokers:2")
        XCTAssertEqual(page.detail, "full")
        XCTAssertEqual(page.brokers.map(\.brokerId), ["b1", "b2"])
    }

    func testCatalogLoadsEveryPageAndPassesCursorBack() async throws {
        let pages = [
            try pageJSON(["b1", "b2"], total: 1273, next: "brokers:500"),
            try pageJSON(["b3"], total: 1273, next: "brokers:1000"),
            try pageJSON(["b4"], total: 1273, next: nil),
        ]
        var requests: [[String: Any]] = []
        let loaded = try await BrokerCatalog.load(arguments: ["jurisdiction": "DE"]) { arguments in
            requests.append(arguments)
            return pages[requests.count - 1]
        }

        XCTAssertEqual(loaded.total, 1273)
        XCTAssertEqual(loaded.brokers.map(\.brokerId), ["b1", "b2", "b3", "b4"])
        XCTAssertEqual(requests.count, 3)
        XCTAssertEqual(requests[0]["detail"] as? String, "full")
        XCTAssertEqual(requests[0]["limit"] as? Int, BrokerCatalog.pageSize)
        XCTAssertEqual(requests[0]["jurisdiction"] as? String, "DE")
        XCTAssertNil(requests[0]["cursor"])
        XCTAssertEqual(requests[1]["cursor"] as? String, "brokers:500")
        XCTAssertEqual(requests[2]["cursor"] as? String, "brokers:1000")
    }

    func testEmptyPageEndsImmediately() async throws {
        let empty = try pageJSON([], total: 0, next: nil)
        var requestCount = 0
        let loaded = try await BrokerCatalog.load { _ in
            requestCount += 1
            return empty
        }
        XCTAssertEqual(loaded.total, 0)
        XCTAssertTrue(loaded.brokers.isEmpty)
        XCTAssertEqual(requestCount, 1)
    }

    func testRepeatedCursorIsRejected() async throws {
        let page = try pageJSON(["b1"], total: 1273, next: "brokers:1")
        do {
            _ = try await BrokerCatalog.load { _ in page }
            XCTFail("a repeated cursor must not loop forever")
        } catch BrokerCatalogError.repeatedCursor(let cursor) {
            XCTAssertEqual(cursor, "brokers:1")
        }
    }
}
