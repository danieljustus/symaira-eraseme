import AppKit
import ApplicationServices
import SwiftUI
import SymairaTheme
import XCTest
@testable import SymairaEraseMe

/// Native geometry is independent from permission-gated accessibility actions.
final class NativeChromeTests: XCTestCase {
    @MainActor
    func testSharedToolbarGeometryInRealWindowHost() async throws {
        try await verifyNativeWindow(checkAccessibility: false)
    }

    @MainActor
    func testNativeAccessibilityNavigationActions() async throws {
        guard AXIsProcessTrusted() else {
            throw XCTSkip("Native SwiftUI accessibility actions require an AX-authorized test host")
        }
        try await verifyNativeWindow(checkAccessibility: true)
    }

    @MainActor
    private func verifyNativeWindow(checkAccessibility: Bool) async throws {
        _ = NSApplication.shared
        let previousPolicy = NSApp.activationPolicy()
        NSApp.setActivationPolicy(.regular)
        defer { NSApp.setActivationPolicy(previousPolicy) }
        let previousHost = MCPClient.configuredHost
        let previousPort = MCPClient.configuredPort
        let previousDataDir = MCPClient.configuredDataDir
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
        MCPClient.configuredDataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("eraseme-chrome-no-credentials").path
        defer {
            MCPClient.configuredHost = previousHost
            MCPClient.configuredPort = previousPort
            MCPClient.configuredDataDir = previousDataDir
        }
        let manager = ServerManager(resourceURL: nil, executableURL: nil)
        let suite = "com.symaira.eraseme.chrome.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let root = ContentView().environmentObject(manager).defaultAppStorage(defaults)
        let window = NSWindow(contentViewController: NSHostingController(rootView: root))
        window.title = "Symaira EraseMe"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
        window.titleVisibility = .visible
        window.titlebarAppearsTransparent = true
        window.toolbarStyle = .unified
        window.setContentSize(NSSize(width: 1080, height: 760))
        window.isReleasedWhenClosed = false
        window.makeKeyAndOrderFront(nil)
        defer { window.close() }
        try await Task.sleep(for: .milliseconds(250))
        window.contentView?.layoutSubtreeIfNeeded()

        let toolbar = try XCTUnwrap(window.toolbar, "The real native window must own the toolbar")
        XCTAssertEqual(toolbar.items.count, 2, "Exactly one sidebar toggle and one settings menu")
        XCTAssertEqual(toolbar.items.last?.label, "Settings")
        let toggle = try XCTUnwrap(toolbar.items.first?.view)
        let gear = try XCTUnwrap(toolbar.items.last?.view)
        XCTAssertEqual(toggle.frame.size, gear.frame.size, "Shared native controls have equal geometry")
        XCTAssertGreaterThanOrEqual(toggle.frame.width, 28)
        XCTAssertGreaterThanOrEqual(toggle.frame.height, 28)
        XCTAssertEqual(window.toolbarStyle, .unified)
        XCTAssertEqual(window.titleVisibility, .visible)
        XCTAssertFalse(manager.isRunning)
        guard checkAccessibility else { return }
        try await verifyNavigation(in: window)
        XCTAssertFalse(manager.isRunning)
    }

    @MainActor
    private func verifyNavigation(in window: NSWindow) async throws {
        var nodes = elements(window)
        for identifier in ["symaira-sidebar-toggle", "symaira-settings-button"] {
            XCTAssertEqual(nodes.filter { text($0, "accessibilityIdentifier") == identifier }.count, 1, identifier)
        }
        XCTAssertFalse(nodes.contains { text($0, "accessibilityIdentifier") == "eraseme-nav-Settings" })
        for item in ContentView.NavItem.sidebarItems {
            try press(identifier: "eraseme-nav-\(item.id)", in: window)
            try await Task.sleep(for: .milliseconds(100))
            nodes = elements(window)
            XCTAssertTrue(nodes.contains { text($0, "accessibilityIdentifier") == "symaira-settings-button" })
            XCTAssertTrue(nodes.contains { text($0, "accessibilityIdentifier") == "eraseme-content-\(item.id)" })
        }
        for _ in 0..<2 {
            try press(identifier: "symaira-sidebar-toggle", in: window)
            try await Task.sleep(for: .milliseconds(100))
        }
    }

    @MainActor
    private func text(_ object: NSObject, _ name: String) -> String? {
        let selector = NSSelectorFromString(name)
        guard object.responds(to: selector) else { return nil }
        return object.perform(selector)?.takeUnretainedValue() as? String
    }

    @MainActor
    private func elements(_ root: NSObject) -> [NSObject] {
        var seen = Set<ObjectIdentifier>()
        func walk(_ node: NSObject) -> [NSObject] {
            guard seen.insert(ObjectIdentifier(node)).inserted else { return [] }
            let selector = NSSelectorFromString("accessibilityChildren")
            var children = node.responds(to: selector)
                ? node.perform(selector)?.takeUnretainedValue() as? [NSObject] ?? [] : []
            if let window = node as? NSWindow {
                if let content = window.contentView { children.append(content) }
                children += window.toolbar?.items.compactMap(\.view) ?? []
            }
            if let view = node as? NSView { children += view.subviews }
            return [node] + children.flatMap(walk)
        }
        return walk(root)
    }

    @MainActor
    private func press(identifier: String, in window: NSWindow) throws {
        let selector = NSSelectorFromString("accessibilityPerformPress")
        let target = try XCTUnwrap(elements(window).first {
            text($0, "accessibilityIdentifier") == identifier && $0.responds(to: selector)
        }, "Missing native action \(identifier)")
        typealias Action = @convention(c) (AnyObject, Selector) -> Bool
        let action = unsafeBitCast(target.method(for: selector), to: Action.self)
        XCTAssertTrue(action(target, selector), identifier)
    }
}
