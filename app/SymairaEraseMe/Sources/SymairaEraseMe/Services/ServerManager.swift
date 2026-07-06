import Foundation
import Combine
import SymairaToolKit
import SymairaDaemonKit

/// Manages spawning/stopping the `symeraseme serve` subprocess.
@MainActor
final class ServerManager: ObservableObject {
    @Published var isRunning = false
    @Published var pid: Int32?
    @Published var lastError: String?

    /// Configurable binary path (empty = auto-detect).
    @Published var binaryPath: String {
        didSet { UserDefaults.standard.set(binaryPath, forKey: "symeraseme_binary_path") }
    }

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

    private let supervisor = DaemonSupervisor()

    init() {
        let defaults = UserDefaults.standard
        self.binaryPath = defaults.string(forKey: "symeraseme_binary_path") ?? ""
        self.dataDir = defaults.string(forKey: "symeraseme_data_dir") ?? ""
        self.anthropicKey = defaults.string(forKey: "symeraseme_anthropic_key") ?? ""
        self.host = defaults.string(forKey: "symeraseme_host") ?? "127.0.0.1"
        self.port = defaults.object(forKey: "symeraseme_port") as? Int ?? 8000

        setupSupervisor()
    }

    /// Detect `symeraseme` binary on PATH or via common locations.
    func detectBinary() -> String? {
        // 1. User-configured path
        if !binaryPath.isEmpty, FileManager.default.isExecutableFile(atPath: binaryPath) {
            return binaryPath
        }

        // 2. Check PATH for `symeraseme`
        if let path = findInPATH("symeraseme") {
            return path
        }

        // 3. Try `uv run symeraseme`
        if findInPATH("uv") != nil {
            return nil // Will use "uv run symeraseme serve"
        }

        // 4. Try `python -m symeraseme`
        if findInPATH("python3") != nil || findInPATH("python") != nil {
            return nil // Will use "python -m symeraseme serve"
        }

        return nil
    }

    private func setupSupervisor() {
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
                case .running(let pid):
                    self.isRunning = true
                    self.pid = pid
                case .failed(let error):
                    self.isRunning = false
                    self.pid = nil
                    self.lastError = error
                }
            }
        }
    }

    /// Start the MCP server subprocess.
    func start() {
        guard !isRunning else { return }
        lastError = nil

        let executable = findExecutable()
        let arguments = buildArguments()

        // Set up environment
        var env = [String: String]()
        if !dataDir.isEmpty {
            env["SYMERASEME_DATA_DIR"] = dataDir
        }
        if !anthropicKey.isEmpty {
            env["ANTHROPIC_API_KEY"] = anthropicKey
        }

        // Update MCPClient with configured host/port
        MCPClient.configuredHost = host
        MCPClient.configuredPort = port

        _ = supervisor.start(executable: executable, arguments: arguments, environment: env)
    }

    /// Stop the MCP server subprocess.
    func stop() {
        supervisor.stop()
    }

    // MARK: - Private Helpers

    private func findExecutable() -> URL {
        if let binPath = detectBinary(), !binPath.isEmpty {
            return URL(fileURLWithPath: binPath)
        }

        // Fallback: try uv
        if let uvPath = findInPATH("uv") {
            return URL(fileURLWithPath: uvPath)
        }

        // Fallback: try python3
        if let pyPath = findInPATH("python3") {
            return URL(fileURLWithPath: pyPath)
        }

        // Last resort
        return URL(fileURLWithPath: "/usr/bin/env")
    }

    private func buildArguments() -> [String] {
        let binPath = detectBinary()

        if binPath == nil || binPath?.isEmpty == true {
            // No direct binary found — try via uv or python
            if findInPATH("uv") != nil {
                return ["run", "symeraseme", "serve", "--host", host, "--port", "\(port)"]
            } else if findInPATH("python3") != nil {
                return ["-m", "symeraseme", "serve", "--host", host, "--port", "\(port)"]
            } else if findInPATH("python") != nil {
                return ["-m", "symeraseme", "serve", "--host", host, "--port", "\(port)"]
            }
        }

        return ["serve", "--host", host, "--port", "\(port)"]
    }

    /// Shared discovery (bundle → exe dir → PATH → Homebrew prefixes).
    /// GUI apps do not inherit a shell PATH, so the Homebrew fallbacks in
    /// BinaryLocator matter — the old PATH-only lookup missed brew installs.
    private func findInPATH(_ name: String) -> String? {
        BinaryLocator().locate(name)?.url.path
    }
}
