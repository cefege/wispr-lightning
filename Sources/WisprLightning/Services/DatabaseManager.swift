import Foundation
import SQLite3

class DatabaseManager {
    let db: OpaquePointer?
    /// Apple's libsqlite3 is built SQLITE_THREADSAFE=1 (serialized handles),
    /// so multi-thread access *technically* works. But the WAL log + cache
    /// behaviors are more predictable when all writes funnel through one
    /// queue, and a serial queue makes the threading model explicit for
    /// future store classes that might assume otherwise.
    let queue = DispatchQueue(label: "com.wisprlightning.db")

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

    /// SQLite-native schema version. We track the schema we wrote vs. the
    /// one the DB currently has and apply migrations in order. Stores call
    /// `migrate(to:, applying:)` once at init to bring their tables forward.
    var userVersion: Int {
        get {
            var stmt: OpaquePointer?
            guard sqlite3_prepare_v2(db, "PRAGMA user_version;", -1, &stmt, nil) == SQLITE_OK else { return 0 }
            defer { sqlite3_finalize(stmt) }
            guard sqlite3_step(stmt) == SQLITE_ROW else { return 0 }
            return Int(sqlite3_column_int(stmt, 0))
        }
        set {
            sqlite3_exec(db, "PRAGMA user_version = \(newValue);", nil, nil, nil)
        }
    }

    /// Apply `migrations` (ordered, idempotent SQL strings) until the
    /// database's user_version matches the count. Stores can add a new
    /// migration by appending to their array — existing installs only run
    /// the new ones; fresh installs run them all.
    func migrate(_ migrations: [String]) {
        queue.sync {
            let current = userVersion
            guard current < migrations.count else { return }
            for (idx, sql) in migrations.enumerated() where idx >= current {
                exec("BEGIN TRANSACTION;")
                if sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK {
                    userVersion = idx + 1
                    exec("COMMIT;")
                } else {
                    NSLog("Wispr Lightning: schema migration %d failed — %s",
                          idx, String(cString: sqlite3_errmsg(db)))
                    exec("ROLLBACK;")
                    return
                }
            }
        }
    }

    /// Run `block` with serialized access to the underlying handle. Stores
    /// that mix reads and writes from background queues should wrap their
    /// statement-prep + step + finalize sequences in this to keep the WAL
    /// behavior predictable.
    func sync<T>(_ block: () -> T) -> T {
        return queue.sync(execute: block)
    }

    func transaction(_ block: () -> Void) {
        queue.sync {
            exec("BEGIN TRANSACTION;")
            block()
            exec("COMMIT;")
        }
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
