{
  "summary": "Complete behavioral spec of Wispr Lightning's data layer (SQLite via raw SQLite3 C API), stores (History/Dictionary/Notes/Polish), PolishService HTTP call, AuthService Supabase-Google OAuth via custom URL scheme, the TranscriptionClient WebSocket tail (append chunking / commit / response parsing / custom Ascii85), and AppSettings JSON persistence. Key findings for the Rust/Tauri port: (a) exactly 4 tables, ZERO indexes, ZERO schema versioning — migration is only a file rename history.db -> lightning.db; (b) PolishService has NO system prompt and NO model name — the server owns the prompt, the client sends only an instruction-name -> bool map, and there is NO explicit timeout (URLSession default 60s); (c) the ascii85 encoder is standard btoa/Python base64.a85encode (offset 33, 'z' for zero full groups, no Adobe delimiters, no line wrap); (d) auth relies on the macOS-shared 'wispr-flow://' URL scheme plus a filesystem watcher on Wispr Flow's session.json — the single biggest Windows parity risk.",
  "files": [
    {
      "path": "Sources/WisprLightning/Services/DatabaseManager.swift",
      "description": "Opens ~/Library/Application Support/WisprLightning/lightning.db, renames legacy history.db, sets PRAGMA journal_mode=WAL, provides exec/transaction/columnText/bindOptionalText helpers. No index or version migrations exist."
    },
    {
      "path": "Sources/WisprLightning/Services/HistoryStore.swift",
      "description": "`transcripts` table; INSERT OR REPLACE on save; SELECT * ... ORDER BY timestamp DESC LIMIT ? OFFSET ?; LIKE search capped at 100; today's stats via COUNT/COALESCE(SUM(num_words),0)."
    },
    {
      "path": "Sources/WisprLightning/Services/DictionaryStore.swift",
      "description": "`dictionary` table with UNIQUE(phrase, team_dictionary_id); INSERT OR IGNORE; soft delete; three memoized hot-path queries (vocabulary LIMIT 50 by frequency_used DESC, replacements map, snippets map); CSV import; auto-learn helper."
    },
    {
      "path": "Sources/WisprLightning/Services/NotesStore.swift",
      "description": "`notes` table; content_preview is content.prefix(200) recomputed on every write; list/search ORDER BY modified_at DESC LIMIT."
    },
    {
      "path": "Sources/WisprLightning/Services/PolishStore.swift",
      "description": "`polish` table; write-only store (no read queries at all); status hard-coded to 'completed'; polish_undone column never written."
    },
    {
      "path": "Sources/WisprLightning/Services/PolishService.swift",
      "description": "POST https://api.wisprflow.ai/llm/polish_text; body keys selected_text/instructions/provider_config/writing_samples/custom_prompt; raw token in Authorization (no Bearer); parses only polished_text and status."
    },
    {
      "path": "Sources/WisprLightning/Services/AuthService.swift",
      "description": "Opens Supabase Google authorize URL in the default browser with redirect_to=wispr-flow://auth/google/success; parses tokens from query (fragment fallback); decodes JWT payload unverified for sub/email/user_metadata."
    },
    {
      "path": "Sources/WisprLightning/Models/Session.swift",
      "description": "Token storage/refresh; session.json in Supabase 'sb-<ref>-auth-token' stringified-JSON format; 60s expiry skew; refresh via /auth/v1/token?grant_type=refresh_token."
    },
    {
      "path": "Sources/WisprLightning/Services/TranscriptionClient.swift",
      "description": "WebSocket auth/append/commit protocol, chunkSize=500 packets, dynamic response timeout max(15s, dur*0.5), final-result parsing, custom Ascii85 encoder."
    },
    {
      "path": "Sources/WisprLightning/Models/Settings.swift",
      "description": "AppSettings Codable; settings.json pretty-printed next to lightning.db; polishInstructions default dict; posts WisprLightningSettingsChanged on every save."
    },
    {
      "path": "Sources/WisprLightning/Services/Constants.swift",
      "description": "All endpoint URLs, Supabase anon key, audio constants (16 kHz, 40 ms, 640 samples), clientVersion 1.4.549, recording limits."
    },
    {
      "path": "Sources/WisprLightning/App/AppDelegate.swift",
      "description": "Wires stores, handles kAEGetURL auth callbacks, auto-learn-words algorithm, polish hotkey flow (Cmd+C simulation), Wispr Flow session directory watcher."
    },
    {
      "path": "Resources/Info.plist",
      "description": "CFBundleURLSchemes = [\"wispr-flow\", \"wisprlightning\"] under bundle id com.wisprlightning.app."
    }
  ],

[Showing lines 1-56 of 58 (4.4KB limit). Use :57 to continue]