import SwiftUI
import SymairaTheme

/// Requests view — paginated list with filters and event detail.
struct RequestsView: View {
    @StateObject private var vm = RequestsViewModel()
    @State private var showDetail = false

    var body: some View {
        HSplitView {
            // Main list
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    header
                    filters

                    switch vm.state {
                    case .idle, .loading:
                        LoadingOverlay(message: "Loading requests…")
                            .frame(height: 200)

                    case .failed(let message):
                        ErrorStateView(message: message) {
                            Task { await vm.refresh() }
                        }
                        .frame(height: 200)

                    case .loaded(let requests):
                        if requests.isEmpty {
                            EmptyStateView(
                                icon: "envelope.fill",
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
                detailPanel(request: request)
                    .frame(minWidth: 320, idealWidth: 400)
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
                Image(systemName: "arrow.clockwise")
            }
            .buttonStyle(.plain)
            .foregroundStyle(BrandColors.goldPrimary)
        }
    }

    private var filters: some View {
        HStack(spacing: 12) {
            TextField("Campaign", text: $vm.filterCampaignId)
                .textFieldStyle(.symaira)
                .frame(width: 150)
            TextField("Status", text: $vm.filterStatus)
                .textFieldStyle(.symaira)
                .frame(width: 180)
            TextField("Broker", text: $vm.filterBrokerId)
                .textFieldStyle(.symaira)
                .frame(width: 150)
            Button("Apply") {
                vm.page = 1
                Task { await vm.refresh() }
            }
            .buttonStyle(.bordered)
            .tint(BrandColors.goldPrimary)
            Button("Reset") { vm.resetFilters() }
                .buttonStyle(.bordered)
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

            Divider().background(BrandColors.textMuted.opacity(0.2))

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

                        Divider().background(BrandColors.textMuted.opacity(0.1))
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
                .disabled(!vm.hasPrevious)
            Button("Next") { vm.nextPage() }
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
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(BrandColors.textMuted)
                }
                .buttonStyle(.plain)
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

            Divider()
                .background(BrandColors.textMuted.opacity(0.2))

            Text("Events")
                .symairaText(.bodyEmphasized)
                .foregroundStyle(BrandColors.textPrimary)

            switch vm.eventsState {
            case .idle, .loading:
                ProgressView()
                    .tint(BrandColors.goldPrimary)

            case .failed(let message):
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
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
        .background(
            Rectangle()
                .fill(BrandColors.bgDarker.opacity(0.8))
                .overlay(
                    Rectangle()
                        .stroke(Color.white.opacity(0.06), lineWidth: 1)
                )
        )
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
        HStack {
            Text(label)
                .symairaText(.caption)
                .foregroundStyle(BrandColors.textMuted)
                .frame(width: 90, alignment: .trailing)
            Text(value)
                .symairaText(.callout)
                .foregroundStyle(BrandColors.textPrimary)
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
