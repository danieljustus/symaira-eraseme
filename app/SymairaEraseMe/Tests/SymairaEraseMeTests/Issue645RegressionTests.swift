import XCTest
@testable import SymairaEraseMe

/// Regression tests for #645: the Settings "Test Connection" indicator must
/// stay in sync with the sidebar footer's reachability indicator after the
/// server stops. `SettingsViewModel.isConnected` is derived from
/// `ServerManager.mcpReachable` — the single source of truth refreshed by
/// the shared 10-second poll — so the two indicators can never contradict
/// each other, and no manual "Test Connection" click is needed after a stop.
final class Issue645RegressionTests: XCTestCase {

    @MainActor
    func testIsConnectedTracksServerManagerReachability() {
        let serverManager = ServerManager()
        let vm = SettingsViewModel(serverManager: serverManager)

        // Fresh instance: nothing reachable yet.
        XCTAssertFalse(vm.isConnected)

        // Mirrors a poll observing the running server.
        serverManager.mcpReachable = true
        XCTAssertTrue(
            vm.isConnected,
            "Settings indicator must follow the shared reachability source of truth"
        )

        // Mirrors the poll after the server has been stopped: the sidebar
        // footer and the Settings indicator must flip together, without a
        // manual Test Connection click.
        serverManager.mcpReachable = false
        XCTAssertFalse(
            vm.isConnected,
            "Settings indicator must reset when reachability drops"
        )
    }

    @MainActor
    func testCheckConnectionUpdatesDerivedIndicatorViaSharedRefresh() async {
        let serverManager = ServerManager()
        let vm = SettingsViewModel(serverManager: serverManager)

        serverManager.mcpReachable = false
        XCTAssertFalse(vm.isConnected)

        // checkConnection delegates to the shared reachability refresh, so
        // its outcome must be reflected by the derived indicator.
        await vm.checkConnection()
        XCTAssertEqual(
            vm.isConnected,
            serverManager.mcpReachable,
            "checkConnection must leave both indicators in agreement"
        )
    }
}
