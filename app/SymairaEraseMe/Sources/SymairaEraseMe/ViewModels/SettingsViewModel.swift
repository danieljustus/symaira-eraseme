import Foundation
import AppKit

/// View model for the Settings view.
@MainActor
final class SettingsViewModel: ObservableObject {
    @Published var isConnected = false
    @Published var isChecking = false
    @Published var lastCheckError: String?
    @Published var dashboardPath: String?

    let serverManager: ServerManager

    init(serverManager: ServerManager) {
        self.serverManager = serverManager
    }

    /// Check if the MCP server is reachable.
    func checkConnection() async {
        isChecking = true
        lastCheckError = nil
        defer { isChecking = false }

        let client = MCPClient.shared
        switch await client.ping() {
        case .connected:
            isConnected = true
        case .unauthorized:
            isConnected = false
            lastCheckError = "Server rejected the request (401 Unauthorized) — the Data Directory does not match the server's token directory"
        case .unreachable:
            isConnected = false
            lastCheckError = "Server not reachable at \(MCPClient.configuredHost):\(MCPClient.configuredPort)"
        }
    }

    /// Generate the HTML dashboard and open it.
    func openHTMLDashboard() async {
        do {
            let response: GenerateDashboardResponse = try await MCPClient.shared.callTool("generate_dashboard", arguments: [
                "auto_open": true
            ])
            if let path = response.outputFile {
                dashboardPath = path
            }
        } catch {
            lastCheckError = error.localizedDescription
        }
    }

    /// Open a file path in Finder.
    func showInFinder(_ path: String) {
        let url = URL(fileURLWithPath: path)
        NSWorkspace.shared.activateFileViewerSelecting([url])
    }

    /// Open a URL in the default browser.
    func openURL(_ urlString: String) {
        if let url = URL(string: urlString) {
            NSWorkspace.shared.open(url)
        }
    }
}
