{
  "summary": "Complete UI specification for the 8 files in Sources/WisprLightning/UI/. Covers: SettingsWindow (860x580 NavigationSplitView, 8 sidebar sections, every control in order with label/type/options/binding/default/dependency), Theme constants, RecordingOverlay (NSPanel states, sizes, positions, timings — note: there is NO waveform/bar visualizer, only a single 10pt pulsing dot), ToastNotification (instantiated in AppDelegate but never invoked — dead code), HistoryWindow, NotesView, DictionaryView. Defaults were resolved from Models/Settings.swift (JSON file at ~/Library/Application Support/WisprLightning/settings.json), keycode labels from Services/HotkeyListener.swift, recording timing from Services/Constants.swift, error strings from Models/TranscriptEntry.swift.",
  "files": [
    {
      "path": "Sources/WisprLightning/UI/SettingsWindow.swift",
      "description": "1435 lines. AllSettingsView (NavigationSplitView), SettingsWindowController (NSWindow 860x580), 9 detail sections, KeyCapView, SettingsViewModel with 101-entry language table + hotkey capture via NSEvent local monitors."
    },
    {
      "path": "Sources/WisprLightning/UI/DictionaryView.swift",
      "description": "475 lines. Segmented Vocabulary/Snippets tabs, search, add/edit sheets (380/420pt wide), CSV import via NSOpenPanel, DictionaryViewModel."
    },
    {
      "path": "Sources/WisprLightning/UI/RecordingOverlay.swift",
      "description": "317 lines. NSPanel .nonactivatingPanel floating overlay, 36pt tall, widths 120/145/175/180/200/260/300, bottom-center +50pt, pulsing dot, Retry/Save/✕ buttons."
    },
    {
      "path": "Sources/WisprLightning/UI/HistoryWindow.swift",
      "description": "215 lines. HistoryView list grouped by Today/Yesterday/'MMM d', searchable toolbar, copy/delete per row, Clear All, NSAlert confirmations."
    },
    {
      "path": "Sources/WisprLightning/UI/NotesView.swift",
      "description": "202 lines. Notes list + search + New Note, editor sheet 500x400, context menu Edit/Delete."
    },
    {
      "path": "Sources/WisprLightning/UI/ToastNotification.swift",
      "description": "105 lines. NSPanel toast, 120x40 (340 if msg>30 chars), bottom-center +50pt, 0.25s fade in / 0.3s fade out, 1.5s dwell. Only message is \"Done\" with bolt.fill icon. NEVER CALLED anywhere in the app."
    },
    {
      "path": "Sources/WisprLightning/UI/Theme.swift",
      "description": "32 lines. Semantic NSColor aliases, 4 preferredFont text styles, spacing 4/8/16/24."
    },
    {
      "path": "Sources/WisprLightning/UI/AutoLayoutHelpers.swift",
      "description": "34 lines. NSView.pinToSuperview(insets:), NSView.setSize(width:height:), NSEdgeInsets(all:) / (horizontal:vertical:)."
    },
    {
      "path": "Sources/WisprLightning/Models/Settings.swift",
      "description": "Read for authoritative defaults + JSON keys of every settings binding."
    },
    {
      "path": "Sources/WisprLightning/Services/HotkeyListener.swift",
      "description": "Read for keycodeLabels map (exact keycap strings)."
    },
    {
      "path": "Sources/WisprLightning/Services/Constants.swift",
      "description": "Read for maxRecordingSeconds=600, warningSeconds=540, finalWarningSeconds=570."
    },
    {
      "path": "Sources/WisprLightning/Models/TranscriptEntry.swift",
      "description": "Read for TranscriptionError.userMessage strings shown in the overlay."
    }
  ],

[Showing lines 1-52 of 54 (3.4KB limit). Use :53 to continue]