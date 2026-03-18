import AppKit
import SwiftUI

// MARK: - Dictionary View

struct DictionaryView: View {
    @ObservedObject var vm: DictionaryViewModel
    @State private var selectedTab = 0

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $selectedTab) {
                Text("Vocabulary").tag(0)
                Text("Snippets").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(Theme.Spacing.medium)

            if selectedTab == 0 {
                VocabularyTab(vm: vm)
            } else {
                SnippetsTab(vm: vm)
            }
        }
    }
}

// MARK: - Vocabulary Tab

private struct VocabularyTab: View {
    @ObservedObject var vm: DictionaryViewModel
    @State private var showAddSheet = false

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar row
            HStack {
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("Search vocabulary…", text: $vm.searchQuery)
                        .textFieldStyle(.plain)
                        .onChange(of: vm.searchQuery) { _ in vm.refresh() }
                }
                .padding(6)
                .background(Color(nsColor: .controlBackgroundColor))
                .cornerRadius(6)

                Button {
                    showAddSheet = true
                } label: {
                    Label("Add Word", systemImage: "plus")
                }
            }
            .padding(.horizontal, Theme.Spacing.medium)
            .padding(.bottom, Theme.Spacing.small)

            if vm.vocabularyEntries.isEmpty && vm.searchQuery.isEmpty {
                VStack(spacing: Theme.Spacing.medium) {
                    Image(systemName: "character.book.closed")
                        .font(.system(size: 36))
                        .foregroundStyle(.tertiary)
                    Text("No vocabulary words yet")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                    Button("Add Word") { showAddSheet = true }
                        .controlSize(.large)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if vm.vocabularyEntries.isEmpty {
                VStack(spacing: Theme.Spacing.small) {
                    Text("No results for \"\(vm.searchQuery)\"")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(vm.vocabularyEntries) { entry in
                        VocabularyRow(entry: entry)
                            .contextMenu {
                                Button("Edit") { vm.editingEntry = entry }
                                Divider()
                                Button("Delete", role: .destructive) { vm.deleteEntry(entry) }
                            }
                    }
                }
                .listStyle(.inset(alternatesRowBackgrounds: true))
            }
        }
        .sheet(isPresented: $showAddSheet) {
            AddVocabularySheet(vm: vm, isPresented: $showAddSheet)
        }
        .sheet(item: $vm.editingEntry) { entry in
            EditEntrySheet(vm: vm, entry: entry)
        }
    }
}

// MARK: - Snippets Tab

private struct SnippetsTab: View {
    @ObservedObject var vm: DictionaryViewModel
    @State private var showAddSheet = false

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar row
            HStack {
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("Search snippets…", text: $vm.searchQuery)
                        .textFieldStyle(.plain)
                        .onChange(of: vm.searchQuery) { _ in vm.refresh() }
                }
                .padding(6)
                .background(Color(nsColor: .controlBackgroundColor))
                .cornerRadius(6)

                Button {
                    vm.importCSV()
                } label: {
                    Label("Import CSV", systemImage: "square.and.arrow.down")
                }

                Button {
                    showAddSheet = true
                } label: {
                    Label("Add Snippet", systemImage: "plus")
                }
            }
            .padding(.horizontal, Theme.Spacing.medium)
            .padding(.bottom, Theme.Spacing.small)

            if vm.snippetEntries.isEmpty && vm.searchQuery.isEmpty {
                VStack(spacing: Theme.Spacing.medium) {
                    Image(systemName: "text.snippet")
                        .font(.system(size: 36))
                        .foregroundStyle(.tertiary)
                    Text("No snippets yet")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                    Button("Add Snippet") { showAddSheet = true }
                        .controlSize(.large)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if vm.snippetEntries.isEmpty {
                VStack(spacing: Theme.Spacing.small) {
                    Text("No results for \"\(vm.searchQuery)\"")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List {
                    ForEach(vm.snippetEntries) { entry in
                        SnippetRow(entry: entry)
                            .contextMenu {
                                Button("Edit") { vm.editingEntry = entry }
                                Divider()
                                Button("Delete", role: .destructive) { vm.deleteEntry(entry) }
                            }
                    }
                }
                .listStyle(.inset(alternatesRowBackgrounds: true))
            }
        }
        .sheet(isPresented: $showAddSheet) {
            AddSnippetSheet(vm: vm, isPresented: $showAddSheet)
        }
        .sheet(item: $vm.editingEntry) { entry in
            EditEntrySheet(vm: vm, entry: entry)
        }
    }
}

// MARK: - Rows

private struct VocabularyRow: View {
    let entry: DictionaryEntry

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .short
        return f
    }()

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.phrase)
                    .font(.body.weight(.medium))
                if let replacement = entry.replacement {
                    Text(replacement)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }

            Spacer()

            if let source = entry.source {
                Text(source)
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.accentColor.opacity(0.1))
                    .cornerRadius(4)
            }

            if entry.frequencyUsed > 0 {
                Text("\(entry.frequencyUsed)x")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text(Self.dateFormatter.string(from: entry.modifiedAt))
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 2)
    }
}

private struct SnippetRow: View {
    let entry: DictionaryEntry

    var body: some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.phrase)
                    .font(.body.weight(.medium))
                    .foregroundColor(.accentColor)
                if let replacement = entry.replacement {
                    Text(replacement)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Spacer()
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Add Sheets

private struct AddVocabularySheet: View {
    @ObservedObject var vm: DictionaryViewModel
    @Binding var isPresented: Bool
    @State private var phrase = ""
    @State private var replacement = ""

    var body: some View {
        VStack(spacing: Theme.Spacing.large) {
            Text("Add Vocabulary Word")
                .font(.title3.weight(.semibold))

            TextField("Word or phrase (max 60 chars)", text: $phrase)
                .textFieldStyle(.roundedBorder)
                .onChange(of: phrase) { newValue in
                    if newValue.count > 60 { phrase = String(newValue.prefix(60)) }
                }

            TextField("Replacement (optional)", text: $replacement)
                .textFieldStyle(.roundedBorder)
                .onChange(of: replacement) { newValue in
                    if newValue.count > 200 { replacement = String(newValue.prefix(200)) }
                }

            HStack {
                Button("Cancel") { isPresented = false }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Add") {
                    let rep = replacement.trimmingCharacters(in: .whitespaces)
                    vm.addVocabularyWord(phrase: phrase, replacement: rep.isEmpty ? nil : rep)
                    isPresented = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(phrase.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Theme.Spacing.xlarge)
        .frame(width: 380)
    }
}

private struct AddSnippetSheet: View {
    @ObservedObject var vm: DictionaryViewModel
    @Binding var isPresented: Bool
    @State private var phrase = ""
    @State private var replacement = ""

    var body: some View {
        VStack(spacing: Theme.Spacing.large) {
            Text("Add Snippet")
                .font(.title3.weight(.semibold))

            TextField("Abbreviation (max 60 chars)", text: $phrase)
                .textFieldStyle(.roundedBorder)
                .onChange(of: phrase) { newValue in
                    if newValue.count > 60 { phrase = String(newValue.prefix(60)) }
                }

            VStack(alignment: .leading, spacing: 4) {
                Text("Expansion")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                TextEditor(text: $replacement)
                    .font(.body)
                    .frame(height: 100)
                    .border(Color(nsColor: .separatorColor), width: 1)
                    .onChange(of: replacement) { newValue in
                        if newValue.count > 4000 { replacement = String(newValue.prefix(4000)) }
                    }
            }

            HStack {
                Button("Cancel") { isPresented = false }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Add") {
                    vm.addSnippet(phrase: phrase, replacement: replacement)
                    isPresented = false
                }
                .keyboardShortcut(.defaultAction)
                .disabled(phrase.trimmingCharacters(in: .whitespaces).isEmpty || replacement.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Theme.Spacing.xlarge)
        .frame(width: 420)
    }
}

// MARK: - Edit Sheet

private struct EditEntrySheet: View {
    @ObservedObject var vm: DictionaryViewModel
    let entry: DictionaryEntry
    @State private var phrase: String
    @State private var replacement: String
    @Environment(\.dismiss) private var dismiss

    init(vm: DictionaryViewModel, entry: DictionaryEntry) {
        self.vm = vm
        self.entry = entry
        _phrase = State(initialValue: entry.phrase)
        _replacement = State(initialValue: entry.replacement ?? "")
    }

    var body: some View {
        VStack(spacing: Theme.Spacing.large) {
            Text(entry.isSnippet ? "Edit Snippet" : "Edit Vocabulary Word")
                .font(.title3.weight(.semibold))

            TextField(entry.isSnippet ? "Abbreviation" : "Word or phrase", text: $phrase)
                .textFieldStyle(.roundedBorder)
                .onChange(of: phrase) { newValue in
                    if newValue.count > 60 { phrase = String(newValue.prefix(60)) }
                }

            if entry.isSnippet {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Expansion")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    TextEditor(text: $replacement)
                        .font(.body)
                        .frame(height: 100)
                        .border(Color(nsColor: .separatorColor), width: 1)
                        .onChange(of: replacement) { newValue in
                            if newValue.count > 4000 { replacement = String(newValue.prefix(4000)) }
                        }
                }
            } else {
                TextField("Replacement (optional)", text: $replacement)
                    .textFieldStyle(.roundedBorder)
                    .onChange(of: replacement) { newValue in
                        if newValue.count > 200 { replacement = String(newValue.prefix(200)) }
                    }
            }

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Save") {
                    let rep = replacement.trimmingCharacters(in: .whitespaces)
                    vm.updateEntry(id: entry.id, phrase: phrase, replacement: rep.isEmpty ? nil : rep)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(phrase.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(Theme.Spacing.xlarge)
        .frame(width: entry.isSnippet ? 420 : 380)
    }
}

// MARK: - View Model

class DictionaryViewModel: ObservableObject {
    private let dictionaryStore: DictionaryStore

    @Published var vocabularyEntries: [DictionaryEntry] = []
    @Published var snippetEntries: [DictionaryEntry] = []
    @Published var searchQuery = ""
    @Published var editingEntry: DictionaryEntry?

    init(dictionaryStore: DictionaryStore) {
        self.dictionaryStore = dictionaryStore
        refresh()
    }

    func refresh() {
        if searchQuery.isEmpty {
            vocabularyEntries = dictionaryStore.getAllVocabulary()
            snippetEntries = dictionaryStore.getAllSnippets()
        } else {
            vocabularyEntries = dictionaryStore.searchEntries(query: searchQuery, snippet: false)
            snippetEntries = dictionaryStore.searchEntries(query: searchQuery, snippet: true)
        }
    }

    func addVocabularyWord(phrase: String, replacement: String? = nil) {
        let trimmed = phrase.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        dictionaryStore.addEntry(phrase: trimmed, replacement: replacement, isSnippet: false)
        refresh()
    }

    func addSnippet(phrase: String, replacement: String) {
        let trimmedPhrase = phrase.trimmingCharacters(in: .whitespaces)
        let trimmedReplacement = replacement.trimmingCharacters(in: .whitespaces)
        guard !trimmedPhrase.isEmpty, !trimmedReplacement.isEmpty else { return }
        dictionaryStore.addEntry(phrase: trimmedPhrase, replacement: trimmedReplacement, isSnippet: true)
        refresh()
    }

    func updateEntry(id: String, phrase: String, replacement: String?) {
        let trimmed = phrase.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        dictionaryStore.updateEntry(id: id, phrase: trimmed, replacement: replacement)
        refresh()
    }

    func deleteEntry(_ entry: DictionaryEntry) {
        dictionaryStore.softDelete(id: entry.id)
        refresh()
    }

    func importCSV() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.commaSeparatedText]
        panel.allowsMultipleSelection = false

        guard panel.runModal() == .OK, let url = panel.url else { return }

        let result = dictionaryStore.importCSV(url: url)
        refresh()

        let alert = NSAlert()
        alert.messageText = "Import Complete"
        if result.errors.isEmpty {
            alert.informativeText = "Imported \(result.imported) entries."
        } else {
            alert.informativeText = "Imported \(result.imported) entries with \(result.errors.count) errors:\n\(result.errors.prefix(5).joined(separator: "\n"))"
        }
        alert.runModal()
    }
}
