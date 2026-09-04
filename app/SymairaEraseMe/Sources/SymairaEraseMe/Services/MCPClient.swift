import Foundation

/// Result of a reachability probe against the MCP server.
enum ConnectionStatus: Equatable, Sendable {
    /// The server answered and accepted the request.
    case connected
    /// The server is up but rejected the request as unauthenticated (401).
    case unauthorized
    /// The server could not be reached (connection refused, timeout, ...).
    case unreachable
}

/// JSON-RPC 2.0 HTTP client for the Symaira MCP server.
/// Posts to `http://127.0.0.1:8000` and parses the MCP content envelope.
actor MCPClient {
    static let shared = MCPClient()

    private let session: URLSession
    private let decoder = JSONDecoder()
    private var requestId: Int = 0

    /// Configurable host (default 127.0.0.1).
    nonisolated(unsafe) static var configuredHost: String = "127.0.0.1"
    /// Configurable port (default 8000).
    nonisolated(unsafe) static var configuredPort: Int = 8000
    /// Configurable data directory (default ~/.local/share/symeraseme).
    /// Respects SYMERASEME_DATA_DIR environment variable.
    nonisolated(unsafe) static var configuredDataDir: String = {
        if let env = ProcessInfo.processInfo.environment["SYMERASEME_DATA_DIR"] {
            return env
        }
        return NSString(string: "~/.local/share/symeraseme").expandingTildeInPath
    }()

    init(session: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 30
        config.timeoutIntervalForResource = 60
        return URLSession(configuration: config)
    }()) {
        self.session = session
    }

    // MARK: - Public API

    /// List available tools from the MCP server.
    /// Parses the raw JSON-RPC 2.0 `{"result": {"tools": [...]}}` response
    /// without routing through the tools/call content-envelope decoder.
    func listTools() async throws -> [[String: Any]] {
        let data = try await postJSONRPC(method: "tools/list", params: [:])
        return try Self.decodeToolsListResponse(data)
    }

    /// Decodes a tools/list JSON-RPC 2.0 response into an array of tool definition dictionaries.
    /// Does not route through the tools/call content-envelope decoder.
    static func decodeToolsListResponse(_ data: Data) throws -> [[String: Any]] {
        let jsonObject: Any
        do {
            jsonObject = try JSONSerialization.jsonObject(with: data)
        } catch {
            throw MCPClientError.invalidResponse("Could not decode JSON: \(error.localizedDescription)")
        }

        guard let dict = jsonObject as? [String: Any] else {
            throw MCPClientError.invalidResponse("Expected JSON object response")
        }

        guard let version = dict["jsonrpc"] as? String, version == "2.0" else {
            throw MCPClientError.invalidResponse("Invalid or missing JSON-RPC version: expected 2.0")
        }

        if let errorObj = dict["error"] {
            if let errorDict = errorObj as? [String: Any],
               let code = errorDict["code"] as? Int,
               let message = errorDict["message"] as? String {
                throw MCPClientError.jsonRPCError(JSONRPCError(code: code, message: message))
            }
            throw MCPClientError.invalidResponse("Invalid JSON-RPC error object in response")
        }

        guard let result = dict["result"] as? [String: Any] else {
            throw MCPClientError.invalidResponse("Missing or invalid 'result' object in response")
        }

        guard let rawTools = result["tools"] as? [Any] else {
            throw MCPClientError.invalidResponse("Missing or invalid 'tools' array in result")
        }

        var tools = [[String: Any]]()
        tools.reserveCapacity(rawTools.count)
        for item in rawTools {
            guard let toolDict = item as? [String: Any],
                  let name = toolDict["name"] as? String,
                  !name.isEmpty else {
                throw MCPClientError.invalidResponse("Invalid tool definition in tools array")
            }
            tools.append(toolDict)
        }

        return tools
    }

    /// Call an MCP tool by name with arguments. Returns the decoded data.
    func callTool<T: Decodable>(_ name: String, arguments: [String: Any] = [:]) async throws -> T {
        let params: [String: Any] = ["name": name, "arguments": arguments]
        let result = try await call(method: "tools/call", params: params)

        // Use JSONEncoder to serialize AnyCodable safely —
        // JSONSerialization.data(withJSONObject:) would raise
        // an uncatchable ObjC exception on AnyCodable values.
        let data = try JSONEncoder().encode(result.raw)
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw MCPClientError.decodingError("\(T.self): \(error.localizedDescription)")
        }
    }

    /// Call an MCP tool and return the raw dictionary (for dynamic shapes).
    func callToolRaw(_ name: String, arguments: [String: Any] = [:]) async throws -> [String: Any] {
        let params: [String: Any] = ["name": name, "arguments": arguments]
        let result = try await call(method: "tools/call", params: params)

        // Round-trip through JSONEncoder → JSONSerialization
        // so AnyCodable values become JSON-legal Foundation types.
        let data = try JSONEncoder().encode(result.raw)
        let obj = try JSONSerialization.jsonObject(with: data)
        guard let dict = obj as? [String: Any] else {
            throw MCPClientError.invalidResponse("Expected dictionary payload")
        }
        return dict
    }

    /// Check if the MCP server is reachable.
    /// Uses its own lightweight JSON-RPC parser for the tools/list response shape.
    /// Distinguishes an auth rejection (401) from a transport failure so the
    /// UI can point at the real cause instead of claiming unreachability.
    func ping() async -> ConnectionStatus {
        do {
            let host = MCPClient.configuredHost
            let port = MCPClient.configuredPort
            guard let url = URL(string: "http://\(host):\(port)/") else {
                return .unreachable
            }

            var request = URLRequest(url: url)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")

            if let token = Self.readAuthToken(), !token.isEmpty {
                request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            }

            let body = try JSONSerialization.data(withJSONObject: [
                "jsonrpc": "2.0",
                "method": "tools/list",
                "params": [:],
                "id": 1
            ])
            request.httpBody = body

            let (data, response) = try await session.data(for: request)

            guard let httpResponse = response as? HTTPURLResponse else {
                return .unreachable
            }

            // 401 means the server is up and listening but rejected our
            // token — an auth problem, not a reachability problem.
            if httpResponse.statusCode == 401 {
                return .unauthorized
            }

            guard (200...299).contains(httpResponse.statusCode) else {
                return .unreachable
            }

            // Parse as a generic JSON-RPC 2.0 response — tools/list returns
            // {"result": {"tools": [...]}} not the tool-call envelope shape.
            guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  json["result"] != nil,
                  json["error"] == nil else {
                return .unreachable
            }

            return .connected
        } catch {
            return .unreachable
        }
    }

    // MARK: - Internal

    private func call(method: String, params: [String: Any]) async throws -> MCPCallResult {
        let data = try await postJSONRPC(method: method, params: params)
        return try decodeCallResponse(data)
    }

    private func decodeCallResponse(_ data: Data) throws -> MCPCallResult {
        guard let rpcResponse = try? decoder.decode(JSONRPCResponse.self, from: data) else {
            throw MCPClientError.invalidResponse("Could not decode JSON-RPC response")
        }

        guard rpcResponse.jsonrpc == "2.0" else {
            throw MCPClientError.invalidResponse("Invalid or missing JSON-RPC version: expected 2.0")
        }

        if let error = rpcResponse.error {
            throw MCPClientError.jsonRPCError(error)
        }

        guard let result = rpcResponse.result,
              let content = result.content.first,
              content.type == "text" else {
            throw MCPClientError.invalidResponse("Missing or non-text content in result")
        }

        // Parse the inner JSON string (CliResult.to_json())
        guard let textData = content.text.data(using: .utf8) else {
            throw MCPClientError.invalidResponse("Could not encode text as UTF-8")
        }

        let callResult: MCPCallResult
        do {
            callResult = try decoder.decode(MCPCallResult.self, from: textData)
        } catch {
            throw MCPClientError.decodingError("Inner payload: \(error.localizedDescription)")
        }

        // Check success field
        if !callResult.success {
            let errMsg = callResult.error ?? callResult.message ?? "Unknown error"
            throw MCPClientError.toolCallFailed(errMsg)
        }

        return callResult
    }

    // MARK: - Transport

    /// Shared private HTTP JSON-RPC transport helper used by both tools/list and tools/call.
    /// Manages request IDs, JSON-RPC envelope formation, bearer authentication,
    /// network dispatch, and HTTP status code validation (200...299).
    private func postJSONRPC(method: String, params: [String: Any]) async throws -> Data {
        requestId += 1
        let currentId = requestId

        let body = try JSONSerialization.data(withJSONObject: [
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": currentId
        ])

        let host = MCPClient.configuredHost
        let port = MCPClient.configuredPort
        guard let url = URL(string: "http://\(host):\(port)/") else {
            throw MCPClientError.invalidResponse("Invalid URL")
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.httpBody = body
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        // Attach per-run Bearer token for MCP server authentication.
        if let token = Self.readAuthToken(), !token.isEmpty {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw MCPClientError.serverUnreachable(error.localizedDescription)
        }

        guard let httpResponse = response as? HTTPURLResponse else {
            throw MCPClientError.invalidResponse("Not an HTTP response")
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            let bodyStr = String(data: data, encoding: .utf8) ?? "no body"
            throw MCPClientError.serverUnreachable("HTTP \(httpResponse.statusCode): \(bodyStr)")
        }

        return data
    }

    // MARK: - Auth

    /// Reads the MCP bearer token from the configured data directory.
    /// The token is written by the MCP server on startup and must be
    /// included as an Authorization header on every request.
    private static func readAuthToken() -> String? {
        let tokenPath = (configuredDataDir as NSString).appendingPathComponent("mcp_token")
        guard let token = try? String(contentsOfFile: tokenPath, encoding: .utf8) else {
            return nil
        }
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
