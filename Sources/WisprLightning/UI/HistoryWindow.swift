import AppKit
import SwiftUI

// MARK: - SwiftUI History View

struct HistoryView: View {
    @ObservedObject var vm: HistoryViewModel

    var body: some View {
        NavigationStack {
            Group {
                if vm.groupedEntries.isEmpty {
                    VStack(spacing: Theme.Spacing.medium) {
                        Image(systemName: "text.badge.minus")
                            .font(.system(size: 36))
                            .foregroundStyle(.tertiary)
                        Text("No dictations yet")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    List {
                        ForEach(vm.groupedEntries) { group in
                            Section {
                                ForEach(group.entries) { entry in
                                    HistoryRow(entry: entry, vm: vm)
                                }
                            } header: {
                                Text(group.title)
                                    .font(.subheadline.weight(.semibold))
                                    .foregroundStyle(.secondary)
                            }
                        }

                        Section {
                            HStack {
                                Spacer()
                                Button("Clear All", role: .destructive) {
                                    vm.clearAll()
                                }
                                .controlSize(.small)
                            }
                        }
                    }
                    .listStyle(.inset(alternatesRowBackgrounds: true))
                }
            }
            .searchable(text: $vm.searchQuery, placement: .toolbar)
            .onChange(of: vm.searchQuery) { _ in vm.refresh() }
        }
    }
}

// MARK: - History Row

private struct HistoryRow: View {
    let entry: TranscriptEntry
    let vm: HistoryViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.small) {
            HStack(spacing: Theme.Spacing.medium) {
                Text(vm.formatTime(entry.timestamp))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text("·")
                    .foregroundStyle(.quaternary)
                Text(entry.appName)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text("·")
                    .foregroundStyle(.quaternary)
                Text("\(String(format: "%.1f", entry.duration))s")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Text("·")
                    .foregroundStyle(.quaternary)
                Text("\(entry.numWords) words")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Spacer()

                Button {
                    vm.copyEntry(entry)
                } label: {
                    Image(systemName: "doc.on.doc")
                }
                .buttonStyle(.borderless)
                .help("Copy")

                Button {
                    vm.deleteEntry(entry)
                } label: {
                    Image(systemName: "trash")
                }
                .buttonStyle(.borderless)
                .help("Delete")
            }

            Text(entry.formattedText ?? entry.asrText ?? "")
                .font(.body)
                .lineLimit(2)
                .foregroundStyle(.primary)
        }
        .padding(.vertical, Theme.Spacing.small)
    }
}

// MARK: - Grouped Entries

struct DateGroup: Identifiable {
    let id: String
    let title: String
    let entries: [TranscriptEntry]
}

// MARK: - View Model

class HistoryViewModel: ObservableObject {
    private let historyStore: HistoryStore

    @Published var entries: [TranscriptEntry] = []
    @Published var searchQuery = ""

    var groupedEntries: [DateGroup] {
        let calendar = Calendar.current
        let grouped = Dictionary(grouping: entries) { entry -> String in
            if calendar.isDateInToday(entry.timestamp) {
                return "Today"
            } else if calendar.isDateInYesterday(entry.timestamp) {
                return "Yesterday"
            } else {
                return Self.dateGroupFormatter.string(from: entry.timestamp)
            }
        }

        // Sort groups by the most recent entry in each group
        return grouped.map { key, values in
            DateGroup(id: key, title: key, entries: values.sorted { $0.timestamp > $1.timestamp })
        }
        .sorted { group1, group2 in
            guard let d1 = group1.entries.first?.timestamp,
                  let d2 = group2.entries.first?.timestamp else { return false }
            return d1 > d2
        }
    }

    init(historyStore: HistoryStore) {
        self.historyStore = historyStore
        refresh()
    }

    func refresh() {
        if searchQuery.isEmpty {
            entries = historyStore.getEntries()
        } else {
            entries = historyStore.search(query: searchQuery)
        }
    }

    func copyEntry(_ entry: TranscriptEntry) {
        let text = entry.formattedText ?? entry.asrText ?? ""
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    func deleteEntry(_ entry: TranscriptEntry) {
        let alert = NSAlert()
        alert.messageText = "Delete this entry?"
        alert.informativeText = "This action cannot be undone."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.alertStyle = .warning

        if alert.runModal() == .alertFirstButtonReturn {
            historyStore.deleteEntry(id: entry.id)
            refresh()
        }
    }

    func clearAll() {
        let alert = NSAlert()
        alert.messageText = "Clear all history?"
        alert.informativeText = "This will delete all transcript entries. This action cannot be undone."
        alert.addButton(withTitle: "Clear All")
        alert.addButton(withTitle: "Cancel")
        alert.alertStyle = .critical

        if alert.runModal() == .alertFirstButtonReturn {
            historyStore.clearAll()
            refresh()
        }
    }

    // MARK: - Date Formatting

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.timeStyle = .short
        return f
    }()

    private static let dateGroupFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "MMM d"
        return f
    }()

    func formatTime(_ date: Date) -> String {
        Self.timeFormatter.string(from: date)
    }
}

