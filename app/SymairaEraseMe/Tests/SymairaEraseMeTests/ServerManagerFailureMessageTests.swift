import XCTest
@testable import SymairaEraseMe

/// Regression tests for #617: the server failure banner must show the real
/// stderr text and error, not the literal `\(stderr)` escape sequence that
/// the double-escaped string produced.
final class ServerManagerFailureMessageTests: XCTestCase {

    func testFailureMessageContainsStderrWhenPresent() {
        let stderr = "ModuleNotFoundError: No module named 'symeraseme'"
        let message = ServerManager.failureMessage(stderr: stderr, error: "server exited with code 1")
        XCTAssertTrue(message.contains(stderr), "message should contain the real stderr text")
        XCTAssertTrue(message.contains("server exited with code 1"))
        XCTAssertFalse(message.contains(#"\("#), "message must not contain a literal interpolation escape")
    }

    func testFailureMessageFallsBackToErrorAloneWithoutStderr() {
        XCTAssertEqual(ServerManager.failureMessage(stderr: nil, error: "server exited"), "server exited")
        XCTAssertEqual(ServerManager.failureMessage(stderr: "", error: "server exited"), "server exited")
        XCTAssertFalse(ServerManager.failureMessage(stderr: "", error: "server exited").contains(#"\("#))
    }
}
