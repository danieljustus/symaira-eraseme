import SwiftUI
import SymairaTheme

/// Shared status badge with a visible label and semantic tone.
struct StatusBadge: View {
    let status: String

    /// Request statuses from the backend projection plus manual-task states.
    static let vocabulary: [String: (label: String, tone: SymairaTone)] = [
        "PLANNED": ("Planned", .neutral),
        "SENT": ("Sent", .informative),
        "ACK": ("Acknowledged", .informative),
        "AWAITING_ACK": ("Awaiting confirmation of receipt", .warning),
        "AWAITING_RESPONSE": ("Awaiting Response", .warning),
        "AWAITING_USER_ACTION": ("Action Needed", .warning),
        "RE_SCAN_DUE": ("Re-scan Due", .informative),
        "CONFIRMED": ("Confirmed", .positive),
        "REJECTED": ("Rejected", .critical),
        "REJECTED_FINAL": ("Rejected", .critical),
        "OVERDUE": ("Overdue", .critical),
        "SEND_FAILED": ("Send Failed", .critical),
        "BOUNCE": ("Bounced", .critical),
        "ESCALATED": ("Escalated", .critical),
        "DPA_FILED": ("DPA Filed", .critical),
        "PENDING": ("Pending", .warning),
        "COMPLETED": ("Completed", .positive),
    ]

    /// Shorter labels for table cells. `SymairaBadge` never truncates, so the
    /// full "Awaiting confirmation of receipt" pill (about 221 pt) covered the
    /// next column in Requests and Calendar. The full label stays the tooltip
    /// and the accessibility label.
    static let compactLabels: [String: String] = [
        "AWAITING_ACK": "Awaiting Receipt",
    ]

    /// Status column width that fits the widest compact pill ("Awaiting
    /// Response", about 135 pt); the table adds its own cell padding.
    /// ponytail: fixed for the current English labels and label font; measure
    /// the widest compact label instead once labels are localized.
    static let compactColumnWidth: CGFloat = 140

    var compact = false

    /// The pill text: the compact label in tables when one exists.
    static func label(for status: String, compact: Bool) -> String {
        let full = vocabulary[status.uppercased()]?.label ?? status
        return compact ? compactLabels[status.uppercased()] ?? full : full
    }

    var body: some View {
        let entry = Self.vocabulary[status.uppercased()] ?? (status, .neutral)
        let shown = Self.label(for: status, compact: compact)
        let badge = SymairaBadge(shown, tone: entry.tone, icon: entry.tone.symairaIcon)
        if shown == entry.label {
            badge
        } else {
            badge.help(entry.label).accessibilityLabel(entry.label)
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

// MARK: - Page scaffold glue

/// Header refresh control. `SymairaPage` has no built-in refresh, so every page puts this
/// as the last item of its fixed-height toolbar zone; Command-R triggers it.
struct PageRefreshButton: View {
    let title: String
    let refresh: () async -> Void

    var body: some View {
        Button {
            Task { await refresh() }
        } label: {
            SymairaIconView(.refresh)
        }
        .symairaButtonStyle(.secondary)
        .keyboardShortcut("r", modifiers: .command)
        .accessibilityLabel("Refresh \(title.lowercased())")
        .topoProtected()
    }
}

extension ViewState {
    /// Bridge to the shared page state model rendered by `SymairaPageState`.
    var pageState: SymairaPageLoadState<T> {
        switch self {
        case .idle: .idle
        case .loading: .loading
        case .loaded(let value): .loaded(value)
        case .failed(let message): .failed(message)
        }
    }
}
