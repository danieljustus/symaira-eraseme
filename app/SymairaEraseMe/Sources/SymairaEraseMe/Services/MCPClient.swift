import Foundation

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

    private init() {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 30
        config.timeoutIntervalForResource = 60
        self.session = URLSession(configuration: config)
    }

    // MARK: - Public API

    /// List available tools from the MCP server.
    func listTools() async throws -> [[String: Any]] {
        let params: [String: Any] = [:]
        let result = try await call(method: "tools/list", params: params)
        // The tools list is inside the raw dictionary under a tools key,
        // or the entire raw value might be the array if the server returns it that way.
        // Try to extract from raw
        if let toolsAny = result.raw["tools"]?.value as? [Any] {
            return toolsAny.compactMap { $0 as? [String: Any] }
        }
        // Fallback: if the raw value itself is a dictionary, the tools list may not be present
        // In that case, return an empty array (tools/list returns the list differently)
        return []
    }

    /// Call an MCP tool by name with arguments. Returns the decoded data.
    func callTool<T: Decodable>(_ name: String, arguments: [String: Any] = [:]) async throws -> T {
        let params: [String: Any] = ["name": name, "arguments": arguments]
        let result = try await call(method: "tools/call", params: params)

        // Try to decode the raw value directly
        let data = try JSONSerialization.data(withJSONObject: result.raw)
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
        // Convert AnyCodable values back to Any
        var output: [String: Any] = [:]
        for (key, val) in result.raw {
            output[key] = val.value
        }
        return output
    }

    /// Check if the MCP server is reachable.
    func ping() async -> Bool {
        do {
            let _: MCPCallResult = try await call(method: "tools/list", params: [:])
            return true
        } catch {
            return false
        }
    }

    // MARK: - Internal

    private func call(method: String, params: [String: Any]) async throws -> MCPCallResult {
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

        guard let rpcResponse = try? decoder.decode(JSONRPCResponse.self, from: data) else {
            throw MCPClientError.invalidResponse("Could not decode JSON-RPC response")
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
}
