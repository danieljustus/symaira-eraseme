import AppKit
import ApplicationServices
import SwiftUI
import SymairaTheme
import XCTest
@testable import SymairaEraseMe

final class SettingsAppearanceTests: XCTestCase {
    @MainActor
    func testProductionPickerPersistsAndAppliesToNativeWindows() async throws {
        // The shared picker is SwiftUI segments; only an AX-authorized host can press them.
        guard AXIsProcessTrusted() else {
            throw XCTSkip("Pressing SwiftUI segments requires an AX-authorized test host")
        }
        _ = NSApplication.shared
        let previousAppearance = NSApp.appearance
        let previousHost = MCPClient.configuredHost
        let previousPort = MCPClient.configuredPort
        let previousDataDir = MCPClient.configuredDataDir
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
        MCPClient.configuredDataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("settings-appearance-no-credentials").path
        let suite = "com.symaira.eraseme.appearance-test.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer {
            defaults.removePersistentDomain(forName: suite)
            NSApp.appearance = previousAppearance
            MCPClient.configuredHost = previousHost
            MCPClient.configuredPort = previousPort
            MCPClient.configuredDataDir = previousDataDir
        }
        let manager = ServerManager(resourceURL: nil, executableURL: nil)
        let root = SettingsView(serverManager: manager)
            .environmentObject(manager)
            .defaultAppStorage(defaults)
        let window = NSWindow(contentViewController: NSHostingController(rootView: root))
        let otherWindow = NSWindow(contentViewController: NSHostingController(rootView: Text("Workspace")))
        window.isReleasedWhenClosed = false
        otherWindow.isReleasedWhenClosed = false
        window.setContentSize(NSSize(width: 720, height: 680))
        defer { window.close(); otherWindow.close() }
        window.contentView?.layoutSubtreeIfNeeded()
        window.orderFront(nil)
        let restoreAccessibility = enableEnhancedAccessibility()
        defer { restoreAccessibility() }
        try await Task.sleep(for: .milliseconds(300))
        for choice in [SymairaAppearance.dark, .light, .system] {
            try press(in: window, "Missing segment \(choice.title)") {
                text($0, "accessibilityRole") == NSAccessibility.Role.button.rawValue
                    && text($0, "accessibilityLabel") == choice.title
            }
            try await Task.sleep(for: .milliseconds(150))
            XCTAssertEqual(defaults.string(forKey: "symeraseme_appearance"), choice.rawValue)
            XCTAssertEqual(NSApp.appearance?.name, choice.nativeAppearance?.name)
            XCTAssertEqual(window.effectiveAppearance.name, otherWindow.effectiveAppearance.name)
        }
        XCTAssertFalse(manager.isRunning)
    }
}
