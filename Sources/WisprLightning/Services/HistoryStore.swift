import Foundation
import SQLite3

class HistoryStore {
    private let dbManager: DatabaseManager
    private var db: OpaquePointer? { dbManager.db }

    init(dbManager: DatabaseManager) {
        self.dbManager = dbManager
        createTable()
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
        dbManager.exec(sql)
    }

    func addEntry(result: TranscriptResult, appInfo: [String: String], language: String = "en") {
        let sql = """
            INSERT OR REPLACE INTO transcripts
            (id, asr_text, formatted_text, timestamp, app_name, app_bundle_id, duration, num_words, language)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?);
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_text(stmt, 1, (result.id as NSString).utf8String, -1, nil)
        dbManager.bindOptionalText(stmt, 2, result.asrText)
        dbManager.bindOptionalText(stmt, 3, result.formattedText)
        sqlite3_bind_double(stmt, 4, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 5, ((appInfo["name"] ?? "") as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 6, ((appInfo["bundle_id"] ?? "") as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 7, result.duration)
        sqlite3_bind_int(stmt, 8, Int32(result.numWords))
        sqlite3_bind_text(stmt, 9, (language as NSString).utf8String, -1, nil)

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
        let sql = "DELETE FROM transcripts WHERE id = ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_text(stmt, 1, (id as NSString).utf8String, -1, nil)
        sqlite3_step(stmt)
    }

    func clearAll() {
        dbManager.exec("DELETE FROM transcripts;")
    }

    func todayStats() -> (dictations: Int, words: Int) {
        let startOfDay = Calendar.current.startOfDay(for: Date()).timeIntervalSince1970
        let sql = "SELECT COUNT(*), COALESCE(SUM(num_words), 0) FROM transcripts WHERE timestamp >= ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK else { return (0, 0) }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_double(stmt, 1, startOfDay)
        guard sqlite3_step(stmt) == SQLITE_ROW else { return (0, 0) }
        return (Int(sqlite3_column_int(stmt, 0)), Int(sqlite3_column_int(stmt, 1)))
    }

    func close() {
        // Closing is now handled by DatabaseManager
    }

    private func entryFromRow(_ stmt: OpaquePointer?) -> TranscriptEntry {
        TranscriptEntry(
            id: dbManager.columnText(stmt, 0) ?? "",
            asrText: dbManager.columnText(stmt, 1),
            formattedText: dbManager.columnText(stmt, 2),
            timestamp: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 3)),
            appName: dbManager.columnText(stmt, 4) ?? "",
            appBundleId: dbManager.columnText(stmt, 5) ?? "",
            duration: sqlite3_column_double(stmt, 6),
            numWords: Int(sqlite3_column_int(stmt, 7)),
            language: dbManager.columnText(stmt, 8) ?? "en"
        )
    }
}
