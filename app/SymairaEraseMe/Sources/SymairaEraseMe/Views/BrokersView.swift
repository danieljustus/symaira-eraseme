import SwiftUI

/// Brokers view — browse and filter the broker registry.
struct BrokersView: View {
    @StateObject private var vm = BrokersViewModel()
    @State private var showDetail = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header

                if let error = vm.errorMessage {
                    ErrorBanner(message: error) { vm.errorMessage = nil }
                }

                filters

                if vm.isLoading && vm.brokers.isEmpty {
                    LoadingOverlay(message: "Loading brokers…")
                        .frame(height: 200)
                } else if vm.filteredBrokers.isEmpty {
                    EmptyStateView(
                        icon: "person.2.fill",
                        title: "No Brokers Found",
                        message: "Adjust filters or check the broker registry."
                    )
                    .frame(height: 200)
                } else {
                    brokerGrid
                }
            }
            .padding(24)
        }
        .background(Color.clear)
        .task { await vm.refresh() }
        .sheet(item: $vm.selectedBroker) { broker in
            brokerDetailSheet(broker)
        }
    }

    private var header: some View {
        HStack {
            VStack(alignment: .leading) {
                Text("Brokers")
                    .font(.largeTitle.bold())
                    .foregroundStyle(BrandColors.textPrimary)
                Text("\(vm.total) brokers in registry")
                    .font(.caption)
                    .foregroundStyle(BrandColors.textMuted)
            }
            Spacer()
            TextField("Search…", text: $vm.searchText)
                .textFieldStyle(.roundedBorder)
                .frame(width: 200)
            Button {
                Task { await vm.refresh() }
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.title3)
            }
            .buttonStyle(.plain)
            .foregroundStyle(BrandColors.goldPrimary)
        }
    }

    private var filters: some View {
        HStack(spacing: 12) {
            Picker("Jurisdiction", selection: $vm.filterJurisdiction) {
                Text("All").tag("")
                Text("GDPR").tag("GDPR")
                Text("CCPA").tag("CCPA")
                Text("LGPD").tag("LGPD")
                Text("PIPEDA").tag("PIPEDA")
            }
            .frame(width: 130)

            Picker("Priority", selection: $vm.filterPriority) {
                Text("All").tag("")
                Text("High").tag("high")
                Text("Medium").tag("medium")
                Text("Low").tag("low")
            }
            .frame(width: 110)

            Picker("Category", selection: $vm.filterCategory) {
                Text("All").tag("")
                Text("People Search").tag("people-search")
                Text("Marketing").tag("marketing")
                Text("Credit").tag("credit")
                Text("Analytics").tag("analytics")
                Text("Background Check").tag("background-check")
                Text("Social Media").tag("social-media")
            }
            .frame(width: 160)

            Button("Apply") {
                Task { await vm.refresh() }
            }
            .buttonStyle(.bordered)
            .tint(BrandColors.goldPrimary)
            Button("Reset") { vm.resetFilters() }
                .buttonStyle(.bordered)
        }
    }

    private var brokerGrid: some View {
        let columns = [
            GridItem(.adaptive(minimum: 220, maximum: 320), spacing: 12)
        ]

        return LazyVGrid(columns: columns, spacing: 12) {
            ForEach(vm.filteredBrokers) { broker in
                BrokerCard(broker: broker)
                    .onTapGesture {
                        vm.selectedBroker = broker
                        showDetail = true
                    }
            }
        }
    }

    @ViewBuilder
    private func brokerDetailSheet(_ broker: Broker) -> some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    // Header
                    VStack(alignment: .leading, spacing: 4) {
                        Text(broker.name)
                            .font(.title2.bold())
                            .foregroundStyle(BrandColors.goldPrimary)
                        if let website = broker.website {
                            Link(website, destination: URL(string: website) ?? URL(string: "about:blank")!)
                                .font(.caption)
                        }
                    }

                    // Info grid
                    Group {
                        detailRow("ID", broker.brokerId)
                        detailRow("Category", broker.category ?? "—")
                        detailRow("Priority", broker.priority ?? "—")
                        detailRow("Jurisdictions", broker.jurisdictions?.joined(separator: ", ") ?? "—")
                        detailRow("Laws", broker.laws?.joined(separator: ", ") ?? "—")
                        if let sensitivity = broker.dataSensitivity {
                            detailRow("Data Sensitivity", "\(sensitivity)/5")
                        }
                        if let disabled = broker.disabled, disabled {
                            detailRow("Status", "DISABLED")
                        }
                    }

                    Divider().background(BrandColors.textMuted.opacity(0.2))

                    // Opt-out channels
                    if let optOut = broker.optOut, !optOut.isEmpty {
                        Text("Opt-Out Channels")
                            .font(.headline)
                            .foregroundStyle(BrandColors.textPrimary)
                        ForEach(optOut.indices, id: \.self) { index in
                            let channel = optOut[index]
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    StatusBadge(status: channel.type)
                                    if let endpoint = channel.endpoint {
                                        Text(endpoint)
                                            .font(.caption)
                                    }
                                    if let url = channel.url {
                                        Link(url, destination: URL(string: url) ?? URL(string: "about:blank")!)
                                            .font(.caption)
                                    }
                                }
                                if let fields = channel.requiredFields, !fields.isEmpty {
                                    Text("Required fields: \(fields.joined(separator: ", "))")
                                        .font(.caption2)
                                        .foregroundStyle(BrandColors.textMuted)
                                }
                            }
                        }
                    }

                    if let notes = broker.notes, !notes.isEmpty {
                        Divider().background(BrandColors.textMuted.opacity(0.2))
                        Text("Notes")
                            .font(.headline)
                            .foregroundStyle(BrandColors.textPrimary)
                        Text(notes)
                            .font(.subheadline)
                            .foregroundStyle(BrandColors.textSecondary)
                    }
                }
                .padding(20)
            }
            .background(BrandColors.bgDark)
            .navigationTitle("Broker Details")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        vm.selectedBroker = nil
                        showDetail = false
                    }
                }
            }
        }
        .frame(width: 500, height: 600)
    }

    private func detailRow(_ label: String, _ value: String) -> some View {
        HStack(alignment: .top) {
            Text(label)
                .font(.caption)
                .foregroundStyle(BrandColors.textMuted)
                .frame(width: 110, alignment: .trailing)
            Text(value)
                .font(.subheadline)
                .foregroundStyle(BrandColors.textPrimary)
        }
    }
}

struct BrokerCard: View {
    let broker: Broker
    @State private var isHovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(broker.name)
                    .font(.subheadline.bold())
                    .foregroundStyle(BrandColors.goldPrimary)
                    .lineLimit(1)
                Spacer()
                if let priority = broker.priority {
                    Text(priority.uppercased())
                        .font(.caption2.bold())
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(
                            Capsule().fill(priority == "high" ? BrandColors.rejectedBg : BrandColors.pendingBg)
                        )
                        .foregroundStyle(priority == "high" ? BrandColors.rejected : BrandColors.pending)
                }
            }

            if let category = broker.category {
                Text(category.replacingOccurrences(of: "-", with: " ").capitalized)
                    .font(.caption)
                    .foregroundStyle(BrandColors.textSecondary)
            }

            if let jurisdictions = broker.jurisdictions, !jurisdictions.isEmpty {
                HStack(spacing: 4) {
                    ForEach(jurisdictions, id: \.self) { jur in
                        Text(jur)
                            .font(.caption2)
                            .padding(.horizontal, 4)
                            .padding(.vertical, 1)
                            .background(Capsule().fill(BrandColors.plannedBg))
                            .foregroundStyle(BrandColors.planned)
                    }
                }
            }

            if let channels = broker.optOut {
                HStack(spacing: 8) {
                    ForEach(channels, id: \.type) { ch in
                        Label(ch.type, systemImage: ch.type == "email" ? "envelope" : "globe")
                            .font(.caption2)
                            .foregroundStyle(BrandColors.textMuted)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(isHovered ? BrandColors.bgCardHover : BrandColors.bgCard)
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(isHovered ? BrandColors.goldPrimary.opacity(0.2) : Color.white.opacity(0.05), lineWidth: 1)
                )
        )
        .scaleEffect(isHovered ? 1.01 : 1.0)
        .onHover { hovering in
            withAnimation(.spring(response: 0.2, dampingFraction: 0.8)) {
                isHovered = hovering
            }
        }
        .contentShape(Rectangle())
    }
}
