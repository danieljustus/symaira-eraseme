import XCTest
@testable import SymairaEraseMe

/// Tests for #727: the app launches the self-contained Go server from the
/// bundle or the local SPM build before considering Homebrew.
final class ServerManagerBinaryDiscoveryTests: XCTestCase {

    private var tempDir: URL!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("sema-discovery-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        if let tempDir {
            try? FileManager.default.removeItem(at: tempDir)
        }
    }

    private func setMode(_ mode: Int, of path: String) throws {
        try FileManager.default.setAttributes([.posixPermissions: mode], ofItemAtPath: path)
    }

    private func makeExecutable(named name: String, contents: String = "#!/bin/sh\n") throws {
        let url = tempDir.appendingPathComponent(name)
        try Data(contents.utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
    }

    // MARK: - isAcceptableExecutableDirectory

    func testRejectsWorldWritableDirectory() throws {
        try setMode(0o777, of: tempDir.path)
        XCTAssertFalse(ServerManager.isAcceptableExecutableDirectory(tempDir.path))
    }

    func testAcceptsOwnerWritableDirectory() throws {
        try setMode(0o755, of: tempDir.path)
        XCTAssertTrue(ServerManager.isAcceptableExecutableDirectory(tempDir.path))
    }

    func testAcceptsGroupWritableDirectoryOwnedByCurrentUser() throws {
        // The stock Homebrew layout: /opt/homebrew/bin is 775 and owned by
        // the installing user (group admin). BinaryLocator's strict check
        // rejects this; the relaxed rule must accept it.
        try setMode(0o775, of: tempDir.path)
        XCTAssertTrue(ServerManager.isAcceptableExecutableDirectory(tempDir.path))
    }

    func testRejectsMissingDirectory() {
        XCTAssertFalse(ServerManager.isAcceptableExecutableDirectory(tempDir.path + "-does-not-exist"))
    }

    // A 0o775 directory NOT owned by the current user must be rejected, but
    // simulating foreign ownership requires root (chown), which is not
    // available here. The owner check itself is exercised indirectly:
    // world-writable is always rejected and group-writable is only
    // tolerated for the current user's own directories (the refusal reason
    // for a foreign owner is produced by the same code path).
    func testDirectoryRefusalReasonExplainsWorldWritable() throws {
        try setMode(0o777, of: tempDir.path)
        XCTAssertEqual(ServerManager.directoryRefusalReason(tempDir.path), "world-writable")
    }

    func testDirectoryRefusalReasonNilForAcceptableDirectory() throws {
        try setMode(0o775, of: tempDir.path)
        XCTAssertNil(ServerManager.directoryRefusalReason(tempDir.path))
    }

    // MARK: - candidateRefusalDiagnostic

    func testCandidateRefusalDiagnosticNilForAcceptableDirectory() throws {
        try setMode(0o755, of: tempDir.path)
        try makeExecutable(named: "symeraseme")
        XCTAssertNil(ServerManager.candidateRefusalDiagnostic(directory: tempDir.path, binaryName: "symeraseme"))
    }

    func testCandidateRefusalDiagnosticReportsRefusedCandidate() throws {
        try setMode(0o777, of: tempDir.path)
        try makeExecutable(named: "symeraseme")
        let diagnostic = ServerManager.candidateRefusalDiagnostic(directory: tempDir.path, binaryName: "symeraseme")
        XCTAssertNotNil(diagnostic)
        XCTAssertTrue(diagnostic!.contains("found \(tempDir.path)/symeraseme"))
        XCTAssertTrue(diagnostic!.contains("directory not accepted"))
        XCTAssertTrue(diagnostic!.contains("world-writable"))
    }

    func testCandidateRefusalDiagnosticNilWhenNoExecutable() throws {
        try setMode(0o777, of: tempDir.path)
        XCTAssertNil(ServerManager.candidateRefusalDiagnostic(directory: tempDir.path, binaryName: "symeraseme"))
    }

    // MARK: - Bundled and development binaries

    func testBundledBinaryPathFindsExecutableInResources() throws {
        let resources = tempDir.appendingPathComponent("Symaira EraseMe.app/Contents/Resources")
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        let binary = resources.appendingPathComponent("symeraseme")
        try Data("go-server".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)

        XCTAssertEqual(ServerManager.bundledBinaryPath(resourceURL: resources), binary.path)
    }

    func testBundledBinaryPathRejectsNonExecutableResource() throws {
        let resources = tempDir.appendingPathComponent("Resources", isDirectory: true)
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        let binary = resources.appendingPathComponent("symeraseme")
        try Data("go-server".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: binary.path)

        XCTAssertNil(ServerManager.bundledBinaryPath(resourceURL: resources))
    }

    func testDevelopmentBinaryPathFindsSiblingOfSwiftExecutable() throws {
        let products = tempDir.appendingPathComponent("Products/Debug", isDirectory: true)
        try FileManager.default.createDirectory(at: products, withIntermediateDirectories: true)
        let binary = products.appendingPathComponent("symeraseme")
        try Data("go-server".utf8).write(to: binary)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: binary.path)
        let swiftExecutable = products.appendingPathComponent("SymairaEraseMe")

        XCTAssertEqual(ServerManager.developmentBinaryPath(executableURL: swiftExecutable), binary.path)
    }

    // MARK: - Launch failure diagnostics

    func testStartFailureMessageExplainsMissingCli() {
        XCTAssertEqual(
            ServerManager.startFailureMessage(refusals: []),
            "Could not find the symeraseme CLI. Install the self-contained Go binary via Homebrew or set the Binary Path in Settings."
        )
    }

    func testStartFailureMessageIncludesAllRefusals() {
        let refusals = [
            "found /tmp/symeraseme but directory not accepted (world-writable)",
            "found /usr/bin/python3 but symeraseme module not importable",
        ]
        let message = ServerManager.startFailureMessage(refusals: refusals)
        XCTAssertTrue(message.hasPrefix("Could not start the symeraseme server — no usable CLI found:\n"))
        XCTAssertTrue(message.contains(refusals[0]))
        XCTAssertTrue(message.contains(refusals[1]))
        XCTAssertEqual(message.components(separatedBy: "\n").count, 3)
    }

    func testScanDirectoriesStartsWithHomebrewPrefixesAndDeduplicatesPath() {
        let directories = ServerManager.scanDirectories()
        XCTAssertEqual(Array(directories.prefix(2)), ["/opt/homebrew/bin", "/usr/local/bin"])
        XCTAssertEqual(directories.count, Set(directories).count)
    }
}
