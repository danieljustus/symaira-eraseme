import XCTest
@testable import SymairaEraseMe

/// Regression tests for #1254: the Anthropic API key moves from the
/// plain-text `UserDefaults` plist into the Keychain without being lost.
final class APIKeyStorageTests: XCTestCase {
    private struct StoreFailure: LocalizedError {
        var errorDescription: String? { "simulated keychain denial" }
    }

    /// In-memory stand-in for the Keychain with injectable failures.
    private final class FakeStore: APIKeyStore, @unchecked Sendable {
        var value: String?
        var failRead = false
        var failSave = false
        var failDelete = false
        private(set) var saveCount = 0

        init(value: String? = nil) { self.value = value }

        func read() throws -> String? {
            if failRead { throw StoreFailure() }
            return value
        }

        func save(_ value: String) throws {
            saveCount += 1
            if failSave { throw StoreFailure() }
            self.value = value
        }

        func delete() throws {
            if failDelete { throw StoreFailure() }
            value = nil
        }
    }

    // Built at runtime so no provider-format literal appears in source.
    private let secret = "sk-" + "ant-test-" + String(repeating: "x", count: 12)
    private let legacyKey = APIKeyMigration.legacyDefaultsKey
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() {
        super.setUp()
        suiteName = "APIKeyStorageTests.\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
        defaults = nil
        super.tearDown()
    }

    func testLegacyKeyMigratesAndPlainTextCopyIsRemoved() {
        defaults.set(secret, forKey: legacyKey)
        defaults.set("/custom/data", forKey: "symeraseme_data_dir")
        let store = FakeStore()

        let outcome = APIKeyMigration.load(store: store, defaults: defaults)

        XCTAssertEqual(outcome, .init(key: secret, storageError: nil))
        XCTAssertEqual(store.value, secret)
        XCTAssertNil(defaults.object(forKey: legacyKey))
        XCTAssertEqual(defaults.string(forKey: "symeraseme_data_dir"), "/custom/data",
                       "unrelated settings must be preserved")
    }

    func testFailedMigrationKeepsLegacyValueAndReportsWithoutLeakingKey() {
        defaults.set(secret, forKey: legacyKey)
        let store = FakeStore()
        store.failSave = true

        let outcome = APIKeyMigration.load(store: store, defaults: defaults)

        XCTAssertEqual(outcome.key, secret, "the key must stay usable for this session")
        XCTAssertEqual(defaults.string(forKey: legacyKey), secret, "a failed migration must not lose the key")
        let message = try? XCTUnwrap(outcome.storageError)
        XCTAssertNotNil(message)
        XCTAssertFalse(message?.contains(secret) ?? true)
    }

    func testKeychainReadFailureFallsBackToLegacyWithoutDeletingIt() {
        defaults.set(secret, forKey: legacyKey)
        let store = FakeStore()
        store.failRead = true

        let outcome = APIKeyMigration.load(store: store, defaults: defaults)

        XCTAssertEqual(outcome.key, secret)
        XCTAssertNotNil(outcome.storageError)
        XCTAssertEqual(store.saveCount, 0)
        XCTAssertEqual(defaults.string(forKey: legacyKey), secret)
    }

    func testRelaunchReadsKeyFromKeychainAndDropsStaleLegacyCopy() {
        defaults.set("stale", forKey: legacyKey)
        let store = FakeStore(value: secret)

        let outcome = APIKeyMigration.load(store: store, defaults: defaults)

        XCTAssertEqual(outcome, .init(key: secret, storageError: nil))
        XCTAssertEqual(store.saveCount, 0)
        XCTAssertNil(defaults.object(forKey: legacyKey))
    }

    @MainActor
    func testEditingKeyWritesKeychainAndNeverUserDefaults() {
        let store = FakeStore()
        let manager = ServerManager(resourceURL: nil, executableURL: nil, apiKeyStore: store, defaults: defaults)

        manager.anthropicKey = secret

        XCTAssertEqual(store.value, secret)
        XCTAssertNil(manager.apiKeyStorageError)
        XCTAssertNil(defaults.object(forKey: legacyKey))
        let plist = defaults.persistentDomain(forName: suiteName) ?? [:]
        XCTAssertFalse(plist.values.contains { ($0 as? String) == secret },
                       "the key must not appear anywhere in the preferences plist")

        // Relaunch: a fresh manager reads the key back from the store.
        let relaunched = ServerManager(resourceURL: nil, executableURL: nil, apiKeyStore: store, defaults: defaults)
        XCTAssertEqual(relaunched.anthropicKey, secret)
    }

    @MainActor
    func testFailedSaveSurfacesErrorAndDoesNotFallBackToUserDefaults() {
        let store = FakeStore()
        store.failSave = true
        let manager = ServerManager(resourceURL: nil, executableURL: nil, apiKeyStore: store, defaults: defaults)

        manager.anthropicKey = secret

        XCTAssertEqual(manager.anthropicKey, secret)
        XCTAssertNotNil(manager.apiKeyStorageError)
        XCTAssertFalse(manager.apiKeyStorageError?.contains(secret) ?? true)
        XCTAssertNil(defaults.object(forKey: legacyKey))
    }

    @MainActor
    func testClearingKeyDeletesKeychainItemAndLegacyCopy() {
        defaults.set(secret, forKey: legacyKey)
        let store = FakeStore()
        store.failSave = true // migration fails, legacy stays
        let manager = ServerManager(resourceURL: nil, executableURL: nil, apiKeyStore: store, defaults: defaults)
        XCTAssertEqual(defaults.string(forKey: legacyKey), secret)

        store.failSave = false
        manager.anthropicKey = ""

        XCTAssertNil(store.value)
        XCTAssertNil(defaults.object(forKey: legacyKey))
        XCTAssertNil(manager.apiKeyStorageError)
    }
}
