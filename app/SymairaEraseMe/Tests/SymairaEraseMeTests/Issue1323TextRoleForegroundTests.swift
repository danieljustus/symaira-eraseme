import Foundation
import XCTest

/// Regression tests for #1323: `.symairaText(role)` sets the role's foreground
/// colour on the text itself, so a `.foregroundStyle(...)` chained after it is
/// dead code and the text renders in the role colour instead of the intended
/// tone. A view that wants a different tone passes `respectsForeground: false`
/// (or picks the role that carries the tone, e.g. `.secondary`).
final class Issue1323TextRoleForegroundTests: XCTestCase {

    /// Lines (1-based) where `.symairaText(...)` without `respectsForeground`
    /// is followed on the same line, or within the next two modifier lines of
    /// the same chain, by `.foregroundStyle(`/`.foregroundColor(`.
    static func deadForegroundLines(in source: String) -> [Int] {
        let lines = source.components(separatedBy: "\n")
        var hits: [Int] = []
        for (index, line) in lines.enumerated() {
            guard let call = line.range(of: ".symairaText(") else { continue }
            guard !line.contains("respectsForeground") else { continue }
            let rest = String(line[call.upperBound...])
            if isForegroundModifier(rest) {
                hits.append(index + 1)
                continue
            }
            for next in lines.dropFirst(index + 1).prefix(2) {
                let trimmed = next.trimmingCharacters(in: .whitespaces)
                // A line that does not continue the modifier chain starts another view.
                guard trimmed.hasPrefix(".") else { break }
                if trimmed.contains(".symairaText(") { break }
                if isForegroundModifier(trimmed) {
                    hits.append(index + 1)
                    break
                }
            }
        }
        return hits
    }

    private static func isForegroundModifier(_ text: String) -> Bool {
        text.contains(".foregroundStyle(") || text.contains(".foregroundColor(")
    }

    func testDetectorFlagsDeadForegroundAndAcceptsExplicitTones() {
        let dead = """
            Text("a")
                .symairaText(.caption)
                .foregroundStyle(BrandColors.textPrimary)
            Text("b").symairaText(.body).foregroundStyle(.red)
            Text("c")
                .symairaText(.callout)
                .lineLimit(1)
                .foregroundColor(.secondary)
            """
        XCTAssertEqual(Self.deadForegroundLines(in: dead), [2, 4, 6])

        let fine = """
            Text("a")
                .symairaText(.caption, respectsForeground: false)
                .foregroundStyle(BrandColors.textPrimary)
            Text("b")
                .symairaText(.secondary)
            Text("c")
                .foregroundStyle(BrandColors.textMuted)
                .symairaText(.caption)
            HStack {
                Text("d")
                    .symairaText(.callout)
                Text("•")
                    .foregroundStyle(BrandColors.textMuted)
            }
            """
        XCTAssertEqual(Self.deadForegroundLines(in: fine), [])
    }

    func testNoForegroundStyleAfterSymairaTextWithoutRespectsForegroundFalse() throws {
        let sources = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // SymairaEraseMeTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // package root
            .appendingPathComponent("Sources/SymairaEraseMe", isDirectory: true)
        let enumerator = try XCTUnwrap(
            FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
        var scanned = 0
        var offenders: [String] = []
        for case let file as URL in enumerator where file.pathExtension == "swift" {
            scanned += 1
            let text = try String(contentsOf: file, encoding: .utf8)
            for line in Self.deadForegroundLines(in: text) {
                offenders.append("\(file.lastPathComponent):\(line)")
            }
        }
        XCTAssertGreaterThan(scanned, 10, "Source scan must actually find the app sources")
        XCTAssertEqual(
            offenders, [],
            "`.foregroundStyle` after `.symairaText` has no effect; pass `respectsForeground: false`, drop it, or use the matching role")
    }
}
