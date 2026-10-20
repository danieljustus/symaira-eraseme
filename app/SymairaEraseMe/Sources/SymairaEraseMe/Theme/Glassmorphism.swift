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
        "AWAITING_ACK": ("Awaiting ACK", .warning),
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

    var body: some View {
        let entry = Self.vocabulary[status.uppercased()] ?? (status, .neutral)
        SymairaBadge(entry.label, tone: entry.tone, icon: entry.tone.symairaIcon)
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

// MARK: - Page scaffold

/// One frame for every sidebar page: same header, content width, spacing and state heights.
enum PageLayout {
    static let maxWidth: CGFloat = 1008
    static let stateHeight: CGFloat = 240
}

struct Page<Accessory: View, Content: View>: View {
    let title: String
    /// Required: a one-line header on some pages and two on others misaligns the controls.
    let subtitle: String
    /// Lists that scroll themselves (native `Table`) fill the page instead of nesting scroll views.
    var scrolls = true
    let refresh: () async -> Void
    @ViewBuilder var accessory: Accessory
    @ViewBuilder var content: Content

    var body: some View {
        let column = VStack(alignment: .leading, spacing: SymairaSpacing.large) {
            header
            content
        }
        .padding(SymairaSpacing.xLarge)
        .frame(maxWidth: PageLayout.maxWidth, maxHeight: scrolls ? nil : .infinity, alignment: .topLeading)
        .frame(maxWidth: .infinity)
        if scrolls {
            ScrollView { column }
        } else {
            column
        }
    }

    private var header: some View {
        HStack(alignment: .center, spacing: SymairaSpacing.medium) {
            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                Text(title)
                    .symairaText(.display)
                    .foregroundStyle(BrandColors.textPrimary)
                    .topoProtected()
                Text(subtitle)
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.textMuted)
                    .topoProtected()
            }
            Spacer(minLength: SymairaSpacing.large)
            accessory
            Button {
                Task { await refresh() }
            } label: {
                SymairaIconView(.refresh)
            }
            .symairaButtonStyle(.secondary)
            .accessibilityLabel("Refresh \(title.lowercased())")
            .topoProtected()
        }
    }
}

extension Page where Accessory == EmptyView {
    init(
        title: String, subtitle: String, scrolls: Bool = true, refresh: @escaping () async -> Void,
        @ViewBuilder content: () -> Content
    ) {
        self.init(
            title: title, subtitle: subtitle, scrolls: scrolls, refresh: refresh, accessory: { EmptyView() },
            content: content)
    }
}

/// Loading and failure look identical on every page; `loaded` decides empty vs content.
struct PageState<Value, Loaded: View>: View {
    let state: ViewState<Value>
    let loading: String
    let retry: () async -> Void
    @ViewBuilder let loaded: (Value) -> Loaded

    var body: some View {
        switch state {
        case .idle, .loading:
            SymairaLoadingState(loading).frame(height: PageLayout.stateHeight)
        case .failed(let message):
            ErrorStateView(message: message) { Task { await retry() } }
                .frame(height: PageLayout.stateHeight)
        case .loaded(let value):
            loaded(value)
        }
    }
}

/// Empty result with the shared state height.
struct PageEmpty: View {
    let icon: SymairaIcon
    let title: String
    let message: String

    var body: some View {
        SymairaEmptyState(icon: icon, title: title, message: message)
            .frame(height: PageLayout.stateHeight)
    }
}
