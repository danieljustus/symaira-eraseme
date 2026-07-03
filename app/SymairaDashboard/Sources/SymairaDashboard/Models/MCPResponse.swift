import Foundation

// MARK: - JSON-RPC 2.0 Envelope

/// Top-level JSON-RPC 2.0 response from the MCP server.
struct JSONRPCResponse: Codable {
    let jsonrpc: String
    let result: JSONRPCResult?
    let error: JSONRPCError?
    let id: Int
}

struct JSONRPCResult: Codable {
    let content: [MCPContent]
}

struct MCPContent: Codable {
    let type: String
    let text: String
}

struct JSONRPCError: Codable, LocalizedError {
    let code: Int
    let message: String

    var errorDescription: String? { message }
}

/// JSON-RPC 2.0 request body.
struct JSONRPCRequest: Codable {
    let jsonrpc: String
    let method: String
    let params: [String: AnyCodable]
    let id: Int
}

// MARK: - MCP Tool Call Envelope

/// The inner payload spread across CliResult.to_json().
/// Keys are spread at top level — we capture them dynamically.
struct MCPCallResult: Codable {
    let success: Bool
    let message: String?
    let error: String?

    // Dynamic data keys are decoded via a dictionary wrapper
    let raw: [String: AnyCodable]

    enum CodingKeys: String, CodingKey {
        case success, message, error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: DynamicKey.self)
        // Decode known fields
        success = try container.decodeIfPresent(Bool.self, forKey: DynamicKey(stringValue: "success")) ?? false
        message = try container.decodeIfPresent(String.self, forKey: DynamicKey(stringValue: "message"))
        error = try container.decodeIfPresent(String.self, forKey: DynamicKey(stringValue: "error"))
        // Capture everything as raw
        var dict: [String: AnyCodable] = [:]
        for key in container.allKeys {
            dict[key.stringValue] = try container.decode(AnyCodable.self, forKey: key)
        }
        raw = dict
    }
}

// MARK: - Dynamic Coding Support

/// A CodingKey that accepts any string.
struct DynamicKey: CodingKey {
    var stringValue: String
    var intValue: Int?

    init(stringValue: String) {
        self.stringValue = stringValue
    }

    init(intValue: Int) {
        self.stringValue = "\(intValue)"
        self.intValue = intValue
    }
}

/// Type-erased Codable wrapper for JSON values.
struct AnyCodable: Codable {
    let value: Any

    init(_ value: Any) {
        self.value = value
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            value = NSNull()
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if let int = try? container.decode(Int.self) {
            value = int
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let string = try? container.decode(String.self) {
            value = string
        } else if let array = try? container.decode([AnyCodable].self) {
            value = array.map(\.value)
        } else if let dict = try? container.decode([String: AnyCodable].self) {
            value = dict.mapValues(\.value)
        } else {
            throw DecodingError.dataCorruptedError(
                in: container, debugDescription: "Unsupported AnyCodable type")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch value {
        case is NSNull:
            try container.encodeNil()
        case let bool as Bool:
            try container.encode(bool)
        case let int as Int:
            try container.encode(int)
        case let double as Double:
            try container.encode(double)
        case let string as String:
            try container.encode(string)
        case let array as [Any]:
            try container.encode(array.map { AnyCodable($0) })
        case let dict as [String: Any]:
            try container.encode(dict.mapValues { AnyCodable($0) })
        default:
            try container.encodeNil()
        }
    }

    /// Convenience accessors
    var asInt: Int? { value as? Int }
    var asDouble: Double? { value as? Double }
    var asString: String? { value as? String }
    var asBool: Bool? { value as? Bool }
    var asArray: [Any]? { value as? [Any] }
    var asDict: [String: Any]? { value as? [String: Any] }
}

// MARK: - MCPClient Errors

enum MCPClientError: LocalizedError {
    case serverUnreachable(String)
    case jsonRPCError(JSONRPCError)
    case toolCallFailed(String)
    case decodingError(String)
    case invalidResponse(String)

    var errorDescription: String? {
        switch self {
        case .serverUnreachable(let msg): return "Server unreachable: \(msg)"
        case .jsonRPCError(let err): return "JSON-RPC error \(err.code): \(err.message)"
        case .toolCallFailed(let msg): return "Tool call failed: \(msg)"
        case .decodingError(let msg): return "Decoding error: \(msg)"
        case .invalidResponse(let msg): return "Invalid response: \(msg)"
        }
    }
}
