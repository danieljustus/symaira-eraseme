import XCTest
@testable import SymairaEraseMe

/// Regression tests for #642: a blank or whitespace-only campaign id must not
/// be accepted in the New Campaign sheet — the Create Campaign button stays
/// disabled until the trimmed Campaign ID field is non-empty.
final class CampaignCreateValidationTests: XCTestCase {

    @MainActor
    func testCreateDisabledForEmptyCampaignId() {
        let vm = CampaignsViewModel()
        vm.newCampaignId = ""
        XCTAssertFalse(vm.canCreateCampaign)
    }

    @MainActor
    func testCreateDisabledForWhitespaceOnlyCampaignId() {
        let vm = CampaignsViewModel()
        vm.newCampaignId = "   "
        XCTAssertFalse(vm.canCreateCampaign)
    }

    @MainActor
    func testCreateEnabledForNonBlankCampaignId() {
        let vm = CampaignsViewModel()
        vm.newCampaignId = "initial-2026-Q2"
        XCTAssertTrue(vm.canCreateCampaign)
    }

    @MainActor
    func testCreateEnabledForCampaignIdWithSurroundingWhitespace() {
        let vm = CampaignsViewModel()
        vm.newCampaignId = "  initial-2026-Q2  "
        XCTAssertTrue(vm.canCreateCampaign)
    }
}
