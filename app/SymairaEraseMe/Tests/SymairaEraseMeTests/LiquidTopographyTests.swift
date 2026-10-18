import AppKit
import Metal
import SwiftUI
import SymairaTheme
import XCTest
@testable import SymairaEraseMe

/// Native rendering and in-process actions, not physical mouse/VoiceOver acceptance.
final class LiquidTopographyTests: XCTestCase {
    @MainActor
    func testStatusAndTextContrastInBothAppearances() throws {
        for appearance in [NSAppearance.Name.aqua, .darkAqua] {
            let native = try XCTUnwrap(NSAppearance(named: appearance))
            native.performAsCurrentDrawingAppearance {
                let foregrounds = [
                    BrandColors.textPrimary, BrandColors.textSecondary, BrandColors.textMuted,
                    BrandColors.confirmed, BrandColors.pending, BrandColors.rejected, BrandColors.planned
                ]
                for foreground in foregrounds {
                    for background in [SymairaTokens.canvas, SymairaTokens.surface] {
                        XCTAssertGreaterThanOrEqual(contrast(foreground, background), 4.5, "\(appearance)")
                    }
                }
            }
        }
    }

    func testTopographyShaderGuardKeepsRealGPUs() throws {
        let device = try XCTUnwrap(MTLCreateSystemDefaultDevice())
        guard !device.name.contains("Paravirtual") else {
            throw XCTSkip("Paravirtual GPU: topography is intentionally disabled")
        }
        XCTAssertTrue(ContentView.topographyShaderSupported, device.name)
    }

    @MainActor
    func testAllDestinationsRenderAtMinimumAndDefaultWidths() async throws {
        _ = NSApplication.shared
        // No live user backend or credentials: all reads hit an unavailable local port.
        let oldHost = MCPClient.configuredHost
        let oldPort = MCPClient.configuredPort
        let oldDataDir = MCPClient.configuredDataDir
        MCPClient.configuredHost = "127.0.0.1"
        MCPClient.configuredPort = 9
        MCPClient.configuredDataDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("eraseme-design-no-credentials").path
        defer {
            MCPClient.configuredHost = oldHost
            MCPClient.configuredPort = oldPort
            MCPClient.configuredDataDir = oldDataDir
        }
        let manager = ServerManager(resourceURL: nil, executableURL: nil)
        let suiteName = "com.symaira.eraseme.render.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suiteName))
        defer { defaults.removePersistentDomain(forName: suiteName) }
        for width: CGFloat in [900, 1200] {
            for scheme: ColorScheme in [.light, .dark] {
                for destination in ContentView.NavItem.allCases {
                    let root = ContentView(initialNavigation: destination)
                        .environmentObject(manager)
                        .defaultAppStorage(defaults)
                        .environment(\.colorScheme, scheme)
                        .transaction { $0.animation = nil }
                    let name = "\(destination.rawValue)-\(scheme)-\(Int(width))"
                    try await render(root, name: name, width: width, scheme: scheme)
                }
                let settings = SettingsView(serverManager: manager)
                    .environmentObject(manager)
                    .defaultAppStorage(defaults)
                    .environment(\.colorScheme, scheme)
                    .transaction { $0.animation = nil }
                try await render(settings, name: "Settings-\(scheme)-\(Int(width))", width: width, scheme: scheme)
            }
        }
        XCTAssertFalse(manager.isRunning, "Visual review must never start a backend")
    }

    @MainActor
    func testSettingsButtonOpensDirectlyWithoutAppearanceSubmenu() throws {
        guard #available(macOS 14.4, *) else {
            throw XCTSkip("NSHostingMenu requires macOS 14.4")
        }
        var openedSettings = false
        let menu = NSHostingMenu(rootView: SymairaSettingsButton { openedSettings = true })
        menu.update()
        XCTAssertEqual(menu.items.count, 1)
        XCTAssertNil(try XCTUnwrap(menu.items.first).submenu)
        menu.performActionForItem(at: 0)
        XCTAssertTrue(openedSettings)
    }

    @MainActor
    private func render<V: View>(_ root: V, name: String, width: CGFloat, scheme: ColorScheme) async throws {
        let previousAppearance = NSApplication.shared.appearance
        let appearance = NSAppearance(named: scheme == .dark ? .darkAqua : .aqua)
        NSApplication.shared.appearance = appearance
        defer { NSApplication.shared.appearance = previousAppearance }
        let size = CGSize(width: width, height: width == 900 ? 600 : 800)
        // cacheDisplay leaves native glass transparent (alpha 0). These PNGs
        // verify layout, not live WindowServer glass/shader compositing.
        let host = NSHostingView(rootView: root)
        let window = NSWindow(contentRect: CGRect(origin: .zero, size: size),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        window.appearance = appearance
        host.frame = CGRect(origin: .zero, size: size)
        defer { window.close() }
        host.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(200))
        host.layoutSubtreeIfNeeded()
        let bitmap = try XCTUnwrap(host.bitmapImageRepForCachingDisplay(in: host.bounds), name)
        host.cacheDisplay(in: host.bounds, to: bitmap)
        let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]), name)
        XCTAssertGreaterThan(png.count, 10_000, "Blank native render: \(name)")
        XCTAssertEqual(host.bounds.width, width, "Content exceeds window: \(name)")
        XCTAssertLessThanOrEqual(host.bounds.height, size.height, name)
        if let directory = ProcessInfo.processInfo.environment["ERASEME_RENDER_DIR"] {
            let output = URL(fileURLWithPath: directory, isDirectory: true)
            try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
            try png.write(to: output.appendingPathComponent("\(name).png"))
        }
    }

    private func contrast(_ foreground: Color, _ background: Color) -> Double {
        func luminance(_ color: Color) -> Double {
            let rgb = NSColor(color).usingColorSpace(.sRGB)!
            func linear(_ value: Double) -> Double {
                value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
            }
            return 0.2126 * linear(rgb.redComponent)
                + 0.7152 * linear(rgb.greenComponent) + 0.0722 * linear(rgb.blueComponent)
        }
        let foregroundLuminance = luminance(foreground), backgroundLuminance = luminance(background)
        return (max(foregroundLuminance, backgroundLuminance) + 0.05)
            / (min(foregroundLuminance, backgroundLuminance) + 0.05)
    }
}
