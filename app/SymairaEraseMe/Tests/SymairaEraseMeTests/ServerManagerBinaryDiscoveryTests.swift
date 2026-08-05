import XCTest
@testable import SymairaEraseMe

/// Tests for #616: the relaxed executable-directory acceptance rule that
/// lets a stock Homebrew install (`/opt/homebrew/bin`, mode 775, owned by
/// the current user) be used without weakening appkit's BinaryLocator,
/// plus the refusal diagnostics and the verified python fallback.
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

    private func makeExecutable(named name: String) throws {
        let url = tempDir.appendingPathComponent(name)
        try Data("#!/bin/sh\n".utf8).write(to: url)
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

    // MARK: - moduleImportable

    func testModuleImportableAcceptsImportableModule() throws {
        let python = "/usr/bin/python3"
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: python), "python3 not available")
        XCTAssertTrue(ServerManager.moduleImportable(python: python, module: "sys"))
    }

    func testModuleImportableRejectsMissingModule() throws {
        let python = "/usr/bin/python3"
        try XCTSkipUnless(FileManager.default.isExecutableFile(atPath: python), "python3 not available")
        XCTAssertFalse(ServerManager.moduleImportable(python: python, module: "definitely_not_a_real_module_xyz"))
    }

    func testModuleImportableRejectsMissingInterpreter() {
        XCTAssertFalse(ServerManager.moduleImportable(python: "/nonexistent/python3"))
    }
}
