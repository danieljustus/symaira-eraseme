import SwiftUI

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
    }

    private var header: some View {
        VStack(alignment: .leading) {
            Text("Settings")
                .font(.largeTitle.bold())
                .foregroundStyle(BrandColors.textPrimary)
            Text("Configure the MCP server connection and app preferences")
                .font(.caption)
                .foregroundStyle(BrandColors.textMuted)
        }
    }

    // MARK: - Server Section

    private var serverSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("MCP Server")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            HStack(spacing: 16) {
                // Status indicator
                HStack(spacing: 8) {
                    Circle()
                        .fill(serverManager.isRunning ? BrandColors.confirmed : BrandColors.rejected)
                        .frame(width: 10, height: 10)
                    Text(serverManager.isRunning ? "Running" : "Stopped")
                        .font(.subheadline)
                        .foregroundStyle(BrandColors.textSecondary)
                    if let pid = serverManager.pid {
                        Text("PID \(pid)")
                            .font(.caption)
                            .foregroundStyle(BrandColors.textMuted)
                    }
                }

                Spacer()

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
        .interactiveGlassCard()
    }

    // MARK: - Connection Section

    private var connectionSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Connection")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            HStack(spacing: 12) {
                TextField("Host", text: $serverManager.host)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 180)
                    .onChange(of: serverManager.host) { _, _ in
                        MCPClient.configuredHost = serverManager.host
                    }
                TextField("Port", value: $serverManager.port, format: .number)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 100)
                    .onChange(of: serverManager.port) { _, _ in
                        MCPClient.configuredPort = serverManager.port
                    }
                Button("Test Connection") {
                    Task { await vm.checkConnection() }
                }
                .buttonStyle(.bordered)
                .tint(BrandColors.goldPrimary)
                .disabled(vm.isChecking)
            }

            HStack(spacing: 8) {
                Circle()
                    .fill(vm.isConnected ? BrandColors.confirmed : BrandColors.rejected)
                    .frame(width: 8, height: 8)
                Text(vm.isConnected ? "Connected" : "Not connected")
                    .font(.caption)
                    .foregroundStyle(BrandColors.textSecondary)
                if vm.isChecking {
                    ProgressView()
                        .controlSize(.mini)
                }
            }

            if let error = vm.lastCheckError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(BrandColors.rejected)
            }
        }
        .interactiveGlassCard()
    }

    // MARK: - Configuration Section

    private var configurationSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Configuration")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            Group {
                HStack {
                    Text("Binary Path")
                        .frame(width: 120, alignment: .trailing)
                        .font(.subheadline)
                        .foregroundStyle(BrandColors.textSecondary)
                    TextField("Auto-detect", text: $serverManager.binaryPath)
                        .textFieldStyle(.roundedBorder)
                }

                HStack {
                    Text("Data Directory")
                        .frame(width: 120, alignment: .trailing)
                        .font(.subheadline)
                        .foregroundStyle(BrandColors.textSecondary)
                    TextField("Default (~/.symeraseme)", text: $serverManager.dataDir)
                        .textFieldStyle(.roundedBorder)
                }

                HStack {
                    Text("API Key")
                        .frame(width: 120, alignment: .trailing)
                        .font(.subheadline)
                        .foregroundStyle(BrandColors.textSecondary)
                    SecureField("ANTHROPIC_API_KEY", text: $serverManager.anthropicKey)
                        .textFieldStyle(.roundedBorder)
                }
            }
        }
        .interactiveGlassCard()
    }

    // MARK: - HTML Dashboard

    private var htmlDashboardSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("HTML Dashboard")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            Text("Generate and open the HTML dashboard in your browser as a fallback.")
                .font(.subheadline)
                .foregroundStyle(BrandColors.textSecondary)

            HStack(spacing: 12) {
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
                    .font(.caption)
                    .foregroundStyle(BrandColors.textMuted)
                    .lineLimit(1)
            }
        }
        .interactiveGlassCard()
    }

    // MARK: - About

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("About")
                .font(.headline)
                .foregroundStyle(BrandColors.textPrimary)

            HStack {
                Text("Symaira EraseMe")
                    .font(.subheadline)
                    .foregroundStyle(BrandColors.goldPrimary)
                Text("•")
                    .foregroundStyle(BrandColors.textMuted)
                Text("Native Dashboard")
                    .font(.subheadline)
                    .foregroundStyle(BrandColors.textSecondary)
            }

            Text("Connects to the Symaira MCP server for GDPR/CCPA data broker removal automation.")
                .font(.caption)
                .foregroundStyle(BrandColors.textMuted)
        }
        .interactiveGlassCard()
    }
}
