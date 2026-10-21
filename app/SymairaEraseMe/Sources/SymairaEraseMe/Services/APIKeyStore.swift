import Foundation
import Security

/// Persistent storage for the Anthropic API key.
///
/// The key must never be written to `UserDefaults`: preferences are stored
/// as a plain-text plist. The production store is the macOS Keychain.
protocol APIKeyStore: Sendable {
    /// The stored key, or `nil` when none is stored.
    func read() throws -> String?
    /// Stores `value` and verifies it can be read back.
    func save(_ value: String) throws
    /// Removes the stored key. Absence counts as success.
    func delete() throws
}

struct KeychainStoreError: LocalizedError, Equatable {
    let operation: String
    let status: OSStatus

    var errorDescription: String? {
        let detail = SecCopyErrorMessageString(status, nil) as String? ?? "unknown error"
        return "Keychain \(operation) failed (OSStatus \(status): \(detail))."
    }
}

/// Generic-password item in the user's login keychain.
///
/// ponytail: deliberately not `SymairaKeychain`, which always targets the
/// data-protection keychain; that needs a keychain-access-groups
/// entitlement this Developer ID app does not carry and fails with
/// errSecMissingEntitlement (-34018). Switch once the app ships with that
/// entitlement.
struct KeychainAPIKeyStore: APIKeyStore {
    static let defaultService = "dev.symaira.eraseme"
    static let account = "anthropic_api_key"
    let service: String

    init(service: String = KeychainAPIKeyStore.defaultService) {
        self.service = service
    }

    private var identity: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: Self.account,
        ]
    }

    func read() throws -> String? {
        var query = identity
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data else {
            throw KeychainStoreError(operation: "read", status: status)
        }
        return String(data: data, encoding: .utf8)
    }

    func save(_ value: String) throws {
        let data = Data(value.utf8)
        var status = SecItemUpdate(identity as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var add = identity
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlocked
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            throw KeychainStoreError(operation: "save", status: status)
        }
        // A successful write does not prove this binary can read it back.
        guard try read() == value else {
            throw KeychainStoreError(operation: "read-back", status: errSecDecode)
        }
    }

    func delete() throws {
        let status = SecItemDelete(identity as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainStoreError(operation: "delete", status: status)
        }
    }
}

/// Resolves the startup key and migrates the legacy plain-text preference.
enum APIKeyMigration {
    /// Former `UserDefaults` key that held the API key in plain text.
    static let legacyDefaultsKey = "symeraseme_anthropic_key"

    struct Outcome: Equatable {
        let key: String
        /// User-facing storage problem, never containing the key itself.
        let storageError: String?
    }

    /// Reads the key from `store`, migrating a legacy preference first.
    ///
    /// The legacy preference is removed only after the store verified the
    /// write. On any store failure the legacy value stays in place, so a
    /// failed migration never loses the user's key, and is retried on the
    /// next launch.
    static func load(store: APIKeyStore, defaults: UserDefaults) -> Outcome {
        let legacy = defaults.string(forKey: legacyDefaultsKey) ?? ""

        let stored: String?
        do {
            stored = try store.read()
        } catch {
            return Outcome(key: legacy, storageError: readFailureMessage(error))
        }

        if let stored, !stored.isEmpty {
            // Keychain already authoritative; drop any stale plain-text copy.
            if defaults.object(forKey: legacyDefaultsKey) != nil {
                defaults.removeObject(forKey: legacyDefaultsKey)
            }
            return Outcome(key: stored, storageError: nil)
        }

        guard !legacy.isEmpty else {
            defaults.removeObject(forKey: legacyDefaultsKey)
            return Outcome(key: "", storageError: nil)
        }

        do {
            try store.save(legacy)
        } catch {
            return Outcome(key: legacy, storageError: migrationFailureMessage(error))
        }
        defaults.removeObject(forKey: legacyDefaultsKey)
        return Outcome(key: legacy, storageError: nil)
    }

    static func readFailureMessage(_ error: Error) -> String {
        "Could not read the API key from the Keychain: \(error.localizedDescription)"
    }

    static func migrationFailureMessage(_ error: Error) -> String {
        "Could not move the API key into the Keychain: \(error.localizedDescription) "
            + "It stays in the previous settings storage and migration is retried on next launch."
    }

    static func saveFailureMessage(_ error: Error) -> String {
        "Could not save the API key to the Keychain: \(error.localizedDescription) "
            + "The key is used for this session only."
    }

    static func deleteFailureMessage(_ error: Error) -> String {
        "Could not remove the API key from the Keychain: \(error.localizedDescription)"
    }
}
