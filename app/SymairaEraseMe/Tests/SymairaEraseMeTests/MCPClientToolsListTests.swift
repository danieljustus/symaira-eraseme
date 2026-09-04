import XCTest
@testable import SymairaEraseMe

final class MCPClientToolsListTests: XCTestCase {

    // MARK: - Synchronized Mock Transport

    private final class MockURLProtocol: URLProtocol, @unchecked Sendable {
        private static let lock = NSLock()
        private static var _handlers: [String: @Sendable (URLRequest) throws -> (HTTPURLResponse, Data)] = [:]
        private static var _defaultHandler: (@Sendable (URLRequest) throws -> (HTTPURLResponse, Data))?

        static func registerHandler(
            id: String,
            handler: @escaping @Sendable (URLRequest) throws -> (HTTPURLResponse, Data)
        ) {
            lock.lock()
            defer { lock.unlock() }
            _handlers[id] = handler
            _defaultHandler = handler
        }

        static func unregisterHandler(id: String) {
            lock.lock()
            defer { lock.unlock() }
            _handlers.removeValue(forKey: id)
            if _handlers.isEmpty {
                _defaultHandler = nil
            }
        }

        static func clearAll() {
            lock.lock()
            defer { lock.unlock() }
            _handlers.removeAll()
            _defaultHandler = nil
        }

        private static func handler(for request: URLRequest) -> (@Sendable (URLRequest) throws -> (HTTPURLResponse, Data))? {
            lock.lock()
            defer { lock.unlock() }
            if let id = request.value(forHTTPHeaderField: "X-Mock-Session-ID"), let h = _handlers[id] {
                return h
            }
            return _defaultHandler
        }

        override class func canInit(with request: URLRequest) -> Bool {
            return true
        }

        override class func canonicalRequest(for request: URLRequest) -> URLRequest {
            return request
        }

        override func startLoading() {
            guard let handler = Self.handler(for: request) else {
                client?.urlProtocol(self, didFailWithError: URLError(.badURL))
                return
            }
            do {
                let (response, data) = try handler(request)
                client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
                client?.urlProtocol(self, didLoad: data)
                client?.urlProtocolDidFinishLoading(self)
            } catch {
                client?.urlProtocol(self, didFailWithError: error)
            }
        }

        override func stopLoading() {}
    }

    private final class CapturedState: @unchecked Sendable {
        private let lock = NSLock()
        private var _request: URLRequest?
        private var _body: Data?

        func record(request: URLRequest, body: Data?) {
            lock.lock()
            defer { lock.unlock() }
            self._request = request
            self._body = body
        }

        var request: URLRequest? {
            lock.lock()
            defer { lock.unlock() }
            return _request
        }

        var body: Data? {
            lock.lock()
            defer { lock.unlock() }
            return _body
        }
    }

    private func makeMockSession(
        handler: @escaping @Sendable (URLRequest) throws -> (HTTPURLResponse, Data)
    ) -> (URLSession, () -> Void) {
        let sessionId = UUID().uuidString
        MockURLProtocol.registerHandler(id: sessionId, handler: handler)

        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [MockURLProtocol.self]
        config.httpAdditionalHeaders = ["X-Mock-Session-ID": sessionId]
        let session = URLSession(configuration: config)

        return (session, {
            MockURLProtocol.unregisterHandler(id: sessionId)
        })
    }

    override func tearDown() {
        MockURLProtocol.clearAll()
        super.tearDown()
    }

    // MARK: - Helpers

    /// Resolves fixture path robustly by walking up parent directories from #filePath.
    /// Does not rely on current working directory (CWD).
    private static func loadCommittedToolsListFixture() -> Data {
        var current = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        while current.pathComponents.count > 1 {
            let candidate = current.appendingPathComponent("tests/fixtures/mcp-contract/tools.list.json")
            if FileManager.default.fileExists(atPath: candidate.path),
               let data = try? Data(contentsOf: candidate) {
                return data
            }
            let parent = current.deletingLastPathComponent()
            if parent == current { break }
            current = parent
        }
        fatalError("Could not locate tests/fixtures/mcp-contract/tools.list.json starting from \(#filePath)")
    }

    private static func makeExactToolsListJSONRPCResponseData() -> Data {
        let rawFixture = loadCommittedToolsListFixture()
        guard let fixtureObj = try? JSONSerialization.jsonObject(with: rawFixture) as? [String: Any] else {
            fatalError("Could not decode tools.list.json fixture")
        }
        let jsonRPC: [String: Any] = [
            "jsonrpc": "2.0",
            "result": fixtureObj,
            "id": 1
        ]
        return try! JSONSerialization.data(withJSONObject: jsonRPC)
    }

    private static func extractBody(from request: URLRequest) -> Data? {
        if let body = request.httpBody {
            return body
        }
        guard let stream = request.httpBodyStream else {
            return nil
        }
        stream.open()
        defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 1024)
        while stream.hasBytesAvailable {
            let bytesRead = stream.read(&buffer, maxLength: buffer.count)
            if bytesRead > 0 {
                data.append(buffer, count: bytesRead)
            } else {
                break
            }
        }
        return data.isEmpty ? nil : data
    }

    // MARK: - 1. Exact Committed 26-Tool Fixture Transport Test

    /// Verifies that listTools parses the exact committed 26-tool tools/list response over the transport.
    func testListToolsParsesExactCommitted26ToolResponse() async throws {
        let responseData = Self.makeExactToolsListJSONRPCResponseData()
        let captured = CapturedState()

        let (session, cleanup) = makeMockSession { request in
            captured.record(request: request, body: Self.extractBody(from: request))
            guard let url = request.url else { throw URLError(.badURL) }
            let response = HTTPURLResponse(
                url: url,
                statusCode: 200,
                httpVersion: "HTTP/1.1",
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, responseData)
        }
        defer { cleanup() }

        let client = MCPClient(session: session)
        let tools = try await client.listTools()

        // Verify request construction
        guard let request = captured.request else {
            XCTFail("Request was not captured")
            return
        }
        XCTAssertEqual(request.httpMethod, "POST")
        XCTAssertEqual(request.value(forHTTPHeaderField: "Content-Type"), "application/json")

        guard let body = captured.body,
              let bodyJSON = try? JSONSerialization.jsonObject(with: body) as? [String: Any] else {
            XCTFail("tools/list request body could not be decoded as JSON")
            return
        }
        XCTAssertEqual(bodyJSON["jsonrpc"] as? String, "2.0")
        XCTAssertEqual(bodyJSON["method"] as? String, "tools/list")
        XCTAssertNotNil(bodyJSON["id"])
        XCTAssertNotNil(bodyJSON["params"])

        // Verify tool count and names against committed catalogue
        XCTAssertEqual(tools.count, 26, "Expected exactly 26 tools from the committed catalogue")
        let toolNames = tools.compactMap { $0["name"] as? String }
        XCTAssertEqual(toolNames.count, 26, "All 26 tools must have string names")

        let expectedNames = [
            "redact_file", "plan_create", "plan_show", "execute",
            "poll_inbox", "classify_reply", "generate_rebuttal",
            "generate_dashboard", "generate_report", "manual_tasks_list",
            "manual_tasks_show", "manual_tasks_complete", "manual_tasks_cleanup",
            "generate_scheduler", "schedule_install", "schedule_uninstall",
            "schedule_status", "validate", "run_web_form", "auto_confirm",
            "get_dashboard_data", "list_requests", "get_events",
            "list_brokers", "get_calendar", "grant"
        ]
        XCTAssertEqual(toolNames, expectedNames, "Tools order and names must match the exact committed Go catalogue")

        for tool in tools {
            XCTAssertNotNil(tool["name"], "Tool must have a name")
            XCTAssertNotNil(tool["description"], "Tool must have a description")
            XCTAssertNotNil(tool["inputSchema"] as? [String: Any], "Tool must have an inputSchema dictionary")
        }
    }

    // MARK: - 2. Table-Driven Malformed-Result Cases

    /// Table-driven verification of malformed or non-compliant tools/list response payloads.
    func testDecodeToolsListResponseRejectsMalformedPayloads() {
        struct TestCase {
            let name: String
            let payload: String
        }

        let cases: [TestCase] = [
            TestCase(name: "invalid json", payload: "{\"jsonrpc\": \"2.0\", broken"),
            TestCase(name: "top-level array", payload: "[1, 2, 3]"),
            TestCase(name: "missing jsonrpc version", payload: "{\"result\": {\"tools\": []}, \"id\": 1}"),
            TestCase(name: "invalid jsonrpc version", payload: "{\"jsonrpc\": \"1.0\", \"result\": {\"tools\": []}, \"id\": 1}"),
            TestCase(name: "missing result object", payload: "{\"jsonrpc\": \"2.0\", \"id\": 1}"),
            TestCase(name: "primitive result", payload: "{\"jsonrpc\": \"2.0\", \"result\": \"unexpected string\", \"id\": 1}"),
            TestCase(name: "missing tools key", payload: "{\"jsonrpc\": \"2.0\", \"result\": {\"methods\": []}, \"id\": 1}"),
            TestCase(name: "non-array tools", payload: "{\"jsonrpc\": \"2.0\", \"result\": {\"tools\": \"not an array\"}, \"id\": 1}"),
            TestCase(name: "non-dictionary tool item", payload: "{\"jsonrpc\": \"2.0\", \"result\": {\"tools\": [\"redact_file\"]}, \"id\": 1}"),
            TestCase(name: "tool missing name", payload: "{\"jsonrpc\": \"2.0\", \"result\": {\"tools\": [{\"description\": \"missing\"}]}, \"id\": 1}"),
            TestCase(name: "tool with empty name", payload: "{\"jsonrpc\": \"2.0\", \"result\": {\"tools\": [{\"name\": \"\"}]}, \"id\": 1}"),
            TestCase(name: "malformed error object", payload: "{\"jsonrpc\": \"2.0\", \"error\": \"server exploded\", \"id\": 1}")
        ]

        for testCase in cases {
            let data = Data(testCase.payload.utf8)
            XCTAssertThrowsError(
                try MCPClient.decodeToolsListResponse(data),
                "Expected invalidResponse failure for case '\(testCase.name)'"
            ) { error in
                guard case MCPClientError.invalidResponse = error else {
                    XCTFail("Expected invalidResponse for case '\(testCase.name)', got \(error)")
                    return
                }
            }
        }
    }

    // MARK: - 3. JSON-RPC Error

    /// Verifies that a JSON-RPC error response throws MCPClientError.jsonRPCError over decoder and transport.
    func testToolsListThrowsOnJSONRPCError() async {
        let errorJSON = """
        {
            "jsonrpc": "2.0",
            "error": {
                "code": -32601,
                "message": "method not found"
            },
            "id": 1
        }
        """
        let errorData = Data(errorJSON.utf8)

        // Direct decoder verification
        XCTAssertThrowsError(try MCPClient.decodeToolsListResponse(errorData)) { error in
            guard case MCPClientError.jsonRPCError(let rpcError) = error else {
                XCTFail("Expected jsonRPCError from decoder, got \(error)")
                return
            }
            XCTAssertEqual(rpcError.code, -32601)
            XCTAssertEqual(rpcError.message, "method not found")
        }

        // Transport path verification
        let (session, cleanup) = makeMockSession { request in
            guard let url = request.url else { throw URLError(.badURL) }
            let response = HTTPURLResponse(
                url: url,
                statusCode: 200,
                httpVersion: "HTTP/1.1",
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, errorData)
        }
        defer { cleanup() }

        let client = MCPClient(session: session)
        do {
            _ = try await client.listTools()
            XCTFail("Expected listTools to throw jsonRPCError")
        } catch MCPClientError.jsonRPCError(let rpcError) {
            XCTAssertEqual(rpcError.code, -32601)
            XCTAssertEqual(rpcError.message, "method not found")
        } catch {
            XCTFail("Unexpected error thrown: \(error)")
        }
    }

    // MARK: - 4. HTTP Error

    /// Verifies that an HTTP error status (500) over the transport throws serverUnreachable.
    func testListToolsThrowsOnHTTPErrorStatus() async {
        let (session, cleanup) = makeMockSession { request in
            guard let url = request.url else { throw URLError(.badURL) }
            let response = HTTPURLResponse(
                url: url,
                statusCode: 500,
                httpVersion: "HTTP/1.1",
                headerFields: [:]
            )!
            return (response, Data("Internal Server Error".utf8))
        }
        defer { cleanup() }

        let client = MCPClient(session: session)
        do {
            _ = try await client.listTools()
            XCTFail("Expected listTools to throw on HTTP 500")
        } catch MCPClientError.serverUnreachable(let msg) {
            XCTAssertTrue(msg.contains("HTTP 500"))
        } catch {
            XCTFail("Unexpected error: \(error)")
        }
    }

    // MARK: - 5. tools/call Regression

    /// Verifies that tools/call content-envelope decoding and ping remain intact and regression-free.
    func testToolsCallContentEnvelopePathRemainsUnchanged() async throws {
        let callResponseJSON = """
        {
            "jsonrpc": "2.0",
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": "{\\"success\\":true,\\"message\\":\\"ok\\",\\"total_count\\":42}"
                    }
                ]
            },
            "id": 1
        }
        """
        let (callSession, callCleanup) = makeMockSession { request in
            guard let url = request.url else { throw URLError(.badURL) }
            let response = HTTPURLResponse(
                url: url,
                statusCode: 200,
                httpVersion: "HTTP/1.1",
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(callResponseJSON.utf8))
        }
        defer { callCleanup() }

        let callClient = MCPClient(session: callSession)
        let rawDict = try await callClient.callToolRaw("get_dashboard_data")
        XCTAssertEqual(rawDict["success"] as? Bool, true)
        XCTAssertEqual(rawDict["message"] as? String, "ok")
        XCTAssertEqual(rawDict["total_count"] as? Int, 42)

        // Ping reachability verification with tools/list response shape
        let pingResponseJSON = """
        {
            "jsonrpc": "2.0",
            "result": {
                "tools": []
            },
            "id": 1
        }
        """
        let (pingSession, pingCleanup) = makeMockSession { request in
            guard let url = request.url else { throw URLError(.badURL) }
            let response = HTTPURLResponse(
                url: url,
                statusCode: 200,
                httpVersion: "HTTP/1.1",
                headerFields: ["Content-Type": "application/json"]
            )!
            return (response, Data(pingResponseJSON.utf8))
        }
        defer { pingCleanup() }

        let pingClient = MCPClient(session: pingSession)
        let status = await pingClient.ping()
        XCTAssertEqual(status, .connected)
    }
}
