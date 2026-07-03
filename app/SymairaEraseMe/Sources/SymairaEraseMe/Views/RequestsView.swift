import SwiftUI

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
                    if let error = vm.errorMessage {
                        ErrorBanner(message: error) { vm.errorMessage = nil }
                    }
                    if vm.isLoading && vm.requests.isEmpty {
                        LoadingOverlay(message: "Loading requests…")
                            .frame(height: 200)
                    } else if vm.requests.isEmpty {
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
                    .font(.largeTitle.bold())
                    .foregroundStyle(BrandColors.textPrimary)
                Text("\(vm.total) total requests")
                    .font(.caption)
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
                .textFieldStyle(.roundedBorder)
                .frame(width: 150)
            TextField("Status", text: $vm.filterStatus)
                .textFieldStyle(.roundedBorder)
                .frame(width: 180)
            TextField("Broker", text: $vm.filterBrokerId)
                .textFieldStyle(.roundedBorder)
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
                Text("Broker").frame(minWidth: 100, alignment: .leading)
                Text("Channel").frame(width: 80, alignment: .leading)
                Text("Status").frame(minWidth: 120, alignment: .leading)
                Text("Jurisdiction").frame(width: 90, alignment: .leading)
                Text("Deadline").frame(width: 80, alignment: .leading)
            }
            .font(.caption.bold())
            .foregroundStyle(BrandColors.textMuted)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)

            Divider().background(BrandColors.textMuted.opacity(0.2))

            // Data rows
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(vm.requests) { request in
                        HStack {
                            Text("\(request.id)")
                                .frame(width: 50, alignment: .leading)
                            Text(request.brokerId)
                                .foregroundStyle(BrandColors.goldPrimary)
                                .frame(minWidth: 100, alignment: .leading)
                            Text(request.channel)
                                .frame(width: 80, alignment: .leading)
                            StatusBadge(status: request.statusDisplay)
                                .frame(minWidth: 120, alignment: .leading)
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
                        .font(.subheadline)
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
            Text("Page \(vm.page) of \(vm.totalPages)")
                .font(.caption)
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
                Text("Request #\(request.id)")
                    .font(.headline)
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
                .font(.subheadline.bold())
                .foregroundStyle(BrandColors.textPrimary)

            if vm.isLoadingEvents {
                ProgressView()
                    .tint(BrandColors.goldPrimary)
            } else if vm.requestEvents.isEmpty {
                Text("No events")
                    .font(.caption)
                    .foregroundStyle(BrandColors.textMuted)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(vm.requestEvents) { event in
                            HStack(alignment: .top, spacing: 8) {
                                Circle()
                                    .fill(BrandColors.color(for: event.eventType))
                                    .frame(width: 8, height: 8)
                                    .offset(y: 4)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(event.eventType.replacingOccurrences(of: "_", with: " ").capitalized)
                                        .font(.caption)
                                        .foregroundStyle(BrandColors.textPrimary)
                                    Text(event.occurredAt.formattedDate)
                                        .font(.caption2)
                                        .foregroundStyle(BrandColors.textMuted)
                                    Text("Source: \(event.source)")
                                        .font(.caption2)
                                        .foregroundStyle(BrandColors.textMuted)
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

    private func detailRow(_ label: String, _ value: String) -> some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundStyle(BrandColors.textMuted)
                .frame(width: 90, alignment: .trailing)
            Text(value)
                .font(.subheadline)
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
