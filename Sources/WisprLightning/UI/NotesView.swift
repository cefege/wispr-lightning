import AppKit
import SwiftUI

// MARK: - Notes View

struct NotesView: View {
    @ObservedObject var vm: NotesViewModel

    var body: some View {
        VStack(spacing: 0) {
            // Toolbar row
            HStack {
                HStack {
                    Image(systemName: "magnifyingglass")
                        .foregroundStyle(.secondary)
                    TextField("Search notes…", text: $vm.searchQuery)
                        .textFieldStyle(.plain)
                        .onChange(of: vm.searchQuery) { _ in vm.refresh() }
                }
                .padding(6)
                .background(Color(nsColor: .controlBackgroundColor))
                .cornerRadius(6)

                Button {
                    vm.createNote()
                } label: {
                    Label("New Note", systemImage: "plus")
                }
            }
            .padding(.horizontal, Theme.Spacing.medium)
            .padding(.vertical, Theme.Spacing.small)

            if vm.notes.isEmpty && vm.searchQuery.isEmpty {
                VStack(spacing: Theme.Spacing.medium) {
                    Image(systemName: "note.text")
                        .font(.system(size: 36))
                        .foregroundStyle(.tertiary)
                    Text("No notes yet")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                    Button("Create Note") { vm.createNote() }
                        .controlSize(.large)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if vm.notes.isEmpty {
                VStack(spacing: Theme.Spacing.small) {
                    Text("No results for \"\(vm.searchQuery)\"")
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(selection: $vm.selectedNoteId) {
                    ForEach(vm.notes) { note in
                        NoteRow(note: note)
                            .tag(note.id)
                            .contextMenu {
                                Button("Edit") {
                                    vm.editingNote = note
                                }
                                Divider()
                                Button("Delete", role: .destructive) { vm.deleteNote(note) }
                            }
                    }
                }
                .listStyle(.inset(alternatesRowBackgrounds: true))
            }
        }
        .sheet(item: $vm.editingNote) { note in
            NoteEditorSheet(vm: vm, note: note)
        }
        .onChange(of: vm.selectedNoteId) { newValue in
            if let id = newValue, let note = vm.notes.first(where: { $0.id == id }) {
                vm.editingNote = note
                vm.selectedNoteId = nil
            }
        }
    }
}

// MARK: - Note Row

private struct NoteRow: View {
    let note: NoteEntry

    private static let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateStyle = .short
        f.timeStyle = .short
        return f
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: Theme.Spacing.small) {
            HStack {
                Text(note.title.isEmpty ? "Untitled" : note.title)
                    .font(.body.weight(.medium))
                Spacer()
                Text(Self.dateFormatter.string(from: note.modifiedAt))
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            if !note.contentPreview.isEmpty {
                Text(note.contentPreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Note Editor Sheet

private struct NoteEditorSheet: View {
    @ObservedObject var vm: NotesViewModel
    let note: NoteEntry
    @State private var title: String
    @State private var content: String
    @Environment(\.dismiss) private var dismiss

    init(vm: NotesViewModel, note: NoteEntry) {
        self.vm = vm
        self.note = note
        _title = State(initialValue: note.title)
        _content = State(initialValue: note.content)
    }

    var body: some View {
        VStack(spacing: Theme.Spacing.medium) {
            TextField("Title", text: $title)
                .textFieldStyle(.roundedBorder)
                .font(.title3)

            TextEditor(text: $content)
                .font(.body)
                .frame(minHeight: 200)
                .border(Color(nsColor: .separatorColor), width: 1)

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Save") {
                    vm.saveNote(id: note.id, title: title, content: content)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(Theme.Spacing.xlarge)
        .frame(width: 500, height: 400)
    }
}

// MARK: - View Model

class NotesViewModel: ObservableObject {
    private let notesStore: NotesStore

    @Published var notes: [NoteEntry] = []
    @Published var searchQuery = ""
    @Published var selectedNoteId: String?
    @Published var editingNote: NoteEntry?

    init(notesStore: NotesStore) {
        self.notesStore = notesStore
        refresh()
    }

    func refresh() {
        if searchQuery.isEmpty {
            notes = notesStore.getNotes()
        } else {
            notes = notesStore.search(query: searchQuery)
        }
    }

    func createNote() {
        let id = notesStore.addNote()
        refresh()
        if let note = notes.first(where: { $0.id == id }) {
            editingNote = note
        }
    }

    func saveNote(id: String, title: String, content: String) {
        notesStore.updateNote(id: id, title: title, content: content)
        refresh()
    }

    func deleteNote(_ note: NoteEntry) {
        notesStore.softDelete(id: note.id)
        refresh()
    }
}

// Make NoteEntry work with .sheet(item:)
extension NoteEntry: Hashable {
    static func == (lhs: NoteEntry, rhs: NoteEntry) -> Bool { lhs.id == rhs.id }
    func hash(into hasher: inout Hasher) { hasher.combine(id) }
}
