import Foundation
import SQLite3

class DictionaryStore {
    private let dbManager: DatabaseManager

    // Cached query results for the transcription hot path
    private var cachedVocabulary: [String]?
    private var cachedReplacements: [String: String]?
    private var cachedSnippets: [String: String]?

    init(dbManager: DatabaseManager) {
        self.dbManager = dbManager
        createTable()
    }

    private func invalidateCache() {
        cachedVocabulary = nil
        cachedReplacements = nil
        cachedSnippets = nil
    }

    private func createTable() {
        let sql = """
            CREATE TABLE IF NOT EXISTS dictionary (
                id TEXT PRIMARY KEY,
                phrase TEXT NOT NULL,
                replacement TEXT,
                team_dictionary_id TEXT DEFAULT '00000000-0000-0000-0000-000000000000',
                last_used REAL,
                frequency_used INTEGER DEFAULT 0,
                manual_entry INTEGER DEFAULT 0,
                created_at REAL NOT NULL,
                modified_at REAL NOT NULL,
                is_deleted INTEGER DEFAULT 0,
                source TEXT,
                is_snippet INTEGER DEFAULT 0,
                UNIQUE(phrase, team_dictionary_id)
            );
            """
        dbManager.exec(sql)
    }

    func addEntry(phrase: String, replacement: String?, isSnippet: Bool, source: String = "manual", manualEntry: Bool = true) {
        let sql = """
            INSERT OR IGNORE INTO dictionary
            (id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at)
            VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?);
            """
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        let now = Date().timeIntervalSince1970
        sqlite3_bind_text(stmt, 1, (UUID().uuidString as NSString).utf8String, -1, nil)
        sqlite3_bind_text(stmt, 2, (phrase as NSString).utf8String, -1, nil)
        dbManager.bindOptionalText(stmt, 3, replacement)
        sqlite3_bind_int(stmt, 4, isSnippet ? 1 : 0)
        sqlite3_bind_int(stmt, 5, manualEntry ? 1 : 0)
        sqlite3_bind_text(stmt, 6, (source as NSString).utf8String, -1, nil)
        sqlite3_bind_double(stmt, 7, now)
        sqlite3_bind_double(stmt, 8, now)

        sqlite3_step(stmt)
        invalidateCache()
    }

    func updateEntry(id: String, phrase: String, replacement: String?) {
        let sql = "UPDATE dictionary SET phrase = ?, replacement = ?, modified_at = ? WHERE id = ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_text(stmt, 1, (phrase as NSString).utf8String, -1, nil)
        dbManager.bindOptionalText(stmt, 2, replacement)
        sqlite3_bind_double(stmt, 3, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 4, (id as NSString).utf8String, -1, nil)

        sqlite3_step(stmt)
        invalidateCache()
    }

    func softDelete(id: String) {
        let sql = "UPDATE dictionary SET is_deleted = 1, modified_at = ? WHERE id = ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_double(stmt, 1, Date().timeIntervalSince1970)
        sqlite3_bind_text(stmt, 2, (id as NSString).utf8String, -1, nil)

        sqlite3_step(stmt)
        invalidateCache()
    }

    func getVocabularyPhrases(limit: Int = 50) -> [String] {
        if let cached = cachedVocabulary { return cached }

        let sql = "SELECT phrase FROM dictionary WHERE is_snippet = 0 AND is_deleted = 0 ORDER BY frequency_used DESC LIMIT ?;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int(stmt, 1, Int32(limit))

        var phrases: [String] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            if let phrase = dbManager.columnText(stmt, 0) {
                phrases.append(phrase)
            }
        }
        cachedVocabulary = phrases
        return phrases
    }

    func getReplacements() -> [String: String] {
        if let cached = cachedReplacements { return cached }

        let sql = "SELECT phrase, replacement FROM dictionary WHERE is_snippet = 0 AND replacement IS NOT NULL AND is_deleted = 0;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [:] }
        defer { sqlite3_finalize(stmt) }

        var replacements: [String: String] = [:]
        while sqlite3_step(stmt) == SQLITE_ROW {
            if let phrase = dbManager.columnText(stmt, 0),
               let replacement = dbManager.columnText(stmt, 1) {
                replacements[phrase] = replacement
            }
        }
        cachedReplacements = replacements
        return replacements
    }

    func getSnippets() -> [String: String] {
        if let cached = cachedSnippets { return cached }

        let sql = "SELECT phrase, replacement FROM dictionary WHERE is_snippet = 1 AND is_deleted = 0;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [:] }
        defer { sqlite3_finalize(stmt) }

        var snippets: [String: String] = [:]
        while sqlite3_step(stmt) == SQLITE_ROW {
            if let phrase = dbManager.columnText(stmt, 0),
               let replacement = dbManager.columnText(stmt, 1) {
                snippets[phrase] = replacement
            }
        }
        cachedSnippets = snippets
        return snippets
    }

    func getAllVocabulary() -> [DictionaryEntry] {
        return fetchEntries(snippet: false)
    }

    func getAllSnippets() -> [DictionaryEntry] {
        return fetchEntries(snippet: true)
    }

    private func entryFromRow(_ stmt: OpaquePointer?) -> DictionaryEntry {
        DictionaryEntry(
            id: dbManager.columnText(stmt, 0) ?? "",
            phrase: dbManager.columnText(stmt, 1) ?? "",
            replacement: dbManager.columnText(stmt, 2),
            isSnippet: sqlite3_column_int(stmt, 3) == 1,
            manualEntry: sqlite3_column_int(stmt, 4) == 1,
            source: dbManager.columnText(stmt, 5),
            frequencyUsed: Int(sqlite3_column_int(stmt, 6)),
            createdAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 7)),
            modifiedAt: Date(timeIntervalSince1970: sqlite3_column_double(stmt, 8))
        )
    }

    private func fetchEntries(snippet: Bool) -> [DictionaryEntry] {
        let sql = "SELECT id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at FROM dictionary WHERE is_snippet = ? AND is_deleted = 0 ORDER BY modified_at DESC;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int(stmt, 1, snippet ? 1 : 0)

        var entries: [DictionaryEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            entries.append(entryFromRow(stmt))
        }
        return entries
    }

    func searchEntries(query: String, snippet: Bool) -> [DictionaryEntry] {
        let sql = "SELECT id, phrase, replacement, is_snippet, manual_entry, source, frequency_used, created_at, modified_at FROM dictionary WHERE is_snippet = ? AND is_deleted = 0 AND phrase LIKE ? ORDER BY modified_at DESC;"
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(dbManager.db, sql, -1, &stmt, nil) == SQLITE_OK else { return [] }
        defer { sqlite3_finalize(stmt) }

        sqlite3_bind_int(stmt, 1, snippet ? 1 : 0)
        let pattern = "%\(query)%"
        sqlite3_bind_text(stmt, 2, (pattern as NSString).utf8String, -1, nil)

        var entries: [DictionaryEntry] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            entries.append(entryFromRow(stmt))
        }
        return entries
    }

    func importCSV(url: URL) -> (imported: Int, errors: [String]) {
        guard let content = try? String(contentsOf: url, encoding: .utf8) else {
            return (0, ["Failed to read file"])
        }

        var imported = 0
        var errors: [String] = []

        let lines = content.components(separatedBy: .newlines)
        for (index, line) in lines.enumerated() {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            guard !trimmed.isEmpty else { continue }
            // Skip header row
            if index == 0 && (trimmed.lowercased().contains("phrase") || trimmed.lowercased().contains("abbreviation")) { continue }

            let parts = trimmed.components(separatedBy: ",")
            guard parts.count >= 1 else {
                errors.append("Line \(index + 1): invalid format")
                continue
            }

            let phrase = parts[0].trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            let replacement = parts.count >= 2 ? parts[1...].joined(separator: ",").trimmingCharacters(in: .whitespaces).trimmingCharacters(in: CharacterSet(charactersIn: "\"")) : nil

            guard !phrase.isEmpty else {
                errors.append("Line \(index + 1): empty phrase")
                continue
            }

            let isSnippet = replacement != nil
            addEntry(phrase: phrase, replacement: replacement, isSnippet: isSnippet, source: "csv_import")
            imported += 1
        }

        return (imported, errors)
    }

    func addAutoLearnedWord(phrase: String) {
        addEntry(phrase: phrase, replacement: nil, isSnippet: false, source: "user_edits", manualEntry: false)
    }

    func addAutoLearnedWords(phrases: [String]) {
        guard !phrases.isEmpty else { return }
        dbManager.transaction {
            for phrase in phrases {
                addEntry(phrase: phrase, replacement: nil, isSnippet: false, source: "user_edits", manualEntry: false)
            }
        }
    }

    func seedDefaults(userName: String?) {
        if let name = userName, !name.isEmpty {
            addEntry(phrase: name, replacement: nil, isSnippet: false, source: "default", manualEntry: false)
        }
        addEntry(phrase: "Wispr Lightning", replacement: nil, isSnippet: false, source: "default", manualEntry: false)
    }
}
