import XCTest
import AppKit
import Darwin
@testable import SymairaEraseMe

/// Regression tests for #1294: persisted server address validation,
/// MCPClient address publication, auto-start and shutdown of the child.
final class ServerManagerSupervisionTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!
    private var previousHost = ""
    private var previousPort = 0

    @MainActor
    override func setUp() {
        super.setUp()
        suiteName = "com.symaira.eraseme.supervision-test.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)
        previousHost = MCPClient.configuredHost
        previousPort = MCPClient.configuredPort
    }

    @MainActor
    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
        MCPClient.configuredHost = previousHost
        MCPClient.configuredPort = previousPort
        super.tearDown()
    }

    @MainActor
    private func makeManager() -> ServerManager {
        ServerManager(resourceURL: nil, executableURL: nil, apiKeyStore: MemoryStore(), defaults: defaults)
    }

    private final class MemoryStore: APIKeyStore, @unchecked Sendable {
        func read() throws -> String? { nil }
        func save(_ value: String) throws {}
        func delete() throws {}
    }

    // MARK: - Validation

    func testInvalidHostsFallBackToDefault() {
        for bad in ["0", "", "   ", "1234", "1.2.3", "-bad.example", "bad..host", "has space", "a_b.example", nil] {
            XCTAssertEqual(ServerManager.validatedHost(bad), "127.0.0.1", "\(bad ?? "nil") must fall back")
        }
    }

    func testValidHostsArePreserved() {
        for good in ["127.0.0.1", "::1", "localhost", "0.0.0.0", "example.com", "my-host.local", "fe80::1"] {
            XCTAssertEqual(ServerManager.validatedHost(good), good)
        }
    }

    func testOutOfRangePortsFallBackToDefault() {
        for bad in [0, -1, 65536, 70000] as [Int?] + [nil] {
            XCTAssertEqual(ServerManager.validatedPort(bad), 8000)
        }
    }

    func testValidPortsArePreserved() {
        for good in [1, 8000, 6548, 65535] {
            XCTAssertEqual(ServerManager.validatedPort(good), good)
        }
    }

    @MainActor
    func testInitSanitizesPersistedAddressAndPublishesItToMCPClient() {
        defaults.set("0", forKey: "symeraseme_host")
        defaults.set(70000, forKey: "symeraseme_port")
        MCPClient.configuredHost = "stale.invalid"
        MCPClient.configuredPort = 1
        let manager = makeManager()
        XCTAssertEqual(manager.host, "127.0.0.1")
        XCTAssertEqual(manager.port, 8000)
        XCTAssertEqual(MCPClient.configuredHost, "127.0.0.1")
        XCTAssertEqual(MCPClient.configuredPort, 8000)
    }

    @MainActor
    func testInitKeepsValidPersistedAddress() {
        defaults.set("localhost", forKey: "symeraseme_host")
        defaults.set(6548, forKey: "symeraseme_port")
        let manager = makeManager()
        XCTAssertEqual(manager.host, "localhost")
        XCTAssertEqual(manager.port, 6548)
        XCTAssertEqual(MCPClient.configuredHost, "localhost")
        XCTAssertEqual(MCPClient.configuredPort, 6548)
    }

    @MainActor
    func testEditsPersistToInjectedDefaultsOnly() {
        let manager = makeManager()
        manager.host = "::1"
        manager.port = 9001
        XCTAssertEqual(defaults.string(forKey: "symeraseme_host"), "::1")
        XCTAssertEqual(defaults.integer(forKey: "symeraseme_port"), 9001)
    }

    @MainActor
    func testStartSurfacesErrorForInvalidAddress() {
        let manager = makeManager()
        manager.host = "0"
        manager.start()
        XCTAssertNotNil(manager.lastError)
        XCTAssertTrue(manager.lastError?.contains("host") == true)
        XCTAssertFalse(manager.isRunning)

        manager.host = "127.0.0.1"
        manager.port = 0
        manager.start()
        XCTAssertTrue(manager.lastError?.contains("port") == true)
    }

    // MARK: - Auto-start

    @MainActor
    func testAutoStartRunsOnlyOnce() async {
        let manager = makeManager()
        MCPClient.configuredPort = 9  // keep the reachability probe off any live server
        manager.host = "0"  // start() will refuse, leaving an observable error
        await manager.autoStartIfNeeded()
        XCTAssertNotNil(manager.lastError)
        manager.lastError = nil
        await manager.autoStartIfNeeded()
        XCTAssertNil(manager.lastError, "second call must not start again")
    }

    // MARK: - Shutdown

    @MainActor
    func testQuittingTerminatesOnlyTheLaunchedChild() async throws {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("supervision-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let script = dir.appendingPathComponent("fake-server.sh")
        try "#!/bin/sh\nexec sleep 60\n".write(to: script, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)

        let manager = makeManager()
        manager.binaryPath = script.path
        manager.dataDir = dir.path
        manager.start()
        let deadline = Date().addingTimeInterval(5)
        while manager.pid == nil, Date() < deadline { try await Task.sleep(for: .milliseconds(25)) }
        let pid = try XCTUnwrap(manager.pid, "fake server did not start: \(manager.lastError ?? "")")
        XCTAssertEqual(kill(pid, 0), 0)

        NotificationCenter.default.post(name: NSApplication.willTerminateNotification, object: nil)

        XCTAssertEqual(kill(pid, 0), -1, "child must be gone after the app terminates")
        XCTAssertEqual(errno, ESRCH)
    }

    @MainActor
    func testStopForTerminationWithoutChildIsNoOp() {
        let manager = makeManager()
        manager.stopForTermination()
        XCTAssertNil(manager.pid)
    }

    // MARK: - Dashboard refresh

    @MainActor
    func testDashboardRefreshesWhenServerBecomesReachable() async {
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
        let vm = DashboardViewModel()
        await vm.reachabilityChanged(false)
        XCTAssertNil(vm.errorMessage)
        if case .idle = vm.state {} else { XCTFail("must not load when still unreachable") }

        await vm.reachabilityChanged(true)
        XCTAssertNotNil(vm.errorMessage, "reachable flip must trigger a refresh")
    }
}
