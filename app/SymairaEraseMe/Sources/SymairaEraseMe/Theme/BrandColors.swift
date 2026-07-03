import SwiftUI
import SymairaTheme

/// Symaira brand color tokens — matches the HTML dashboard design system.
/// Shared brand values come from symaira-appkit; status colors and the
/// lighter card backings are EraseMe-specific and stay local.
enum BrandColors {
    // MARK: - Backgrounds (shared tokens; card backings deviate on purpose)
    static let bgDark = SymairaTheme.bgDark
    static let bgDarker = SymairaTheme.bgDarker
    static let bgCard = Color.white.opacity(0.065)
    static let bgCardHover = Color.white.opacity(0.1)

    // MARK: - Gold (shared tokens)
    static let goldPrimary = SymairaTheme.goldPrimary
    static let goldSecondary = SymairaTheme.goldSecondary
    static let goldShadow = SymairaTheme.goldShadow

    // MARK: - Text (shared tokens)
    static let textPrimary = SymairaTheme.textPrimary
    static let textSecondary = SymairaTheme.textSecondary
    static let textMuted = SymairaTheme.textMuted

    // MARK: - Status Colors
    static let confirmed = Color(red: 0xA7/255, green: 0xF3/255, blue: 0xD0/255)
    static let confirmedBg = Color(red: 0x06/255, green: 0x4E/255, blue: 0x3B/255).opacity(0.5)
    static let confirmedBorder = Color(red: 0x10/255, green: 0xB9/255, blue: 0x81/255).opacity(0.3)

    static let pending = Color(red: 0xFD/255, green: 0xE6/255, blue: 0x8A/255)
    static let pendingBg = Color(red: 0x78/255, green: 0x35/255, blue: 0x0F/255).opacity(0.5)
    static let pendingBorder = Color(red: 0xF5/255, green: 0x9E/255, blue: 0x0B/255).opacity(0.3)

    static let rejected = Color(red: 0xFC/255, green: 0xA5/255, blue: 0xA5/255)
    static let rejectedBg = Color(red: 0x5C/255, green: 0x1D/255, blue: 0x1D/255).opacity(0.5)
    static let rejectedBorder = Color(red: 0xEF/255, green: 0x44/255, blue: 0x44/255).opacity(0.3)

    static let overdue = Color(red: 0xFE/255, green: 0xCA/255, blue: 0xCA/255)
    static let overdueBg = Color(red: 0x7F/255, green: 0x1D/255, blue: 0x1D/255).opacity(0.6)
    static let overdueBorder = Color(red: 0xEF/255, green: 0x44/255, blue: 0x44/255).opacity(0.4)

    static let planned = Color(red: 0xDB/255, green: 0xEA/255, blue: 0xFE/255)
    static let plannedBg = Color(red: 0x1E/255, green: 0x3A/255, blue: 0x8A/255).opacity(0.4)
    static let plannedBorder = Color(red: 0x3B/255, green: 0x82/255, blue: 0xF6/255).opacity(0.3)

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

    /// Map a request status string to its background color.
    static func backgroundColor(for status: String) -> Color {
        switch status.uppercased() {
        case "PLANNED": return plannedBg
        case "SENT", "AWAITING_ACK", "AWAITING_RESPONSE": return pendingBg
        case "CONFIRMED": return confirmedBg
        case "REJECTED", "REJECTED_FINAL": return rejectedBg
        case "OVERDUE": return overdueBg
        default: return Color.white.opacity(0.05)
        }
    }
}
