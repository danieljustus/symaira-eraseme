import XCTest
@testable import SymairaEraseMe

/// "In Progress" (tile) and "In Flight" (campaign column) must count every
/// sent, unresolved request. The flat counters only cover SENT, AWAITING_ACK
/// and AWAITING_RESPONSE, so acknowledged requests used to vanish from both.
@MainActor
final class DashboardInProgressTests: XCTestCase {
    private let breakdown = #"{"PLANNED":114,"ACK":289,"AWAITING_ACK":580,"CONFIRMED":115,"REJECTED_FINAL":50}"#

    private func dashboard(byStatus: String?) throws -> DashboardData {
        let status = byStatus.map { #","by_status":\#($0)"# } ?? ""
        let json = """
        {"campaigns":[{"id":"us","kind":"initial","total":1148,"planned":114,"sent":0,
          "awaiting_ack":580,"awaiting_response":0,"confirmed":115,"rejected":50,"overdue":214\(status)}],
         "total_requests":1148,"planned":114,"sent":0,"awaiting_ack":580,"awaiting_response":0,
         "confirmed":115,"rejected":50,"overdue":214\(status),"broker_status":[],"recent_events":[],
         "generated_at":"2026-11-28T00:54:00+00:00"}
        """
        return try JSONDecoder().decode(DashboardData.self, from: Data(json.utf8))
    }

    func testAcknowledgedRequestsCountAsInProgress() throws {
        let data = try dashboard(byStatus: breakdown)
        let vm = DashboardViewModel()
        vm.state = .loaded(data)
        XCTAssertEqual(vm.inProgress, 289 + 580)
        XCTAssertEqual(data.campaigns[0].inProgress, 289 + 580)
        // The status tiles now add up to the total again.
        XCTAssertEqual(vm.planned + vm.inProgress + vm.confirmed + vm.rejected + vm.failedSends, 1148)
    }

    func testFailedAndRescanStatesAreNotInProgress() {
        let counts = ["SENT": 1, "SEND_FAILED": 2, "BOUNCE": 3, "RE_SCAN_DUE": 4, "OVERDUE": 5, "ESCALATED": 6]
        XCTAssertEqual(RequestProgress.inProgress(byStatus: counts, fallback: 0), 1 + 5 + 6)
    }

    /// Tables show the short pill; the chart, tooltip and VoiceOver keep the
    /// plain-language label from #1340.
    func testTablesUseTheCompactLabelOnlyWhereOneExists() {
        XCTAssertEqual(StatusBadge.label(for: "AWAITING_ACK", compact: true), "Awaiting Receipt")
        XCTAssertEqual(StatusBadge.label(for: "awaiting_ack", compact: false), "Awaiting confirmation of receipt")
        XCTAssertEqual(StatusBadge.label(for: "ACK", compact: true), "Acknowledged")
        XCTAssertEqual(StatusBadge.label(for: "FUTURE_STATE", compact: true), "FUTURE_STATE")
    }

    func testPayloadWithoutBreakdownKeepsTheFlatCounters() throws {
        let data = try dashboard(byStatus: nil)
        let vm = DashboardViewModel()
        vm.state = .loaded(data)
        XCTAssertEqual(vm.inProgress, 580)
        XCTAssertEqual(data.campaigns[0].inProgress, 580)
    }
}
