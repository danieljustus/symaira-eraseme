import SwiftUI
import SymairaTheme

/// Requests view — paginated list with filters and event detail.
struct RequestsView: View {
    @StateObject private var vm = RequestsViewModel()
    @State private var showDetail = false

    var body: some View {
        // Not HSplitView: its NSSplitView ignores the toolbar safe area and clips the header.
        HStack(spacing: 0) {
            // Main list
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header
                    filters

                    switch vm.state {
                    case .idle, .loading:
                        SymairaLoadingState("Loading requests…")
                            .frame(height: 200)

                    case .failed(let message):
                        ErrorStateView(message: message) {
                            Task { await vm.refresh() }
                        }
                        .frame(height: 200)

                    case .loaded(let requests):
                        if requests.isEmpty {
                            SymairaEmptyState(
                                systemImage: "envelope.fill",
                                title: "No Requests",
                                message: "Create a campaign to generate removal requests."
                            )
                            .frame(height: 200)
                        } else {
                            requestsTable
                            pagination
                        }
                    }
                }
                .padding(24)
            }

            // Detail panel
            if showDetail, let request = vm.selectedRequest {
                Divider().overlay(SymairaTokens.line)
                detailPanel(request: request)
                    .frame(width: 360)
            }
        }
        .background(Color.clear)
        .task { await vm.refresh() }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Requests")
                    .symairaText(.display)
                    .foregroundStyle(BrandColors.textPrimary)
                Text("\(vm.total.plainDigits) total requests")
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.textMuted)
            }
            Spacer()
            Button {
                Task { await vm.refresh() }
            } label: {
                SymairaIconView(.refresh)
            }
            .symairaButtonStyle(.secondary)
            .accessibilityLabel("Refresh requests")
        }
    }

    private var filters: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            HStack(spacing: SymairaSpacing.medium) {
                TextField("Campaign", text: $vm.filterCampaignId)
                TextField("Status", text: $vm.filterStatus)
                TextField("Broker", text: $vm.filterBrokerId)
            }
            .textFieldStyle(.symaira)
            HStack(spacing: SymairaSpacing.medium) {
                Button("Apply") {
                    vm.page = 1
                    Task { await vm.refresh() }
                }
                .symairaButtonStyle(.secondary)
                Button("Reset") { vm.resetFilters() }
                    .symairaButtonStyle(.secondary)
            }
            .fixedSize()
        }
    }

    private var requestsTable: some View {
        VStack(spacing: 0) {
            // Header row
            HStack {
                Text("ID").frame(width: 50, alignment: .leading)
                Text("Broker").frame(width: 100, alignment: .leading)
                Text("Channel").frame(width: 80, alignment: .leading)
                Text("Status").frame(width: 120, alignment: .leading)
                Text("Jurisdiction").frame(width: 90, alignment: .leading)
                Text("Deadline").frame(width: 80, alignment: .leading)
            }
            .symairaText(.sectionLabel)
            .foregroundStyle(BrandColors.textMuted)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)

            SymairaFormDivider()

            // Data rows
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(vm.requests) { request in
                        HStack {
                            Text("\(request.id.plainDigits)")
                                .frame(width: 50, alignment: .leading)
                            Text(request.brokerId)
                                .foregroundStyle(BrandColors.goldPrimary)
                                .lineLimit(1)
                                .frame(width: 100, alignment: .leading)
                            Text(request.channel)
                                .frame(width: 80, alignment: .leading)
                            StatusBadge(status: request.statusDisplay)
                                .frame(width: 120, alignment: .leading)
                            Text(request.jurisdiction)
                                .frame(width: 90, alignment: .leading)
                            if let date = request.deadlineDate {
                                Text(date.formatted(.dateTime.month().day()))
                                    .frame(width: 80, alignment: .leading)
                            } else {
                                Text("—")
                                    .foregroundStyle(BrandColors.textMuted)
                                    .frame(width: 80, alignment: .leading)
                            }
                        }
                        .symairaText(.callout)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 6)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            Task {
                                await vm.loadEvents(for: request)
                                showDetail = true
                            }
                        }

                        SymairaFormDivider()
                    }
                }
            }
        }
        .frame(minHeight: 200)
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
                    showDetail = false
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
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(vm.requestEvents) { event in
                                HStack(alignment: .top, spacing: 8) {
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
        .padding(16)
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
