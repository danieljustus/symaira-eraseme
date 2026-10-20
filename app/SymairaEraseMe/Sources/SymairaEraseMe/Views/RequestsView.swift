import SwiftUI
import SymairaTheme

/// Requests view — paginated list with filters and event detail.
struct RequestsView: View {
    @StateObject private var vm = RequestsViewModel()
    @State private var selection: RemovalRequest.ID?

    /// Backend status values (`list_requests` matches them exactly).
    private static let statusFilters = [
        "PLANNED", "AWAITING_ACK", "AWAITING_RESPONSE", "AWAITING_USER_ACTION", "CONFIRMED",
        "REJECTED_FINAL", "OVERDUE", "SEND_FAILED", "ESCALATED",
    ]

    var body: some View {
        // Not HSplitView: its NSSplitView ignores the toolbar safe area and clips the header.
        HStack(spacing: 0) {
            Page(
                title: "Requests", subtitle: "\(vm.total.plainDigits) total requests", scrolls: false,
                refresh: vm.refresh
            ) {
                filters
                PageState(state: vm.state, loading: "Loading requests…", retry: vm.refresh) { requests in
                    if requests.isEmpty {
                        PageEmpty(
                            systemImage: "envelope.fill",
                            title: "No Requests",
                            message: "Create a campaign to generate removal requests."
                        )
                    } else {
                        requestsTable
                        pagination
                    }
                }
            }

            if selection != nil, let request = vm.selectedRequest {
                Divider().overlay(SymairaTokens.line)
                detailPanel(request: request)
                    .frame(width: 360)
            }
        }
        // Filters apply on change; the delay keeps typing from firing a request per key.
        .task(id: "\(vm.filterCampaignId)|\(vm.filterStatus)|\(vm.filterBrokerId)") {
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            vm.page = 1
            await vm.refresh()
        }
        .onChange(of: selection) { _, id in
            guard let request = vm.requests.first(where: { $0.id == id }) else { return }
            Task { await vm.loadEvents(for: request) }
        }
    }

    private var filters: some View {
        HStack(spacing: SymairaSpacing.medium) {
            TextField("Campaign", text: $vm.filterCampaignId)
                .textFieldStyle(.symaira)
            Picker("Status", selection: $vm.filterStatus) {
                Text("All statuses").tag("")
                ForEach(Self.statusFilters, id: \.self) { status in
                    Text(StatusBadge.vocabulary[status]?.label ?? status).tag(status)
                }
            }
            .labelsHidden()
            .fixedSize()
            TextField("Broker", text: $vm.filterBrokerId)
                .textFieldStyle(.symaira)
        }
    }

    private var requestsTable: some View {
        Table(vm.requests, selection: $selection) {
            TableColumn("ID") { Text("\($0.id.plainDigits)") }
                .width(min: 40, ideal: 50)
            TableColumn("Broker") { Text($0.brokerId).foregroundStyle(BrandColors.goldPrimary) }
            TableColumn("Channel") { Text($0.channel) }
            TableColumn("Status") { StatusBadge(status: $0.statusDisplay) }
                .width(min: 110, ideal: 140)
            TableColumn("Jurisdiction") { Text($0.jurisdiction) }
            TableColumn("Deadline") { request in
                if let date = request.deadlineDate {
                    Text(date.formatted(.dateTime.month().day()))
                } else {
                    Text("—").foregroundStyle(BrandColors.textMuted)
                }
            }
        }
        .frame(maxHeight: .infinity)
        .symairaCard()
    }

    private var pagination: some View {
        HStack {
            Text("Page \(vm.page.plainDigits) of \(vm.totalPages.plainDigits)")
                .symairaText(.caption)
                .foregroundStyle(BrandColors.textMuted)
            Spacer()
            Button("Previous") { vm.previousPage() }
                .symairaButtonStyle(.secondary)
                .disabled(!vm.hasPrevious)
            Button("Next") { vm.nextPage() }
                .symairaButtonStyle(.secondary)
                .disabled(!vm.hasNext)
        }
    }

    @ViewBuilder
    private func detailPanel(request: RemovalRequest) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Request #\(request.id.plainDigits)")
                    .symairaText(.subheading)
                    .foregroundStyle(BrandColors.textPrimary)
                Spacer()
                Button {
                    selection = nil
                } label: {
                    SymairaIconView(.close)
                        .foregroundStyle(BrandColors.textMuted)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close request details")
            }

            Group {
                detailRow("Broker", request.brokerId)
                detailRow("Channel", request.channel)
                detailRow("Jurisdiction", request.jurisdiction)
                detailRow("Status", request.statusDisplay)
                if let deadline = request.deadlineAt {
                    detailRow("Deadline", deadline.formattedDate)
                }
                if let sent = request.sentAt {
                    detailRow("Sent", sent.formattedDate)
                }
                if let resolved = request.resolvedAt {
                    detailRow("Resolved", resolved.formattedDate)
                }
                if let reminders = request.remindersSent, reminders > 0 {
                    detailRow("Reminders", "\(reminders)")
                }
                if let level = request.escalationLevel, level > 0 {
                    detailRow("Escalation", level == 1 ? "Reminder" : "DPA Complaint")
                }
            }

            SymairaFormDivider()

            Text("Events")
                .symairaText(.bodyEmphasized)
                .foregroundStyle(BrandColors.textPrimary)

            switch vm.eventsState {
            case .idle, .loading:
                ProgressView()
                    .tint(BrandColors.goldPrimary)

            case .failed(let message):
                HStack {
                    SymairaIconView(.warning, size: 12)
                        .foregroundStyle(BrandColors.overdue)
                        .symairaText(.caption)
                    Text(message)
                        .symairaText(.caption)
                        .foregroundStyle(BrandColors.textMuted)
                    Button("Retry") {
                        Task { await vm.loadEvents(for: request) }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(BrandColors.goldPrimary)
                }

            case .loaded:
                if vm.requestEvents.isEmpty {
                    Text("No events")
                        .symairaText(.caption)
                        .foregroundStyle(BrandColors.textMuted)
                } else {
                    ScrollView {
                        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
                            ForEach(vm.requestEvents) { event in
                                HStack(alignment: .top, spacing: SymairaSpacing.small) {
                                    SymairaStatusDot(
                                        tone: eventTone(for: event.eventType),
                                        accessibilityLabel: event.eventType.replacingOccurrences(of: "_", with: " ").capitalized
                                    )
                                    .accessibilityHidden(true)
                                    .offset(y: 4)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(event.eventType.replacingOccurrences(of: "_", with: " ").capitalized)
                                            .symairaText(.caption)
                                            .foregroundStyle(BrandColors.textPrimary)
                                        Text(event.occurredAt.formattedDate)
                                            .symairaText(.caption)
                                            .foregroundStyle(BrandColors.textMuted)
                                        Text("Source: \(event.source)")
                                            .symairaText(.caption)
                                            .foregroundStyle(BrandColors.textMuted)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        .padding(SymairaSpacing.large)
        .background(SymairaTokens.surface)
    }

    private func eventTone(for eventType: String) -> SymairaTone {
        switch eventType.uppercased() {
        case "CONFIRMATION_RECEIVED": return .positive
        case "REJECTION_RECEIVED", "DEADLINE_REACHED": return .critical
        case "SENT", "REMINDER_SENT", "REBUTTAL_SENT": return .warning
        case "PLANNED": return .informative
        default: return .neutral
        }
    }

    private func detailRow(_ label: String, _ value: String) -> some View {
        SymairaFormRow(label) {
            Text(value)
                .symairaText(.callout)
                .textSelection(.enabled)
        }
    }
}

extension Color {
    /// Map an event type string to a display color.
    static func color(for eventType: String) -> Color {
        switch eventType.uppercased() {
        case "PLANNED": return BrandColors.planned
        case "SENT", "REMINDER_SENT", "REBUTTAL_SENT": return BrandColors.pending
        case "CONFIRMATION_RECEIVED": return BrandColors.confirmed
        case "REJECTION_RECEIVED": return BrandColors.rejected
        case "DEADLINE_REACHED": return BrandColors.overdue
        default: return BrandColors.textMuted
        }
    }
}
