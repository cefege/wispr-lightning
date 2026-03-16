import Foundation
import SQLite3

class HistoryStore {
    private var db: OpaquePointer?

    init() {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/WisprLite")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dbPath = dir.appendingPathComponent("history.db").path

        if sqlite3_open(dbPath, &db) == SQLITE_OK {
            createTable()
            NSLog("Wispr Lite: History database opened at %@", dbPath)
        } else {
            NSLog("Wispr Lite: Failed to open history database")
        }
    }

    private func createTable() {
        let sql = """
            CREATE TABLE IF NOT EXISTS transcripts (
                id TEXT PRIMARY KEY,
                asr_text TEXT,
                formatted_text TEXT,
                timestamp REAL,
                app_name TEXT,
                app_bundle_id TEXT,
                duration REAL,
                num_words INTEGER,
                language TEXT
            );
            """
        exec(sql)
    }

    func addEntry(result: TranscriptResult, appInfo: [String: String]) {
        let sql = """
            INSERT OR REPLACE INTO transcripts
            (id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_text(stmt, 1, (result.id as NSString).utf8String, -1, nil)
        bindOptionalText(stmt, 2, result.asrText)
        bindOptionalText(stmt, 3, result.formattedText)
        sqlite3_bind_double(stmt, 4, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 5, ((appInfo["name"] ?? "") as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 6, ((appInfo["bundle_id"] ?? "") as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 7, result.duration)
        sqlite3_bind_int(stmt, 8, Int32(result.numWords))
        sqlite3_bind_text(stmt, 9, ("en" as NSString).utf8String, -1, nil)

        sqlite3_step(stmt)
    }

    func getEntries(limit: Int = 100, offset: Int = 0) -> [TranscriptEntry] {
        let sql = "SELECT * FROM transcripts ORDER BY timestamp DESC LIMIT ? OFFSET ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int(stmt, 1, Int32(limit))
        sqlite3_bind_int(stmt, 2, Int32(offset))

        var entries: [TranscriptEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            entries.append(entryFromRow(stmt))
        }
        return entries
    }

    func search(query: String) -> [TranscriptEntry] {
        let sql = "SELECT * FROM transcripts WHERE formatted_text LIKE ? OR asr_text LIKE ? ORDER BY timestamp DESC LIMIT 100;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        let pattern = "%\(query)%"
        sqlite3_bind_text(stmt, 1, (pattern as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (pattern as NSString).utf8String, -1, nil)

        var entries: [TranscriptEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            entries.append(entryFromRow(stmt))
        }
        return entries
    }

    func deleteEntry(id: String) {
        exec("DELETE FROM transcripts WHERE id = '\(id)';")
    }

    func clearAll() {
        exec("DELETE FROM transcripts;")
    }

    func close() {
        sqlite3_close(db)
        db = nil
    }

    private func entryFromRow(_ stmt: OpaquePointer?) -> TranscriptEntry {
        TranscriptEntry(
            id: columnText(stmt, 0) ?? "",
            asrText: columnText(stmt, 1),
            formattedText: columnText(stmt, 2),
            timestamp: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 3)),
            appName: columnText(stmt, 4) ?? "",
            appBundleId: columnText(stmt, 5) ?? "",
            duration: sqlite3_column_double(stmt, 6),
            numWords: Int(sqlite3_column_int(stmt, 7)),
            language: columnText(stmt, 8) ?? "en"
        )
    }

    private func columnText(_ stmt: OpaquePointer?, _ index: Int32) -> String? {
        guard let cStr = sqlite3_column_text(stmt, index) else { return nil }
        return String(cString: cStr)
    }

    private func bindOptionalText(_ stmt: OpaquePointer?, _ index: Int32, _ value: String?) {
        if let value = value {
            sqlite3_bind_text(stmt, index, (value as NSString).utf8String, -1, nil)
        } else {
            sqlite3_bind_null(stmt, index)
        }
    }

    private func exec(_ sql: String) {
        sqlite3_exec(db, sql, nil, nil, nil)
    }
}
