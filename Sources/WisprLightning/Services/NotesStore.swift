import Foundation
import SQLite3

class NotesStore {
    private let dbManager: DatabaseManager

    init(dbManager: DatabaseManager) {
        self.dbManager = dbManager
        createTable()
    }

    private func createTable() {
        let sql = """
            CREATE TABLE IF NOT EXISTS notes (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                content_preview TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at REAL NOT NULL,
                modified_at REAL NOT NULL,
                is_deleted INTEGER DEFAULT 0,
                finalized INTEGER DEFAULT 0
            );
            """
        dbManager.exec(sql)
    }

    func addNote(title: String = "Untitled", content: String = "") -> String {
        let id = UUID().uuidString
        let sql = """
            INSERT INTO notes (id, title, content_preview, content, created_at, modified_at)
            VALUES (?, ?, ?, ?, ?, ?);
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return id }
        defer { sqlite3_finalize(stmt) }

        let now = Date().timeIntervalSince1970
        let preview = String(content.prefix(200))
        sqlite3_bind_text(stmt, 1, (id as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (title as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 3, (preview as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 4, (content as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 5, now)
        sqlite3_bind_double(stmt, 6, now)

        sqlite3_step(stmt)
        return id
    }

    func updateNote(id: String, title: String, content: String) {
        let sql = "UPDATE notes SET title = ?, content_preview = ?, content = ?, modified_at = ? WHERE id = ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        let preview = String(content.prefix(200))
        sqlite3_bind_text(stmt, 1, (title as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (preview as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 3, (content as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 4, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 5, (id as NSString).utf8String, -1, nil)

        sqlite3_step(stmt)
    }

    func softDelete(id: String) {
        let sql = "UPDATE notes SET is_deleted = 1, modified_at = ? WHERE id = ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_double(stmt, 1, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 2, (id as NSString).utf8String, -1, nil)

        sqlite3_step(stmt)
    }

    func getNotes(limit: Int = 100) -> [NoteEntry] {
        let sql = "SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 ORDER BY modified_at DESC LIMIT ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int(stmt, 1, Int32(limit))

        var notes: [NoteEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            notes.append(noteFromRow(stmt))
        }
        return notes
    }

    func search(query: String) -> [NoteEntry] {
        let sql = "SELECT id, title, content_preview, content, created_at, modified_at FROM notes WHERE is_deleted = 0 AND (title LIKE ? OR content LIKE ?) ORDER BY modified_at DESC LIMIT 100;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        let pattern = "%\(query)%"
        sqlite3_bind_text(stmt, 1, (pattern as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (pattern as NSString).utf8String, -1, nil)

        var notes: [NoteEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            notes.append(noteFromRow(stmt))
        }
        return notes
    }

    private func noteFromRow(_ stmt: OpaquePointer?) -> NoteEntry {
        NoteEntry(
            id: dbManager.columnText(stmt, 0) ?? "",
            title: dbManager.columnText(stmt, 1) ?? "",
            contentPreview: dbManager.columnText(stmt, 2) ?? "",
            content: dbManager.columnText(stmt, 3) ?? "",
            createdAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 4)),
            modifiedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 5))
        )
    }
}
