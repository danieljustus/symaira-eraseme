import Foundation
import Combine
import SymairaToolKit

/// Manages spawning/stopping the `symeraseme serve` subprocess.
///
/// NOTE (DaemonKit v0.2 requirements): long-running daemon supervisor —
/// start/stop lifecycle, terminate-then-interrupt escalation, published
/// state. Second requirements donor (besides seek's EngineManager) for
/// symaira-appkit's future SymairaDaemonKit; only binary discovery is
/// shared for now.
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

    private var process: Process?

    init() {
        let defaults = UserDefaults.standard
        self.binaryPath = defaults.string(forKey: "symeraseme_binary_path") ?? ""
        self.dataDir = defaults.string(forKey: "symeraseme_data_dir") ?? ""
        self.anthropicKey = defaults.string(forKey: "symeraseme_anthropic_key") ?? ""
        self.host = defaults.string(forKey: "symeraseme_host") ?? "127.0.0.1"
        self.port = defaults.object(forKey: "symeraseme_port") as? Int ?? 8000
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

    /// Start the MCP server subprocess.
    func start() {
        guard !isRunning else { return }
        lastError = nil

        let proc = Process()
        proc.executableURL = findExecutable()
        proc.arguments = buildArguments()

        // Set up environment
        var env = ProcessInfo.processInfo.environment
        if !dataDir.isEmpty {
            env["SYMERASEME_DATA_DIR"] = dataDir
        }
        if !anthropicKey.isEmpty {
            env["ANTHROPIC_API_KEY"] = anthropicKey
        }
        proc.environment = env

        // Capture output for debugging
        let outPipe = Pipe()
        let errPipe = Pipe()
        proc.standardOutput = outPipe
        proc.standardError = errPipe

        // Update MCPClient with configured host/port
        MCPClient.configuredHost = host
        MCPClient.configuredPort = port

        do {
            try proc.run()
            process = proc
            pid = proc.processIdentifier
            isRunning = true

            // Monitor process termination
            DispatchQueue.global(qos: .utility).async { [weak self] in
                proc.waitUntilExit()
                Task { @MainActor in
                    self?.isRunning = false
                    self?.pid = nil
                    self?.process = nil
                    if proc.terminationStatus != 0 {
                        let errData = errPipe.fileHandleForReading.readDataToEndOfFile()
                        self?.lastError = "Process exited with status \(proc.terminationStatus): \(String(data: errData, encoding: .utf8) ?? "")"
                    }
                }
            }
        } catch {
            lastError = "Failed to start: \(error.localizedDescription)"
        }
    }

    /// Stop the MCP server subprocess.
    func stop() {
        guard let proc = process, proc.isRunning else {
            isRunning = false
            pid = nil
            return
        }

        proc.terminate()

        // Give it 2 seconds to terminate gracefully, then force kill
        DispatchQueue.global().asyncAfter(deadline: .now() + 2) { [weak self] in
            if proc.isRunning {
                proc.interrupt()
            }
            Task { @MainActor [weak self] in
                self?.isRunning = false
                self?.pid = nil
                self?.process = nil
            }
        }
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
