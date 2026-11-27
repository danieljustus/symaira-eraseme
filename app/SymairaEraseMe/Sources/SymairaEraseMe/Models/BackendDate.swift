import Foundation

/// Parses the timestamp layouts the Rust/MCP backend actually emits.
///
/// The event store keeps SQLite wall-clock text (`2026-09-18 17:15:46`) while
/// generated/reported instants use RFC 3339 (`2026-06-29T13:00:27+00:00`).
/// Older stores can also carry a zone-less `T` form. `timeutil::parse` in the
/// backend treats zone-less values as UTC, so the app does the same.
enum BackendDate {
    static func parse(_ raw: String?) -> Date? {
        guard let raw else { return nil }
        let trimmed = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        var normalized = trimmed
        // SQLite's separator is a space; ISO 8601 expects `T`.
        if let separator = normalized.firstIndex(where: { $0 == " " }) {
            normalized.replaceSubrange(separator...separator, with: "T")
        }
        let hasOffset = normalized.hasSuffix("Z")
            || normalized.range(of: "[-+]\\d{2}:?\\d{2}$", options: .regularExpression) != nil
        if !hasOffset { normalized += "Z" }

        let withFraction = ISO8601DateFormatter()
        withFraction.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = withFraction.date(from: normalized) { return date }

        let wholeSeconds = ISO8601DateFormatter()
        wholeSeconds.formatOptions = [.withInternetDateTime]
        return wholeSeconds.date(from: normalized)
    }
}
