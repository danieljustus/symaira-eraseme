import SwiftUI
import SymairaTheme

/// Product status vocabulary mapped to the shared, appearance-aware palette.
enum BrandColors {
    // No accent here on purpose: champagne (`SymairaTokens.selected`) belongs only to
    // selection, active controls and focus, which the shared AppKit controls and the
    // app-wide tint already apply. IDs, names and values stay in ink (DESIGN.md).

    static let textPrimary = SymairaTokens.ink
    static let textSecondary = SymairaTokens.ink2
    static let textMuted = SymairaTokens.ink3

    // MARK: - Status Colors
    static let confirmed = SymairaTokens.positive
    static let pending = SymairaTokens.warning
    static let rejected = SymairaTokens.critical
    static let overdue = SymairaTokens.critical
    static let planned = SymairaTokens.ink2

    // MARK: - Helper

    /// Map a request status string to its brand color.
    static func color(for status: String) -> Color {
        switch status.uppercased() {
        case "PLANNED": return planned
        case "SENT", "ACK", "AWAITING_ACK", "AWAITING_RESPONSE", "AWAITING_USER_ACTION",
             "RE_SCAN_DUE", "PENDING": return pending
        case "CONFIRMED", "COMPLETED": return confirmed
        case "REJECTED", "REJECTED_FINAL", "OVERDUE", "SEND_FAILED", "BOUNCE", "ESCALATED",
             "DPA_FILED": return rejected
        default: return textMuted
        }
    }
}
