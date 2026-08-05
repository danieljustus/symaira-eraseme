import SwiftUI
import SymairaTheme

/// Settings view — server management, connection, and configuration.
struct SettingsView: View {
    @EnvironmentObject var serverManager: ServerManager
    @StateObject private var vm: SettingsViewModel

    init(serverManager: ServerManager) {
        _vm = StateObject(wrappedValue: SettingsViewModel(serverManager: serverManager))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                serverSection
                connectionSection
                configurationSection
                htmlDashboardSection
                aboutSection
            }
            .padding(24)
        }
        .background(Color.clear)
        .task { await vm.checkConnection() }
        .onChange(of: serverManager.isRunning) { _, running in
            guard running else { return }
            Task {
                // The server needs a moment to accept connections. Retry
                // briefly so the Connection section reflects the running
                // server without pressing Test Connection or waiting for
                // the next shared poll.
                for _ in 0..<6 {
                    await vm.checkConnection()
                    if vm.isConnected { break }
                    try? await Task.sleep(for: .seconds(2))
                }
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading) {
            Text("Settings")
                .symairaText(.display)
                .foregroundStyle(BrandColors.textPrimary)
            Text("Configure the MCP server connection and app preferences")
                .symairaText(.caption)
                .foregroundStyle(BrandColors.textMuted)
        }
    }

    // MARK: - Server Section

    private var serverSection: some View {
        SymairaFormSection("MCP Server") {
            HStack(spacing: SymairaSpacing.large) {
                HStack(spacing: SymairaSpacing.medium) {
                    SymairaStatusLabel(
                        serverManager.isRunning ? "Running" : "Stopped",
                        tone: serverManager.isRunning ? .positive : .critical
                    )
                    if let pid = serverManager.pid {
                        Text("PID \(String(pid))")
                            .symairaText(.monoSmall)
                    }
                }

                Spacer(minLength: SymairaSpacing.medium)

                if serverManager.isRunning {
                    Button("Stop Server") {
                        serverManager.stop()
                    }
                    .buttonStyle(.bordered)
                    .tint(BrandColors.rejected)
                } else {
                    Button("Start Server") {
                        serverManager.start()
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(BrandColors.goldPrimary)
                    .foregroundStyle(BrandColors.bgDark)
                }
            }

            if let error = serverManager.lastError {
                ErrorBanner(message: error) { serverManager.lastError = nil }
            }
        }
    }

    // MARK: - Connection Section

    private var connectionSection: some View {
        SymairaFormSection("Connection") {
            SymairaFormRow("Host") {
                TextField("Host", text: $serverManager.host)
                    .textFieldStyle(.symaira)
                    .frame(minWidth: 180, idealWidth: 260, maxWidth: 360)
                    .onChange(of: serverManager.host) { _, _ in
                        MCPClient.configuredHost = serverManager.host
                    }
            }

            SymairaFormDivider()

            SymairaFormRow("Port") {
                TextField("Port", value: $serverManager.port, format: .number.grouping(.never))
                    .textFieldStyle(.symaira)
                    .frame(width: 120)
                    .onChange(of: serverManager.port) { _, _ in
                        MCPClient.configuredPort = serverManager.port
                    }
            }

            SymairaFormDivider()

            SymairaFormRow("Test Connection") {
                Button("Test Connection") {
                    Task { await vm.checkConnection() }
                }
                .buttonStyle(.bordered)
                .tint(BrandColors.goldPrimary)
                .disabled(vm.isChecking)
            }

            HStack(spacing: SymairaSpacing.medium) {
                SymairaStatusLabel(
                    vm.isConnected ? "Connected" : "Not connected",
                    tone: vm.isConnected ? .positive : .critical
                )
                if vm.isChecking {
                    ProgressView()
                        .controlSize(.mini)
                        .accessibilityLabel("Checking connection")
                }
            }

            if let error = vm.lastCheckError {
                Text(error)
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.rejected)
            }
        }
    }

    // MARK: - Configuration Section

    private var configurationSection: some View {
        SymairaFormSection("Configuration") {
            SymairaFormRow("Binary Path") {
                TextField("Auto-detect", text: $serverManager.binaryPath)
                    .textFieldStyle(.symaira)
                    .frame(minWidth: 240, idealWidth: 360, maxWidth: 520)
            }

            SymairaFormDivider()

            SymairaFormRow("Data Directory") {
                TextField("Default (~/.symeraseme)", text: $serverManager.dataDir)
                    .textFieldStyle(.symaira)
                    .frame(minWidth: 240, idealWidth: 360, maxWidth: 520)
            }

            SymairaFormDivider()

            SymairaFormRow("API Key") {
                SecureField("ANTHROPIC_API_KEY", text: $serverManager.anthropicKey)
                    .textFieldStyle(.symaira)
                    .frame(minWidth: 240, idealWidth: 360, maxWidth: 520)
            }
        }
    }

    // MARK: - HTML Dashboard

    private var htmlDashboardSection: some View {
        SymairaFormSection(
            "HTML Dashboard",
            footer: "Generate and open the HTML dashboard in your browser as a fallback."
        ) {
            HStack(spacing: SymairaSpacing.medium) {
                Button("Generate & Open Dashboard") {
                    Task { await vm.openHTMLDashboard() }
                }
                .buttonStyle(.bordered)
                .tint(BrandColors.goldPrimary)

                if let path = vm.dashboardPath {
                    Button("Show in Finder") {
                        vm.showInFinder(path)
                    }
                    .buttonStyle(.bordered)
                }
            }

            if let path = vm.dashboardPath {
                Text(path)
                    .symairaText(.monoSmall)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
        }
    }

    // MARK: - About

    private var aboutSection: some View {
        SymairaFormSection("About") {
            HStack {
                Text("Symaira EraseMe")
                    .symairaText(.callout)
                    .foregroundStyle(BrandColors.goldPrimary)
                Text("•")
                    .foregroundStyle(BrandColors.textMuted)
                    .accessibilityHidden(true)
                Text("Native Dashboard")
                    .symairaText(.callout)
                    .foregroundStyle(BrandColors.textSecondary)
            }

            Text("Connects to the Symaira MCP server for GDPR/CCPA data broker removal automation.")
                .symairaText(.caption)
        }
    }
}
