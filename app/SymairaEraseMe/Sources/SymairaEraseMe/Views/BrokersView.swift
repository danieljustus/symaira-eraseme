import SwiftUI
import SymairaTheme

/// Brokers view — browse and filter the broker registry.
struct BrokersView: View {
    @StateObject private var vm = BrokersViewModel()

    var body: some View {
        Page(title: "Brokers", subtitle: "\(vm.total.plainDigits) brokers in registry", refresh: vm.refresh) {
            TextField("Search…", text: $vm.searchText)
                .textFieldStyle(.symaira)
                .frame(width: 200)
        } content: {
            filterFields
            PageState(state: vm.state, loading: "Loading brokers…", retry: vm.refresh) { _ in
                if vm.filteredBrokers.isEmpty {
                    PageEmpty(
                        systemImage: "person.2.fill",
                        title: "No Brokers Found",
                        message: "Adjust filters or check the broker registry."
                    )
                } else {
                    brokerGrid
                }
            }
        }
        // Filters apply on change, like every other page; no separate Apply step.
        .task(id: "\(vm.filterJurisdiction)|\(vm.filterPriority)|\(vm.filterCategory)") { await vm.refresh() }
        .sheet(item: $vm.selectedBroker) { broker in
            brokerDetailSheet(broker)
        }
    }

    private var filterFields: some View {
        HStack(spacing: SymairaSpacing.medium) {
            Picker("Jurisdiction", selection: $vm.filterJurisdiction) {
                Text("All jurisdictions").tag("")
                Text("GDPR").tag("GDPR")
                Text("CCPA").tag("CCPA")
                Text("LGPD").tag("LGPD")
                Text("PIPEDA").tag("PIPEDA")
            }
            .labelsHidden()
            .fixedSize()

            Picker("Priority", selection: $vm.filterPriority) {
                Text("All priorities").tag("")
                Text("High").tag("high")
                Text("Medium").tag("medium")
                Text("Low").tag("low")
            }
            .labelsHidden()
            .fixedSize()

            Picker("Category", selection: $vm.filterCategory) {
                Text("All categories").tag("")
                Text("People Search").tag("people-search")
                Text("Marketing").tag("marketing")
                Text("Credit").tag("credit")
                Text("Analytics").tag("analytics")
                Text("Background Check").tag("background-check")
                Text("Social Media").tag("social-media")
            }
            .labelsHidden()
            .fixedSize()
            Spacer(minLength: 0)
        }
    }

    private var brokerGrid: some View {
        let columns = [
            GridItem(.adaptive(minimum: 220, maximum: 320), spacing: SymairaSpacing.medium)
        ]

        return LazyVGrid(columns: columns, spacing: SymairaSpacing.medium) {
            ForEach(vm.filteredBrokers) { broker in
                Button {
                    vm.selectedBroker = broker
                } label: {
                    BrokerCard(broker: broker)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Details for \(broker.name)")
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
                            .symairaText(.heading)
                            .foregroundStyle(BrandColors.textPrimary)
                        if let website = broker.website {
                            Link(website, destination: URL(string: website) ?? URL(string: "about:blank")!)
                                .symairaText(.caption)
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

                    SymairaFormDivider()

                    // Opt-out channels
                    if let optOut = broker.optOut, !optOut.isEmpty {
                        Text("Opt-Out Channels")
                            .symairaText(.subheading)
                            .foregroundStyle(BrandColors.textPrimary)
                        ForEach(optOut.indices, id: \.self) { index in
                            let channel = optOut[index]
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    StatusBadge(status: channel.type)
                                    if let endpoint = channel.endpoint {
                                        Text(endpoint)
                                            .symairaText(.caption)
                                    }
                                    if let url = channel.url {
                                        Link(url, destination: URL(string: url) ?? URL(string: "about:blank")!)
                                            .symairaText(.caption)
                                    }
                                }
                                if let fields = channel.requiredFields, !fields.isEmpty {
                                    Text("Required fields: \(fields.joined(separator: ", "))")
                                        .symairaText(.caption)
                                        .foregroundStyle(BrandColors.textMuted)
                                }
                            }
                        }
                    }

                    if let notes = broker.notes, !notes.isEmpty {
                        SymairaFormDivider()
                        Text("Notes")
                            .symairaText(.subheading)
                            .foregroundStyle(BrandColors.textPrimary)
                        Text(notes)
                            .symairaText(.callout)
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
                    }
                    .keyboardShortcut(.cancelAction)
                }
            }
        }
        .frame(width: 500, height: 600)
        .onExitCommand {
            vm.selectedBroker = nil
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

struct BrokerCard: View {
    let broker: Broker

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(broker.name)
                    .symairaText(.bodyEmphasized)
                    .foregroundStyle(BrandColors.textPrimary)
                    .lineLimit(1)
                Spacer()
                if let priority = broker.priority {
                    SymairaBadge(priority.capitalized, tone: priority == "high" ? .critical : .warning)
                }
            }

            if let category = broker.category {
                Text(category.replacingOccurrences(of: "-", with: " ").capitalized)
                    .symairaText(.caption)
                    .foregroundStyle(BrandColors.textSecondary)
            }

            if let jurisdictions = broker.jurisdictions, !jurisdictions.isEmpty {
                HStack(spacing: 4) {
                    ForEach(jurisdictions, id: \.self) { jur in
                        SymairaBadge(jur)
                    }
                }
            }

            if let channels = broker.optOut {
                HStack(spacing: SymairaSpacing.small) {
                    ForEach(channels, id: \.type) { ch in
                        Label(ch.type, symairaIcon: ch.type == "email" ? .mail : .link, size: 12)
                            .symairaText(.caption)
                            .foregroundStyle(BrandColors.textMuted)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard(padding: SymairaSpacing.medium)
        .contentShape(Rectangle())
    }
}
