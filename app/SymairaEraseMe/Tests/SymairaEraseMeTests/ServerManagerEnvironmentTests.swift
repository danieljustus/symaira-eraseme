import XCTest
@testable import SymairaEraseMe

/// Regression tests for #1322: the launched server learns which process owns
/// it, so it can exit when the app ends without a normal quit.
final class ServerManagerEnvironmentTests: XCTestCase {
    func testServerEnvironmentNamesTheAppAsParent() {
        let env = ServerManager.serverEnvironment(dataDir: nil, anthropicKey: "", parentPID: 4242)
        XCTAssertEqual(env["SYMERASEME_MCP_PARENT_PID"], "4242")
        XCTAssertNil(env["SYMERASEME_DATA_DIR"])
        XCTAssertNil(env["ANTHROPIC_API_KEY"])
    }

    func testServerEnvironmentKeepsDataDirAndKey() {
        let env = ServerManager.serverEnvironment(
            dataDir: "/tmp/eraseme-data", anthropicKey: "synthetic-key", parentPID: 7)
        XCTAssertEqual(env["SYMERASEME_DATA_DIR"], "/tmp/eraseme-data")
        XCTAssertEqual(env["ANTHROPIC_API_KEY"], "synthetic-key")
        XCTAssertEqual(env["SYMERASEME_MCP_PARENT_PID"], "7")
    }
}
