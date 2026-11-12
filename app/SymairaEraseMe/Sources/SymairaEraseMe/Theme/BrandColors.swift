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
        case "SENT", "AWAITING_ACK", "AWAITING_RESPONSE": return pending
        case "CONFIRMED": return confirmed
        case "REJECTED", "REJECTED_FINAL": return rejected
        case "OVERDUE": return overdue
        default: return textMuted
        }
    }
}
