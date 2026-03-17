import Foundation
import SQLite3

class PolishStore {
    private let dbManager: DatabaseManager

    init(dbManager: DatabaseManager) {
        self.dbManager = dbManager
        createTable()
    }

    private func createTable() {
        let sql = """
            CREATE TABLE IF NOT EXISTS polish (
                id TEXT PRIMARY KEY,
                initial_text TEXT,
                polished_text TEXT,
                initial_word_count INTEGER,
                polished_word_count INTEGER,
                app TEXT,
                processing_time REAL,
                status TEXT,
                polish_undone INTEGER DEFAULT 0,
                instruction TEXT,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL
            );
            """
        dbManager.exec(sql)
    }

    func saveResult(_ result: PolishResult, app: String = "") {
        let sql = """
            INSERT OR REPLACE INTO polish
            (id, initial_text, polished_text, initial_word_count, polished_word_count, app, processing_time, status, instruction, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'completed', ?, ?, ?);
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        let now = Date().timeIntervalSince1970
        sqlite3_bind_text(stmt, 1, (result.id as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (result.initialText as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 3, (result.polishedText as NSString).utf8String, -1, nil)
        sqlite3_bind_int(stmt, 4, Int32(result.initialWordCount))
        sqlite3_bind_int(stmt, 5, Int32(result.polishedWordCount))
        sqlite3_bind_text(stmt, 6, (app as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 7, result.processingTime)
        sqlite3_bind_text(stmt, 8, (result.instruction as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 9, now)
        sqlite3_bind_double(stmt, 10, now)

        sqlite3_step(stmt)
    }
}
