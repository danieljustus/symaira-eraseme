import Foundation
import Combine
import Darwin
import SymairaToolKit
import SymairaDaemonKit

/// Well-known Homebrew binary prefixes. GUI apps do not inherit a shell
/// PATH, so these are scanned explicitly (mirrors BinaryLocator's own
/// extra-directory list).
private let homebrewBinDirectories = ["/opt/homebrew/bin", "/usr/local/bin"]

/// Manages spawning/stopping the `symeraseme mcp` subprocess.
@MainActor
final class ServerManager: ObservableObject {
    @Published var isRunning = false
    @Published var pid: Int32?
    @Published var lastError: String?

    /// Configurable binary path (empty = auto-detect).
    @Published var binaryPath: String {
        didSet { UserDefaults.standard.set(binaryPath, forKey: "symeraseme_binary_path") }
    }

    /// Data directory configured by the Go backend when no override is set.
    nonisolated static let defaultDataDir = "~/.local/share/symeraseme"

    /// Configurable data directory.
    @Published var dataDir: String {
        didSet { UserDefaults.standard.set(dataDir, forKey: "symeraseme_data_dir") }
    }

    /// Configurable Anthropic API key.
    @Published var anthropicKey: String {
        didSet { UserDefaults.standard.set(anthropicKey, forKey: "symeraseme_anthropic_key") }
    }

    /// Server host.
    @Published var host: String {
        didSet { UserDefaults.standard.set(host, forKey: "symeraseme_host") }
    }

    /// Server port.
    @Published var port: Int {
        didSet { UserDefaults.standard.set(port, forKey: "symeraseme_port") }
    }

    /// Most recent stderr line from the server subprocess, for surfacing actionable diagnostics.
    @Published var lastStderrLine: String?

    /// Single source of truth for MCP server reachability. Both the sidebar
    /// footer and the Settings connection section read this, so the two
    /// indicators can never contradict each other.
    @Published var mcpReachable = false

    private let supervisor = DaemonSupervisor()
    private var reachabilityTask: Task<Void, Never>?

    init() {
        let defaults = UserDefaults.standard
        self.binaryPath = defaults.string(forKey: "symeraseme_binary_path") ?? ""
        self.dataDir = defaults.string(forKey: "symeraseme_data_dir") ?? ""
        self.anthropicKey = defaults.string(forKey: "symeraseme_anthropic_key") ?? ""
        self.host = defaults.string(forKey: "symeraseme_host") ?? "127.0.0.1"
        self.port = defaults.object(forKey: "symeraseme_port") as? Int ?? 8000

        setupSupervisor()
        startReachabilityPolling()
    }

    /// Shared reachability poller. Runs for the app's lifetime; every
    /// indicator (sidebar footer, Settings Connection section) derives from
    /// `mcpReachable`, so there is exactly one source of truth.
    private func startReachabilityPolling() {
        reachabilityTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.refreshReachability()
                try? await Task.sleep(for: .seconds(10))
            }
        }
    }

    /// Ping the MCP server and publish the result to `mcpReachable`.
    /// Returns the raw status so callers can distinguish an auth rejection
    /// from a transport failure in their error text.
    @discardableResult
    func refreshReachability() async -> ConnectionStatus {
        let status = await MCPClient.shared.ping()
        mcpReachable = (status == .connected)
        return status
    }

    private func setupSupervisor() {
        supervisor.onLog = { [weak self] logLine in
            guard logLine.isError else { return }
            Task { @MainActor [weak self] in
                self?.lastStderrLine = logLine.text
            }
        }

        supervisor.onStateChange = { [weak self] newState in
            Task { @MainActor [weak self] in
                guard let self else { return }
                switch newState {
                case .stopped:
                    self.isRunning = false
                    self.pid = nil
                case .starting:
                    self.isRunning = false
                    self.pid = nil
                    self.lastStderrLine = nil
                case .running(let pid):
                    self.isRunning = true
                    self.pid = pid
                case .failed(let error):
                    self.isRunning = false
                    self.pid = nil
                    // Surface the last stderr line so the user can diagnose the failure
                    self.lastError = Self.failureMessage(stderr: self.lastStderrLine, error: error)
                }
            }
        }
    }

    /// Start the MCP server subprocess.
    func start() {
        guard !isRunning else { return }
        lastError = nil
        lastStderrLine = nil

        let plan = resolveLaunchPlan()
        guard let executable = plan.executable else {
            lastError = Self.startFailureMessage(refusals: plan.refusals)
            return
        }
        let arguments = plan.arguments

        // Set up environment. Expand `~` once so the Go server and the
        // Swift client address the same token/database directory.
        let resolvedDataDir = dataDir.isEmpty ? nil : (dataDir as NSString).expandingTildeInPath
        var env = [String: String]()
        if let resolvedDataDir {
            env["SYMERASEME_DATA_DIR"] = resolvedDataDir
        }
        if !anthropicKey.isEmpty {
            env["ANTHROPIC_API_KEY"] = anthropicKey
        }

        // Update MCPClient with configured host/port
        MCPClient.configuredHost = host
        MCPClient.configuredPort = port
        // The server writes its per-run bearer token to <data_dir>/mcp_token;
        // the client must read the token from the SAME directory, otherwise
        // every request is rejected as unauthenticated while the UI blames
        // reachability. Assigned here (not just lazily at first access) so a
        // later Data Directory change takes effect without relaunching.
        if let resolvedDataDir {
            MCPClient.configuredDataDir = resolvedDataDir
        }

        Task.detached { [supervisor, executable, arguments, env] in
            _ = supervisor.start(executable: executable, arguments: arguments, environment: env)
        }
    }

    /// Stop the MCP server subprocess.
    func stop() {
        supervisor.stop()
    }

    // MARK: - Launch-plan resolution

    /// How to launch the server, or why no launch was possible.
    private struct LaunchPlan {
        let executable: URL?
        let arguments: [String]
        /// Human-readable reasons candidates were refused; empty when a
        /// usable executable was found.
        let refusals: [String]
    }

    /// Resolve the executable and arguments for `symeraseme mcp`.
    ///
    /// Discovery order:
    /// 1. User-configured Binary Path — always honoured when executable.
    /// 2. The binary bundled in the app's Helpers directory (with a
    ///    Resources fallback for older app bundles).
    /// 3. A sibling binary from the local Swift Package Manager build.
    /// 4. Homebrew/PATH, using BinaryLocator's strict check followed by the
    ///    app's safe Homebrew-compatible directory scan.
    ///
    /// There is deliberately no Python or `uv` fallback. The Go binary is
    /// self-contained and is the only backend the Swift app supports after
    /// the cutover.
    private func resolveLaunchPlan() -> LaunchPlan {
        let mcpArguments = ["mcp", "--host", host, "--port", "\(port)"]

        // 1. User-configured path — unchanged, always honoured.
        if !binaryPath.isEmpty, FileManager.default.isExecutableFile(atPath: binaryPath) {
            return LaunchPlan(executable: URL(fileURLWithPath: binaryPath), arguments: mcpArguments, refusals: [])
        }

        // 2. A released app carries the matching Go server in Resources.
        if let path = Self.bundledBinaryPath(resourceURL: Bundle.main.resourceURL) {
            return LaunchPlan(executable: URL(fileURLWithPath: path), arguments: mcpArguments, refusals: [])
        }

        // 3. The build script places a development Go binary next to the
        //    Swift executable so `swift run` remains self-contained.
        if let path = Self.developmentBinaryPath(executableURL: Bundle.main.executableURL) {
            return LaunchPlan(executable: URL(fileURLWithPath: path), arguments: mcpArguments, refusals: [])
        }

        var refusals: [String] = []

        // 4. Direct binary: strict locator first, then the relaxed
        //    Homebrew/PATH scan.
        if let path = findInPATH("symeraseme") {
            return LaunchPlan(executable: URL(fileURLWithPath: path), arguments: mcpArguments, refusals: [])
        }
        for directory in Self.scanDirectories() {
            if let diagnostic = Self.candidateRefusalDiagnostic(directory: directory, binaryName: "symeraseme") {
                refusals.append(diagnostic)
            }
        }

        return LaunchPlan(executable: nil, arguments: [], refusals: refusals)
    }

    /// Shared discovery: strict `BinaryLocator` first, then a relaxed scan
    /// of the Homebrew prefixes and PATH.
    ///
    /// `BinaryLocator` rejects any group-writable directory (e.g.
    /// `/opt/homebrew/bin`, mode 775, owned by the current user), which is
    /// the stock Homebrew layout — so the relaxed scan below fixes
    /// Homebrew installs without loosening appkit itself.
    private func findInPATH(_ name: String) -> String? {
        if let located = BinaryLocator().locate(name) {
            return located.url.path
        }
        for directory in Self.scanDirectories() {
            let candidate = URL(fileURLWithPath: directory).appendingPathComponent(name).path
            if FileManager.default.isExecutableFile(atPath: candidate),
               Self.isAcceptableExecutableDirectory(directory) {
                return candidate
            }
        }
        return nil
    }

    /// Return the bundled Go server from the standard nested-code directory.
    /// The Resources location remains a compatibility fallback for older apps.
    nonisolated static func bundledBinaryPath(resourceURL: URL?) -> String? {
        guard let resourceURL else { return nil }
        let contentsURL = resourceURL.deletingLastPathComponent()
        let candidates = [
            contentsURL.appendingPathComponent("Helpers/symeraseme").path,
            resourceURL.appendingPathComponent("symeraseme").path,
        ]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    /// Return the Go server produced next to the Swift executable by the
    /// local SPM build script. This keeps `swift run` useful without Homebrew.
    nonisolated static func developmentBinaryPath(executableURL: URL?) -> String? {
        guard let executableURL else { return nil }
        let path = executableURL.deletingLastPathComponent()
            .appendingPathComponent("symeraseme").path
        return FileManager.default.isExecutableFile(atPath: path) ? path : nil
    }

    // MARK: - Testable pure helpers

    /// Compose the failure banner message for a failed server transition.
    /// Returns the stderr text followed by the error when stderr is
    /// non-empty, otherwise the error alone.
    nonisolated static func failureMessage(stderr: String?, error: String) -> String {
        if let stderr, !stderr.isEmpty {
            return "\(stderr)\n\(error)"
        }
        return error
    }

    /// Compose the "no usable CLI" banner when discovery refused every
    /// candidate.
    nonisolated static func startFailureMessage(refusals: [String]) -> String {
        if refusals.isEmpty {
            return "Could not find the symeraseme CLI. Install the self-contained Go binary via Homebrew or set the Binary Path in Settings."
        }
        return "Could not start the symeraseme server — no usable CLI found:\n" + refusals.joined(separator: "\n")
    }

    /// Directories scanned by the relaxed fallback: the well-known
    /// Homebrew prefixes followed by PATH entries (deduplicated).
    nonisolated static func scanDirectories() -> [String] {
        let pathEntries = (ProcessInfo.processInfo.environment["PATH"] ?? "")
            .split(separator: ":")
            .map(String.init)
            .filter { !$0.isEmpty }
        var seen = Set<String>()
        return (homebrewBinDirectories + pathEntries).filter { seen.insert($0).inserted }
    }

    /// Relaxed, still-safe acceptance rule for executable directories.
    ///
    /// A directory is acceptable when it is owned by root or the current
    /// user and is not world-writable; group-writable is tolerated only
    /// when the owner is the current user (the standard Homebrew layout,
    /// `/opt/homebrew/bin` is 775 owned by the installing user).
    /// `BinaryLocator`'s strict check in appkit rejects group-writable
    /// directories outright, which is why this relaxed rule lives here and
    /// is applied only to the Homebrew-prefix/PATH fallbacks.
    nonisolated static func isAcceptableExecutableDirectory(_ path: String) -> Bool {
        directoryRefusalReason(path) == nil
    }

    /// Human-readable reason a directory failed the acceptance rule, or
    /// nil when it is acceptable.
    nonisolated static func directoryRefusalReason(_ path: String) -> String? {
        var statBuf = stat()
        guard stat(path, &statBuf) == 0 else { return "directory does not exist" }
        let owner = statBuf.st_uid
        let mode = statBuf.st_mode
        let currentUser = getuid()
        if (mode & S_IWOTH) != 0 {
            return "world-writable"
        }
        if owner != 0 && owner != currentUser {
            return (mode & S_IWGRP) != 0
                ? "group-writable, owner not current user"
                : "owner is not root or the current user"
        }
        return nil
    }

    /// Build the diagnostic line for a refused candidate directory, or nil
    /// when the directory holds no executable with `binaryName` or is
    /// acceptable.
    nonisolated static func candidateRefusalDiagnostic(directory: String, binaryName: String) -> String? {
        let candidate = URL(fileURLWithPath: directory).appendingPathComponent(binaryName).path
        guard FileManager.default.isExecutableFile(atPath: candidate) else { return nil }
        guard let reason = directoryRefusalReason(directory) else { return nil }
        return "found \(candidate) but directory not accepted (\(reason))"
    }

}
