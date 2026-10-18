import SwiftUI
import SymairaTheme

/// Shared status badge with a visible label and semantic tone.
struct StatusBadge: View {
    let status: String

    var body: some View {
        SymairaBadge(displayText, tone: tone, systemImage: tone.systemImage)
    }

    private var tone: SymairaTone {
        switch status.uppercased() {
        case "CONFIRMED": return .positive
        case "REJECTED", "REJECTED_FINAL", "OVERDUE": return .critical
        case "AWAITING_ACK", "AWAITING_RESPONSE": return .warning
        case "SENT": return .informative
        case "PLANNED": return .neutral
        default: return .neutral
        }
    }

    private var displayText: String {
        switch status.uppercased() {
        case "PLANNED": return "Planned"
        case "SENT": return "Sent"
        case "AWAITING_ACK": return "Awaiting ACK"
        case "AWAITING_RESPONSE": return "Awaiting Response"
        case "CONFIRMED": return "Confirmed"
        case "REJECTED", "REJECTED_FINAL": return "Rejected"
        case "OVERDUE": return "Overdue"
        default: return status
        }
    }
}

/// Summary stat card for the dashboard.
struct StatCard: View {
    let title: String
    let value: Int
    var subtitle: String? = nil

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            Text(title)
                .symairaText(.sectionLabel)
                .textCase(.uppercase)
            Text("\(value)")
                .font(SymairaTypography.stat)
                .foregroundStyle(SymairaTokens.ink)
            if let subtitle {
                Text(subtitle)
                    .symairaText(.caption)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard()
        .topoProtected(hill: true)
    }
}

extension Label where Title == Text, Icon == SymairaIconView<Int> {
    /// Caption-sized `Label(_:symairaIcon:)` for runtime strings.
    @MainActor
    init(_ title: String, symairaIcon icon: SymairaIcon, size: CGFloat) {
        self.init {
            Text(title)
        } icon: {
            SymairaIconView(icon, size: size)
        }
    }
}
