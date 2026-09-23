import XCTest
import Darwin
@testable import SymairaEraseMe

final class RustBackendIntegrationTests: XCTestCase {
    @MainActor
    func testExplicitRustBinaryLaunchAuthToolsAndShutdown() async throws {
        guard let binary = ProcessInfo.processInfo.environment["SYMERASEME_RUST_TEST_BINARY"],
              FileManager.default.isExecutableFile(atPath: binary) else {
            throw XCTSkip("Set SYMERASEME_RUST_TEST_BINARY to an executable Rust shadow binary")
        }
        XCTAssertEqual(URL(fileURLWithPath: binary).lastPathComponent, "symeraseme-rust")

        let defaults = UserDefaults.standard
        let keys = ["symeraseme_binary_path", "symeraseme_data_dir", "symeraseme_host", "symeraseme_port"]
        let previous = Dictionary(uniqueKeysWithValues: keys.compactMap { key in defaults.object(forKey: key).map { (key, $0) } })
        defer {
            for key in keys {
                if let value = previous[key] { defaults.set(value, forKey: key) }
                else { defaults.removeObject(forKey: key) }
            }
        }

        let dataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("symeraseme-rust-app-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dataDir) }

        let manager = ServerManager()
        manager.binaryPath = binary
        manager.dataDir = dataDir.path
        manager.host = "127.0.0.1"
        manager.port = try Self.freePort()
        manager.anthropicKey = ""
        manager.start()
        defer { manager.stop() }

        try await Self.waitUntil(timeout: 10) {
            manager.isRunning && FileManager.default.fileExists(atPath: dataDir.appendingPathComponent("mcp_token").path)
        }

        let token = try String(contentsOf: dataDir.appendingPathComponent("mcp_token"), encoding: .utf8)
            .trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertFalse(token.isEmpty, "The server must write its per-run bearer token")

        var unauthenticated = URLRequest(url: URL(string: "http://127.0.0.1:\(manager.port)/")!)
        unauthenticated.httpMethod = "POST"
        unauthenticated.setValue("application/json", forHTTPHeaderField: "Content-Type")
        unauthenticated.httpBody = try JSONSerialization.data(withJSONObject: [
            "jsonrpc": "2.0", "method": "tools/list", "params": [:], "id": 1
        ])
        let (_, unauthenticatedResponse) = try await URLSession.shared.data(for: unauthenticated)
        XCTAssertEqual((unauthenticatedResponse as? HTTPURLResponse)?.statusCode, 401)

        let tools = try await MCPClient.shared.listTools()
        XCTAssertTrue(tools.contains { $0["name"] as? String == "list_brokers" })
        let call = try await MCPClient.shared.callToolRaw("list_brokers")
        XCTAssertEqual(call["success"] as? Bool, true)

        guard let pid = manager.pid else {
            XCTFail("The app did not retain the launched backend PID")
            return
        }
        manager.stop()
        try await Self.waitUntil(timeout: 5) { !manager.isRunning && kill(pid, 0) == -1 && errno == ESRCH }
    }

    private static func waitUntil(
        timeout: TimeInterval,
        condition: @MainActor () -> Bool
    ) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await condition() { return }
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTFail("Condition was not met within \(timeout) seconds")
    }

    private static func freePort() throws -> Int {
        // ponytail: release the ephemeral listener before launch; retry if local port contention appears.
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        defer { close(descriptor) }

        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = 0
        address.sin_addr = in_addr(s_addr: inet_addr("127.0.0.1"))
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard bound == 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }

        var assigned = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let named = withUnsafeMutablePointer(to: &assigned) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                getsockname(descriptor, $0, &length)
            }
        }
        guard named == 0 else { throw POSIXError(.init(rawValue: errno) ?? .EIO) }
        return Int(UInt16(bigEndian: assigned.sin_port))
    }
}
