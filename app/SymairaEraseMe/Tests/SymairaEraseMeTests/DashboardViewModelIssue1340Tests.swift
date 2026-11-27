import XCTest
import SymairaTheme
@testable import SymairaEraseMe

/// #1340/#1341: the dashboard must render the backend's *authoritative*
/// `by_status` projection, including statuses the flat counters omit and
/// statuses this app version has never seen.
@MainActor
final class DashboardViewModelIssue1340Tests: XCTestCase {
    private func payload(total: Int, byStatus: String, events: String = "[]") -> Data {
        Data("""
        {"campaigns":[],"total_requests":\(total),"planned":4,"sent":0,
         "awaiting_ack":5,"awaiting_response":0,"confirmed":0,"rejected":0,
         "overdue":5,"by_status":\(byStatus),"broker_status":[],
         "recent_events":\(events),"generated_at":"2026-11-27T10:00:00+00:00"}
        """.utf8)
    }

    private func viewModel(_ data: Data) throws -> DashboardViewModel {
        let decoded = try JSONDecoder().decode(DashboardData.self, from: data)
        let vm = DashboardViewModel()
        vm.state = .loaded(decoded)
        return vm
    }

    func testLiveStatusProjectionShowsSendFailedAndSumsToTotal() throws {
        let vm = try viewModel(payload(
            total: 14,
            byStatus: #"{"PLANNED":4,"AWAITING_ACK":5,"SEND_FAILED":5}"#))
        XCTAssertEqual(vm.failedSends, 5)
        let breakdown = vm.statusBreakdown
        XCTAssertEqual(breakdown.map(\.status), ["PLANNED", "AWAITING_ACK", "SEND_FAILED"])
        XCTAssertEqual(breakdown.map(\.label), [
            "Planned", "Awaiting confirmation of receipt", "Send Failed",
        ])
        XCTAssertEqual(breakdown.reduce(0) { $0 + $1.count }, 14)
        XCTAssertFalse(breakdown.contains { $0.status == "OVERDUE" },
                       "Overdue is an attention flag, not a status bar")
        XCTAssertEqual(vm.overdue, 5)
    }

    func testUnknownStatusIsDisplayedAndCounted() throws {
        let vm = try viewModel(payload(
            total: 10,
            byStatus: #"{"PLANNED":4,"SEND_FAILED":5,"FUTURE_REVIEW_STATE":1}"#))
        let row = try XCTUnwrap(vm.statusBreakdown.first { $0.status == "FUTURE_REVIEW_STATE" })
        XCTAssertEqual(row.count, 1)
        XCTAssertEqual(row.label, "Future Review State")
        XCTAssertEqual(vm.statusBreakdown.reduce(0) { $0 + $1.count }, 10)
    }

    func testSummaryTilesIncludeSendFailedAndAttentionFlags() throws {
        let vm = try viewModel(payload(
            total: 14,
            byStatus: #"{"PLANNED":4,"AWAITING_ACK":5,"SEND_FAILED":5}"#))
        XCTAssertEqual(vm.summaryTiles.map(\.id), [
            "total", "inProgress", "confirmed", "rejected", "sendFailed", "overdue",
        ])
        let failed = try XCTUnwrap(vm.summaryTiles.first { $0.id == "sendFailed" })
        XCTAssertEqual(failed.title, "Send Failed")
        XCTAssertEqual(failed.value, 5)
        XCTAssertEqual(failed.statusFilter, "SEND_FAILED")
        XCTAssertEqual(failed.detail, "needs attention")
        let overdue = try XCTUnwrap(vm.summaryTiles.first { $0.id == "overdue" })
        XCTAssertTrue(overdue.isAttention)
        XCTAssertEqual(overdue.value, 5)
    }

    func testLegacyPayloadStillBuildsABreakdown() throws {
        let data = Data("""
        {"campaigns":[],"total_requests":2,"planned":1,"sent":0,
         "awaiting_ack":1,"awaiting_response":0,"confirmed":0,"rejected":0,
         "overdue":1,"broker_status":[],"recent_events":[],
         "generated_at":"2026-11-27T10:00:00+00:00"}
        """.utf8)
        let vm = try viewModel(data)
        XCTAssertNil(vm.data?.byStatus)
        XCTAssertEqual(vm.statusBreakdown.reduce(0) { $0 + $1.count }, 2)
        XCTAssertEqual(vm.failedSends, 0)
    }

    func testDashboardEventParsesSQLiteAndZoneLessTimestamps() throws {
        let events = """
        [{"id":1,"request_id":9,"occurred_at":"2026-09-18 17:15:46",\
        "event_type":"send_failed","source":"system","broker_id":"people-search"},\
        {"id":2,"request_id":9,"occurred_at":"2026-06-29T13:00:27",\
        "event_type":"planned","source":"system","broker_id":"people-search"}]
        """
        let vm = try viewModel(payload(total: 0, byStatus: "{}", events: events))
        let decoded = try XCTUnwrap(vm.data?.recentEvents)
        XCTAssertEqual(decoded.count, 2)

        var utc = Calendar(identifier: .gregorian)
        utc.timeZone = TimeZone(secondsFromGMT: 0)!
        let first = try XCTUnwrap(decoded[0].occurredDate)
        XCTAssertEqual(utc.component(.year, from: first), 2026)
        XCTAssertEqual(utc.component(.month, from: first), 9)
        XCTAssertEqual(utc.component(.day, from: first), 18)
        XCTAssertEqual(utc.component(.hour, from: first), 17)
        XCTAssertEqual(utc.component(.minute, from: first), 15)
        XCTAssertEqual(utc.component(.second, from: first), 46)
        let second = try XCTUnwrap(decoded[1].occurredDate)
        XCTAssertEqual(utc.component(.hour, from: second), 13)
        XCTAssertEqual(utc.component(.minute, from: second), 0)
        XCTAssertTrue(decoded[0].isSendFailure)
        XCTAssertFalse(decoded[1].isSendFailure)
        XCTAssertEqual(decoded[0].displayLabel, "Send Failed")
    }

    func testRequestEventParsesBothTimestampForms() throws {
        let data = Data("""
        {"request_id":9,"events":[\
        {"id":1,"request_id":9,"occurred_at":"2026-09-18 17:15:46",\
        "recorded_at":"2026-09-18T17:15:46+00:00","event_type":"send_failed",\
        "payload_json":{},"source":"system"},\
        {"id":2,"request_id":9,"occurred_at":"2026-06-29T13:00:27",\
        "event_type":"planned","payload_json":{},"source":"system"}]}
        """.utf8)
        let response = try JSONDecoder().decode(EventListResponse.self, from: data)
        XCTAssertNotNil(response.events[0].occurredDate)
        XCTAssertNotNil(response.events[1].occurredDate)
        XCTAssertTrue(response.events[0].isSendFailure)
    }
}
