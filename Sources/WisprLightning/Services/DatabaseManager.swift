import Foundation
import SQLite3

class DatabaseManager {
    let db: OpaquePointer?

    init() {
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/WisprLightning")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        // Migrate legacy history.db → lightning.db if needed
        let historyPath = dir.appendingPathComponent("history.db")
        let lightningPath = dir.appendingPathComponent("lightning.db")
        if FileManager.default.fileExists(atPath: historyPath.path) &&
           !FileManager.default.fileExists(atPath: lightningPath.path) {
            try? FileManager.default.moveItem(at: historyPath, to: lightningPath)
            NSLog("Wispr Lightning: Migrated history.db → lightning.db")
        }

        let dbPath = lightningPath.path

        var dbPointer: OpaquePointer?
        if sqlite3_open(dbPath, &dbPointer) == SQLITE_OK {
            self.db = dbPointer
            // Enable WAL mode for safe multi-thread access
            sqlite3_exec(db, "PRAGMA journal_mode=WAL;", nil, nil, nil)
            NSLog("Wispr Lightning: Database opened at %@", dbPath)
        } else {
            self.db = nil
            NSLog("Wispr Lightning: Failed to open database at %@", dbPath)
        }
    }

    func exec(_ sql: String) {
        sqlite3_exec(db, sql, nil, nil, nil)
    }

    func transaction(_ block: () -> Void) {
        exec("BEGIN TRANSACTION;")
        block()
        exec("COMMIT;")
    }

    func columnText(_ stmt: OpaquePointer?, _ index: Int32) -> String? {
        guard let cStr = sqlite3_column_text(stmt, index) else { return nil }
        return String(cString: cStr)
    }

    func bindOptionalText(_ stmt: OpaquePointer?, _ index: Int32, _ value: String?) {
        if let value = value {
            sqlite3_bind_text(stmt, index, (value as NSString).utf8String, -1, nil)
        } else {
            sqlite3_bind_null(stmt, index)
        }
    }

    func close() {
        sqlite3_close(db)
    }
}
