import XCTest
@testable import SymairaEraseMe

/// The app models must decode the payloads the Rust server actually answers.
/// The inputs are the repository's contract fixtures, which the server's own
/// tests replay byte for byte, so a server-side shape change breaks here too
/// instead of surfacing as "Decoding error" in the running app (#1310, #1311).
final class BackendContractDecodingTests: XCTestCase {
    private static let repoRoot = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // SymairaEraseMeTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // SymairaEraseMe
        .deletingLastPathComponent() // app
        .deletingLastPathComponent() // repository root

    private struct StoreCases: Decodable {
        struct Case: Decodable {
            let name: String
            let response: String?
        }
        let cases: [Case]
    }

    /// The tool payload inside a recorded successful `tools/call` response;
    /// `nil` for a recorded JSON-RPC error (rejected arguments).
    private func toolPayload(_ response: String) throws -> Data? {
        let envelope = try JSONDecoder().decode(JSONRPCResponse.self, from: Data(response.utf8))
        return envelope.result?.content.first.map { Data($0.text.utf8) }
    }

    private func listRequestsCases(_ file: String) throws -> [(String, Data)] {
        let url = Self.repoRoot.appendingPathComponent("tests/fixtures/mcp-contract/mcp-003-store/\(file)")
        let fixture = try JSONDecoder().decode(StoreCases.self, from: Data(contentsOf: url))
        return try fixture.cases
            .filter { $0.name.hasPrefix("list_requests") }
            .compactMap { item in try toolPayload(XCTUnwrap(item.response)).map { (item.name, $0) } }
    }

    func testListRequestsPagesDecode() throws {
        let cases = try listRequestsCases("seeded-cases.json")
        XCTAssertGreaterThanOrEqual(cases.count, 4)
        for (name, payload) in cases {
            let decoded = try JSONDecoder().decode(RequestListResponse.self, from: payload)
            let raw = try XCTUnwrap(JSONSerialization.jsonObject(with: payload) as? [String: Any])
            XCTAssertEqual(decoded.items.count, (raw["requests"] as? [Any])?.count ?? 0, name)
            XCTAssertEqual(decoded.total, raw["total"] as? Int, name)
        }

        let first = try JSONDecoder().decode(RequestListResponse.self, from: XCTUnwrap(cases.first?.1))
        let request = try XCTUnwrap(first.items.first)
        XCTAssertEqual(request.brokerId, "broker-a")
        XCTAssertEqual(request.currentStatus, "SENT")
        XCTAssertNotNil(request.createdDate)
        XCTAssertNotNil(request.deadlineDate)
    }

    func testEmptyListRequestsWithNullRowsDecodesAsEmptyPage() throws {
        let cases = try listRequestsCases("empty-cases.json")
        XCTAssertFalse(cases.isEmpty)
        for (name, payload) in cases {
            let decoded = try JSONDecoder().decode(RequestListResponse.self, from: payload)
            XCTAssertEqual(decoded.items.count, 0, name)
            XCTAssertEqual(decoded.total, 0, name)
        }
    }

    func testCalendarDecodesTheGoldenReport() throws {
        let url = Self.repoRoot.appendingPathComponent("tests/fixtures/event-store/golden-reporting.json")
        let golden = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any])
        let payload = try JSONSerialization.data(withJSONObject: XCTUnwrap(golden["calendar"]))

        let calendar = try JSONDecoder().decode(CalendarData.self, from: payload)
        XCTAssertEqual(calendar.horizonWeeks, 4)
        XCTAssertEqual(calendar.totals.entries, 1)
        XCTAssertEqual(calendar.totals.overdue, 1)
        XCTAssertEqual(calendar.weeks.map(\.week), ["2026-W32"])
        let entry = try XCTUnwrap(calendar.weeks.first?.entries.first)
        XCTAssertEqual(entry.requestId, 2)
        XCTAssertEqual(entry.marker, "next_action")
        XCTAssertTrue(entry.overdue)
        // 27 hours before `as_of`: one whole day has passed, although the
        // floored wire value `days_from_now` is -2 (#1367).
        XCTAssertEqual(entry.daysFromNow, -2)
        XCTAssertEqual(entry.relativeDay(asOf: calendar.asOf), "1 day overdue")
        XCTAssertEqual(entry.relativeDay(asOf: nil), "2 days overdue", "without as_of the wire value is used")
        XCTAssertEqual(CalendarView.escalationLabel(for: entry.escalationKey), "DPA pending")
        XCTAssertEqual(CalendarView.markerLabel(for: entry.marker), "Next action")
        XCTAssertNotEqual(CalendarView.markerDate(entry.markerAt), entry.markerAt, "RFC 3339 marker must parse")
        XCTAssertNotEqual(CalendarView.markerDate("2026-08-14 09:00:00"), "2026-08-14 09:00:00",
                          "SQLite-layout marker must parse")
        XCTAssertEqual(CalendarView.markerDate("2026-08-14 09:00:00"),
                       CalendarView.markerDate("2026-08-14T09:00:00Z"),
                       "SQLite-layout markers are UTC")
        XCTAssertEqual(CalendarView.markerDate("not a timestamp"), "not a timestamp")
    }

    func testOverdueLabelCountsElapsedWholeDays() throws {
        func entry(_ markerAt: String, _ days: Int, overdue: Bool = true) throws -> CalendarEntry {
            let json = """
            {"request_id": 1, "broker_id": "b", "campaign_id": "c", "marker": "deadline",
             "marker_at": "\(markerAt)", "days_from_now": \(days), "overdue": \(overdue)}
            """
            return try JSONDecoder().decode(CalendarEntry.self, from: Data(json.utf8))
        }
        let asOf = "2026-11-28T01:55:00+00:00"
        XCTAssertEqual(try entry("2026-11-15 01:45:00", -14).relativeDay(asOf: asOf), "13 days overdue")
        XCTAssertEqual(try entry("2026-11-26T01:55:00Z", -2).relativeDay(asOf: asOf), "2 days overdue")
        XCTAssertEqual(try entry("2026-11-28 01:45:00", -1).relativeDay(asOf: asOf), "overdue today")
        XCTAssertEqual(try entry("2026-11-28T12:00:00Z", 0, overdue: false).relativeDay(asOf: asOf), "today")
        XCTAssertEqual(try entry("2026-11-30T12:00:00Z", 2, overdue: false).relativeDay(asOf: asOf), "in 2 days")
    }

    func testPlanCreateSummaryNamesSkippedBrokers() throws {
        func decode(_ json: String) throws -> PlanCreateResponse {
            try JSONDecoder().decode(PlanCreateResponse.self, from: Data(json.utf8))
        }
        let first = try decode(#"{"campaign_id":"c","total_brokers":3,"matched":3,"planned":2,"requests":[]}"#)
        XCTAssertNil(first.skipped)
        XCTAssertEqual(first.summary, "Planned 2 requests")
        let repeated = try decode(#"{"campaign_id":"c","total_brokers":3,"matched":3,"planned":0,"requests":null,"skipped":2}"#)
        XCTAssertEqual(repeated.summary, "Planned 0 requests; 2 brokers were already in the campaign")
        let one = try decode(#"{"planned":1,"skipped":1}"#)
        XCTAssertEqual(one.summary, "Planned 1 request; 1 broker was already in the campaign")
    }

    func testEmptyCalendarDecodes() throws {
        // Shape of a live answer over a store without open markers.
        let payload = Data("""
        {"as_of":"2026-11-12T18:43:17+00:00","horizon_until":"2026-12-10T18:43:17+00:00",\
        "horizon_weeks":4,"schema_version":1,"scope":{"campaign_id":"all"},\
        "totals":{"entries":0,"overdue":0,"weeks_with_actions":0},"weeks":[]}
        """.utf8)
        let calendar = try JSONDecoder().decode(CalendarData.self, from: payload)
        XCTAssertTrue(calendar.weeks.isEmpty)
        XCTAssertEqual(calendar.totals.weeksWithActions, 0)
    }
}
