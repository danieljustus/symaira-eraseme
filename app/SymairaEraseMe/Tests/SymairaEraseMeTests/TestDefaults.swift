import Foundation
@testable import SymairaEraseMe

/// A throwaway preferences suite for one test.
///
/// A random suite name used to leave one empty plist per test in
/// `~/Library/Preferences`: `removePersistentDomain(forName:)` only empties the
/// domain, and cfprefsd writes the empty file asynchronously, so deleting it
/// right away does not stick. An absolute path as the suite name makes
/// CFPreferences store the plist at that path instead, so the suite lives in
/// the temporary directory and never in the user's preferences.
final class TestDefaultsSuite {
    let name: String
    let defaults: UserDefaults

    init(_ prefix: String) {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("symeraseme-test-defaults", isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        name = directory.appendingPathComponent("\(prefix).\(UUID().uuidString)").path
        defaults = UserDefaults(suiteName: name)!
    }

    func remove() {
        defaults.removePersistentDomain(forName: name)
        try? FileManager.default.removeItem(atPath: name + ".plist")
    }
}

/// In-memory Keychain stand-in, so a test's `ServerManager` never reads or
/// migrates into the user's real Keychain item.
final class TestAPIKeyStore: APIKeyStore, @unchecked Sendable {
    private var value: String?
    func read() throws -> String? { value }
    func save(_ value: String) throws { self.value = value }
    func delete() throws { value = nil }
}
