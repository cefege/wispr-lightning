# Wispr Lightning — UI Specification (for HTML/CSS/Tauri rebuild)

All numbers are macOS points (1pt = 1 CSS px at 1x). All strings are verbatim.

---

## 1. Theme.swift (32 lines)

`enum Theme` — three nested namespaces. **No hardcoded RGBA anywhere**; everything is a macOS *semantic* color that flips with light/dark mode. Concrete values below are the standard macOS resolutions (marked `[INFERENCE]` where the source only names the semantic token).

### Theme.Colors
| Constant | Source token | Light value `[INFERENCE]` | Dark value `[INFERENCE]` |
|---|---|---|---|
| `background` | `NSColor.windowBackgroundColor` | `#ECECEC` | `#323232` |
| `secondaryText` | `NSColor.secondaryLabelColor` | `rgba(0,0,0,0.50)` | `rgba(255,255,255,0.55)` |
| `accent` | `NSColor.controlAccentColor` | user-chosen; default blue `#007AFF` | `#0A84FF` |
| `error` | `NSColor.systemRed` | `#FF3B30` | `#FF453A` |
| `hintText` | `NSColor.tertiaryLabelColor` | `rgba(0,0,0,0.26)` | `rgba(255,255,255,0.25)` |

SwiftUI mirrors: `swiftAccent`, `swiftSecondaryText`, `swiftError`, `swiftHintText` (same values, `Color(nsColor:)` wrapped).

### Theme.Fonts (all dynamic `NSFont.preferredFont(forTextStyle:)`)
| Constant | Text style | Default size / weight `[INFERENCE]` |
|---|---|---|
| `title` | `.title3` | 15pt regular |
| `heading` | `.headline` | 13pt semibold |
| `body` | `.body` | 13pt regular |
| `caption` | `.subheadline` | 11pt regular |

### Theme.Spacing
`small = 4`, `medium = 8`, `large = 16`, `xlarge = 24`.

### Corner radii used across the app (not in Theme, but literal in call sites)
- Recording overlay: **18**
- Toast: **12**
- Sidebar app icon: **14**
- Language chips: **12**
- Section icon tile: **7**
- KeyCapView, language list box: **6**
- Search fields (Dictionary/Notes): **6**
- Vocabulary `source` badge: **4**

---

## 2. AutoLayoutHelpers.swift (34 lines)

Pure AppKit layout sugar; **no visible UI**. Nothing to port literally — the CSS equivalents are `position:absolute; inset:<insets>` and fixed `width`/`height`.

- `NSView.pinToSuperview(insets: NSEdgeInsets = .zero)` — leading/trailing/top/bottom constraints to superview with insets (trailing & bottom negated).
- `NSView.setSize(width: CGFloat? = nil, height: CGFloat? = nil)` — activates constant constraints for whichever is non-nil.
- `NSEdgeInsets(all:)` and `NSEdgeInsets(horizontal:vertical:)` convenience initializers.

---

## 3. SettingsWindow.swift (1435 lines)

### 3.1 Window chrome (`SettingsWindowController.showWindow()`)
| Property | Value |
|---|---|
| Content rect | `860 × 580` |
| Min size | `680 × 460` |
| Style mask | titled, closable, miniaturizable, resizable, fullSizeContentView |
| Title | `"Wispr Lightning Settings"` |
| Titlebar | opaque (`titlebarAppearsTransparent = false`), `titleVisibility = .visible`, `toolbarStyle = .unified` |
| Position | `w.center()` on first show |
| Frame autosave | `"SettingsWindow"` (position/size persisted) |
| Released on close | `false` — window instance reused |

Second invocation of `showWindow()` calls `makeKeyAndOrderFront` + `NSApp.activate(ignoringOtherApps: true)`; **it does not rebuild the view model**, so on-disk changes made elsewhere are not re-read.

### 3.2 Layout skeleton
`NavigationSplitView`:
- **Sidebar**: fixed column width **220**, `.listStyle(.sidebar)`, native sidebar-toggle button removed on macOS 14+.
  - `safeAreaInset(edge: .top)`: the bundled PNG `WisprFlowIcon.png`, `64 × 64`, clipped to `RoundedRectangle(cornerRadius: 14)`, horizontally centered between two Spacers, `padding(.top, 16)`, `padding(.bottom, 8)`, clear background.
  - Three unlabeled `Section`s (visually: separator gaps, no headers):
    1. General, Dictation, Polish
    2. History, Dictionary, Notes
    3. Privacy, System
  - Each row = `Label` with `SectionIcon` + title, `.padding(.vertical, 1)`.
- **Detail**: `.navigationTitle(selectedSection.title)`.
  - For `.history` / `.dictionary` / `.notes` → the respective view fills the pane edge-to-edge (no ScrollView, no 28pt padding).
  - For all other sections → `ScrollView { VStack(alignment: .leading, spacing: 16) { … } .padding(28) .frame(maxWidth: .infinity, alignment: .leading) }`.
- Default selection: **`.general`**.
- `.onAppear` and on `Notification.Name("WisprSessionChanged")` → `refreshAccount()`.

### 3.3 `SectionIcon` tile
`28 × 28` rounded rect, `cornerRadius: 7`, SF Symbol at `size: 13, weight: .semibold`, white glyph, vertical `LinearGradient` top→bottom.

| Section | SF Symbol | Gradient top → bottom (exact RGB 0–1) |
|---|---|---|
| General | `gearshape.fill` | Gray `(0.64,0.64,0.70)` → `(0.48,0.48,0.55)` |
| Dictation | `mic.fill` | Blue `(0.30,0.57,1.00)` → `(0.14,0.38,0.96)` |
| Polish | `sparkles` | Purple `(0.72,0.38,1.00)` → `(0.55,0.22,0.94)` |
| History | `clock.fill` | Orange `(1.00,0.68,0.22)` → `(0.98,0.50,0.02)` |
| Dictionary | `character.book.closed.fill` | Green `(0.34,0.82,0.44)` → `(0.20,0.70,0.30)` |
| Notes | `note.text` | Yellow `(1.00,0.84,0.18)` → `(0.98,0.70,0.04)` |
| Privacy | `hand.raised.fill` | Blue (same as Dictation) |
| System | `desktopcomputer` | Gray (same as General) |

Hex equivalents: Gray `#A3A3B3`→`#7A7A8C`; Blue `#4D91FF`→`#2461F5`; Purple `#B861FF`→`#8C38F0`; Orange `#FFAD38`→`#FA8005`; Green `#57D170`→`#33B34D`; Yellow `#FFD62E`→`#FAB30A`.

### 3.4 `SettingsToggleRow` (the reusable row used by most toggles)
`Toggle(.switch style, controlSize .small)` whose label is a left-aligned `VStack(spacing: 2)`:
- line 1: `title`, default body font;
- line 2 (optional): `description`, `.subheadline`, `.regular` weight, `.secondary` foreground.
Label block is `.frame(maxWidth: .infinity, alignment: .leading)` so the switch is flush right.

### 3.5 Detail panes — every control, in order

All GroupBoxes have `.padding(Theme.Spacing.medium /* 8 */)` inside and an inner `VStack(alignment: .leading, spacing: 8)` unless noted. All section headers are `Text(...).font(.title3.weight(.semibold))`.

---

#### **Tab: General** — three groups, separated by `Divider()`

**Group header `"Account"`** (GroupBox, inner VStack spacing 8)

*Signed-in state* — `HStack(spacing: 8)`:
1. Avatar: `AsyncImage(url:)` `32 × 32`, `scaledToFill`, `Circle()` clip. Placeholder / no-URL fallback: SF Symbol `person.crop.circle.fill`, `.title2` font, `.secondary`.
2. `VStack(spacing: 2)`: display name (`.body.weight(.medium)`) — **rendered only if non-empty AND ≠ email**; then email (`.caption`, `.secondary`).
3. `Spacer()`
4. Button **`"Sign Out"`** (`controlSize .small`) → `session.clear()` + posts `WisprSessionChanged`.

*Signed-out state* — `HStack(spacing: 8)`:
1. SF Symbol `person.crop.circle.badge.questionmark`, `.title2`, `.secondary`.
2. `Text("Not signed in")`, `.secondary`.
3. `Spacer()`
4. Button **`"Sign In with Google"`** (`controlSize .small`) → `AuthService.signInWithBrowser()` (opens system browser).

`displayName` derivation: `[firstName, lastName]` filtered non-empty, joined with `" "`; falls back to email if empty.

**Group header `"Dictation Hotkeys"`**
1. `Text("Any of these keys will start dictation:")` — `.secondary`.
2. For each entry of `hotkeyLabels` (default `["Left Control"]`), an `HStack(spacing: 8)`:
   - `KeyCapView(label:)` — monospaced `.body.weight(.medium)`, `minWidth: 40`, `padding(.horizontal, 12)`, `padding(.vertical, 6)`, `controlBackgroundColor` fill, `cornerRadius 6`, 1pt `separatorColor` stroke.
   - Minus button: SF Symbol `minus.circle`, red, `.borderless`, tooltip **`"Remove this hotkey"`**. **Shown only when `hotkeyLabels.count > 1`.** Removing also rewrites `hotkeyKeyCode`/`hotkeyLabel` to index 0.
3. Button whose title is **`"Add Hotkey"`**, or **`"Press a key…"`** while capturing (`controlSize .small`). Toggles capture: installs an `NSEvent` **local** monitor for `.keyDown` + `.flagsChanged`. On `flagsChanged` only a *press* (not release) counts. Label = `HotkeyListener.keycodeLabels[keycode]` else `charactersIgnoringModifiers.uppercased()` else `"?"`. Duplicate keycode → silently cancels capture without adding.
4. Footer `Text("Modifier keys work as hold-to-talk. Regular keys use press-to-toggle.")` — `.subheadline`, `.tertiary`.

**`HotkeyListener.keycodeLabels` (exact map — the only pre-named keycaps):**
`59: "Left Control"`, `62: "Right Control"`, `58: "Left Option"`, `61: "Right Option"`, `55: "Left Command"`, `54: "Right Command"`, `56: "Left Shift"`, `60: "Right Shift"`, `63: "Fn"`, `36: "Return"`, `49: "Space"`, `53: "Escape"`, `48: "Tab"`.

**Group header `"Input Device"`**
1. `Picker` (labels hidden, default menu/pop-up style) bound to `micDeviceUID`.
   - First option **`"System Default"`** → tag `nil` (**default**).
   - Then one row per device from `AudioRecorder.listInputDevices()` — `(uid, name)`, label = device name.
   - onChange → `saveMicSelection()` (also stores `micDeviceName`).
2. Button `Label("Refresh", systemImage: "arrow.clockwise")`, `controlSize .small` → re-enumerates devices.
3. `Divider()`
4. `SettingsToggleRow` **`"Keep microphone active"`** / desc `"Eliminates startup delay — recommended when using iPhone as microphone"` → key `keepMicrophoneActive`, default **`false`**.

**Group header `"Dictation Languages"`** (inner VStack spacing **4**)
1. `SettingsToggleRow` **`"Auto-detect"`** / desc `"Automatically detect the spoken language"`, `.fontWeight(.medium)`, `.padding(.bottom, 4)`. Bound to `selectedLanguages.contains("auto")`. Default: **off** (`languages = ["en"]`).
2. `Divider().padding(.vertical, 4)`
3. **If Auto-detect is ON** → only `Text("All supported languages will be recognized automatically. Specifying languages manually can improve accuracy.")`, `.subheadline`, `.secondary`. Everything below is hidden.
4. **If Auto-detect is OFF** →
   a. **Chips** (only if ≥1 selected): custom `FlowLayout(spacing: 6)`. Each chip = `HStack(spacing: 4)` of `"<flag> <name>"` (`.subheadline`) + `xmark.circle.fill` button (`.caption`, `.secondary`, borderless). Chip padding `h:8 / v:4`, background `Color.accentColor.opacity(0.12)`, `cornerRadius 12`. Block gets `.padding(.bottom, 4)`.
   b. `TextField("Search languages...")` — `.roundedBorder`, `.padding(.bottom, 4)`. Filter is case-insensitive `contains` on the language **name** only (not the code).
   c. Scrollable list, fixed **height 220**, `textBackgroundColor` fill, `cornerRadius 6`, 1pt `separatorColor` stroke, visible scroll indicators, inner `.padding(.vertical, 4)`. Each row = `.switch` Toggle, `controlSize .small`, label `"<flag> <name>"`, `.padding(.horizontal, 8)`, `.padding(.vertical, 5)`, followed by a `Divider().padding(.leading, 8)`.
   d. Bottom fade overlay: `LinearGradient(.clear → textBackgroundColor.opacity(0.85))` top→bottom, **height 28**, `cornerRadius 6`, `allowsHitTesting(false)`.

**Language selection invariants (`toggleLanguage`)**
- Toggling `"auto"` ON → `selectedLanguages = ["auto"]` (exclusive, wipes all others).
- Toggling `"auto"` OFF → `selectedLanguages = ["en"]`.
- Toggling any specific code → removes `"auto"` first; toggling off the last remaining code resets to `["en"]` (never empty).
- Persisted immediately to `languages`.

**Full language table (104 rows, in display order — code / name / flag):**

> Corrected 2026-08-01: an earlier revision of this line said 101. The enumeration below has always contained 104 entries, and `Sources/WisprLightning/UI/SettingsWindow.swift` has 104 `.init(code:` literals — both counted mechanically. The 101 figure was arithmetic, not data. The four codes a naive two-or-three-character parse drops are `engb`, `zhcn`, `dech` and `hien`, which is the likely origin of the miscount.
`en` English 🇺🇸 · `engb` English — British 🇬🇧 · `zh` Chinese — Traditional (繁體中文) 🇹🇼 · `zhcn` Chinese — Simplified (简体中文) 🇨🇳 · `de` German (Deutsch) 🇩🇪 · `dech` German — Swiss (Deutsch) 🇨🇭 · `es` Spanish (Español) 🇪🇸 · `ru` Russian (Русский) 🇷🇺 · `ko` Korean (한국어) 🇰🇷 · `fr` French (Français) 🇫🇷 · `ja` Japanese (日本語) 🇯🇵 · `pt` Portuguese (Português) 🇧🇷 · `tr` Turkish (Türkçe) 🇹🇷 · `pl` Polish (Polski) 🇵🇱 · `ca` Catalan (Català) 🇪🇸 · `nl` Dutch (Nederlands) 🇳🇱 · `ar` Arabic (العربية) 🇸🇦 · `sv` Swedish (Svenska) 🇸🇪 · `it` Italian (Italiano) 🇮🇹 · `id` Indonesian (Bahasa) 🇮🇩 · `hi` Hindi (हिन्दी) 🇮🇳 · `hien` Hinglish 🇮🇳 · `fi` Finnish (Suomi) 🇫🇮 · `vi` Vietnamese (Tiếng Việt) 🇻🇳 · `he` Hebrew (עברית) 🇮🇱 · `uk` Ukrainian (Українська) 🇺🇦 · `el` Greek (Ελληνικά) 🇬🇷 · `ms` Malay (Bahasa Melayu) 🇲🇾 · `cs` Czech (Čeština) 🇨🇿 · `ro` Romanian (Română) 🇷🇴 · `da` Danish (Dansk) 🇩🇰 · `hu` Hungarian (Magyar) 🇭🇺 · `ta` Tamil (தமிழ்) 🇮🇳 · `no` Norwegian (Norsk) 🇳🇴 · `th` Thai (ไทย) 🇹🇭 · `ur` Urdu (اردو) 🇵🇰 · `hr` Croatian (Hrvatski) 🇭🇷 · `bg` Bulgarian (Български) 🇧🇬 · `lt` Lithuanian (Lietuvių) 🇱🇹 · `la` Latin (Latina) 🌍 · `mi` Maori 🇳🇿 · `ml` Malayalam (മലയാളം) 🇮🇳 · `cy` Welsh (Cymraeg) 🏴󠁧󠁢󠁷󠁬󠁳󠁿 · `sk` Slovak (Slovenčina) 🇸🇰 · `te` Telugu (తెలుగు) 🇮🇳 · `fa` Persian (فارسی) 🇮🇷 · `lv` Latvian (Latviešu) 🇱🇻 · `bn` Bengali (বাংলা) 🇧🇩 · `sr` Serbian (Српски) 🇷🇸 · `az` Azerbaijani (Azərbaycan) 🇦🇿 · `sl` Slovenian (Slovenščina) 🇸🇮 · `kn` Kannada (ಕನ್ನಡ) 🇮🇳 · `et` Estonian (Eesti) 🇪🇪 · `mk` Macedonian (Македонски) 🇲🇰 · `br` Breton (Brezhoneg) 🇫🇷 · `eu` Basque (Euskara) 🇪🇸 · `is` Icelandic (Íslenska) 🇮🇸 · `hy` Armenian (Հայերեն) 🇦🇲 · `ne` Nepali (नेपाली) 🇳🇵 · `mn` Mongolian (Монгол) 🇲🇳 · `bs` Bosnian (Bosanski) 🇧🇦 · `kk` Kazakh (Қазақша) 🇰🇿 · `sq` Albanian (Shqip) 🇦🇱 · `sw` Swahili (Kiswahili) 🇹🇿 · `gl` Galician (Galego) 🇪🇸 · `mr` Marathi (मराठी) 🇮🇳 · `pa` Punjabi (ਪੰਜਾਬੀ) 🇮🇳 · `si` Sinhala (සිංහල) 🇱🇰 · `km` Khmer (ខ្មែរ) 🇰🇭 · `sn` Shona (chiShona) 🇿🇼 · `yo` Yoruba 🇳🇬 · `so` Somali (Soomaali) 🇸🇴 · `af` Afrikaans 🇿🇦 · `oc` Occitan 🌍 · `ka` Georgian (ქართული) 🇬🇪 · `be` Belarusian (Беларуская) 🇧🇾 · `tg` Tajik (Тоҷикӣ) 🇹🇯 · `sd` Sindhi (سنڌي) 🇵🇰 · `gu` Gujarati (ગુજરાતી) 🇮🇳 · `am` Amharic (አማርኛ) 🇪🇹 · `yi` Yiddish (ייִדיש) 🌍 · `lo` Lao (ລາວ) 🇱🇦 · `uz` Uzbek (Oʻzbek) 🇺🇿 · `fo` Faroese (Føroyskt) 🇫🇴 · `ht` Haitian Creole (Kreyòl Ayisyen) 🇭🇹 · `ps` Pashto (پښتو) 🇦🇫 · `tk` Turkmen 🇹🇲 · `nn` Nynorsk 🇳🇴 · `mt` Maltese (Malti) 🇲🇹 · `sa` Sanskrit (संस्कृतम्) 🇮🇳 · `lb` Luxembourgish (Lëtzebuergesch) 🇱🇺 · `my` Myanmar (မြန်မာ) 🇲🇲 · `bo` Tibetan (བོད་སྐད) 🌍 · `tl` Tagalog 🇵🇭 · `mg` Malagasy 🇲🇬 · `as` Assamese (অসমীয়া) 🇮🇳 · `tt` Tatar (Татар) 🇷🇺 · `haw` Hawaiian (ʻŌlelo Hawaiʻi) 🇺🇸 · `ln` Lingala 🇨🇩 · `ha` Hausa 🇳🇬 · `ba` Bashkir (Башҡортса) 🇷🇺 · `jv` Javanese (Basa Jawa) 🇮🇩 · `su` Sundanese (Basa Sunda) 🇮🇩 · `yue` Cantonese (粵語) 🇭🇰

*(Note: chips render in the order of this master table, **not** selection order, because `selectedLanguages` is a `Set` filtered through the master array.)*

---

#### **Tab: Dictation** — two groups separated by `Divider()`

**Group header `"Dictation"`** — every control saves via `saveDictationSettings()` on change.

| # | Control | Label / description | Key | Default | Dependency |
|---|---|---|---|---|---|
| 1 | switch | **AI Formatting** — `"Apply AI formatting to clean up transcriptions"` | `aiFormatting` | `true` | — |
| 2 | **segmented** | (no visible label; `Picker("Cleanup Level")` segmented hides its title) options **None** `none`, **Light** `light`, **Heavy** `heavy` | `autoCleanupLevel` | `"light"` | — |
| 3 | caption | `"How aggressively to clean up filler words"` (`.subheadline`, `.secondary`) | — | — | — |
| 4 | switch | **Voice Commands** — `"Interpret phrases like \"new line\" as commands"` | `commandModeEnabled` | `true` | — |
| 5 | switch | **Auto-detect hyperlinks** — `"Convert spoken URLs to clickable hyperlinks"` | `hyperlinkOn` | `false` | — |
| 6 | switch | **Auto-learn words** — `"Automatically learn new vocabulary from dictations"` | `autoLearnWords` | `true` | — |
| — | `Divider()` | | | | |
| 7 | switch | **Email signature** — `"Append a signature when dictating in email apps"` | `emailAutoSignature` | `false` | — |
| 8 | dropdown (`.menu`) labeled **Signature** — options `"Written with Wispr Lightning"` → `written_with_lightning`, `"Spoken with Wispr Lightning"` → `spoken_with_lightning` | | `emailSignatureOption` | `"written_with_lightning"` | **Entirely hidden (not greyed) unless #7 is ON** |
| — | `Divider()` | | | | |
| 9 | switch | **Creator mode** — `"Extended recording for long-form content (up to 10 min)"` | `creatorMode` | `false` | — |
| — | `Divider()` | | | | |
| 10 | switch | **Natural Mode** — `"Type text character-by-character instead of pasting (slower but feels human)"` | `naturalModeEnabled` | `false` | — |
| 11 | **segmented** labeled **Typing speed** — options **Slow** `slow`, **Normal** `normal`, **Expert** `expert` | | `naturalModeSpeed` | `"normal"` | **Hidden unless #10 is ON** |
| 12 | caption | `"Slow ≈ 30 WPM, Normal ≈ 50 WPM, Expert ≈ 80 WPM"` | — | — | Hidden unless #10 ON |

**Group header `"Personalization"`** — saves via `savePersonalizationSettings()`.

| # | Control | Label / description | Key | Default | Dependency |
|---|---|---|---|---|---|
| 1 | switch | **Style detection** — `"Automatically adjust tone based on context"` | `styleDetectionEnabled` | `true` | — |
| 2–5 | four `.menu` dropdowns, labels **Work**, **Email**, **Personal**, **Other** (keys `work`, `email`, `personal`, `other`) | | `personalizationStyles[<key>]` | all `"default"` | **Hidden unless #1 is ON** |

Dropdown options for all four (raw value → displayed via `.capitalized`): `default` → **Default**, `formal` → **Formal**, `casual` → **Casual**, `friendly` → **Friendly**, `professional` → **Professional**.

---

#### **Tab: Polish** — single group, header `"Polish"`; all saves via `savePolishSettings()`

1. `SettingsToggleRow` **`"Enable Polish"`** / `"Refine selected text with AI"` → `polishEnabled`, default **`false`**.
2. **Everything below is hidden (not disabled) unless Enable Polish is ON.**
3. Sub-VStack (spacing 4):
   - `Text("Polish hotkey:")` — `.subheadline`, `.secondary`.
   - Per `polishHotkeyLabels` (default `["Right Control"]`, keycode `62`): `KeyCapView` + minus button (`minus.circle`, red, borderless, tooltip **`"Remove this polish hotkey"`**) shown only when `count > 1`.
   - Button **`"Add Polish Hotkey"`** / **`"Press a key…"`** while capturing, `controlSize .small`. Same capture semantics as the dictation hotkey; writes `polishHotkeyKeyCodes` / `polishHotkeyLabels`.
4. `Divider()`
5. `Text("Polish instructions:")` — `.subheadline`, `.secondary`.
6. One `.switch` Toggle (`controlSize .small`) per key of `polishInstructions`, **sorted alphabetically by key** (`keys.sorted()`), label = the key string itself. Defaults:
   | Instruction (sorted display order) | Default |
   |---|---|
   | `Add structure for readability` | `true` |
   | `Clarify main point` | `false` |
   | `Maintain your tone` | `true` |
   | `Make more concise` | `true` |
   | `Refine phrasing for impact` | `false` |
   | `Reorder for readability` | `true` |
   | `Reword for clarity` | `true` |
7. `Divider()`
8. `SettingsToggleRow` **`"Auto-polish after dictation"`** / `"Automatically polish text after each dictation"` → `autoPolish`, default **`false`**.

---

#### **Tab: Privacy** — single group, header `"Privacy"`; saves via `savePrivacySettings()`

| # | Label / description | Key | Default |
|---|---|---|---|
| 1 | **Screen context (OCR)** — `"Capture screen text for context-aware formatting"` | `useScreenContext` | `false` |
| 2 | **Accessibility context** — `"Use accessibility APIs for better transcription context"` | `useAccessibilityContext` | `true` |
| 3 | **Share anonymous usage data** — `"Help improve Wispr by sharing anonymous statistics"` | `shareUsageData` | `false` |

All three are independent switches; no dependencies.

---

#### **Tab: System** — single group, header `"System"`; saves via `saveSystemSettings()`

| # | Control | Label / description | Key | Default | Side effect |
|---|---|---|---|---|---|
| 1 | switch | **Launch at login** (no description) | `launchAtLogin` | `false` | `updateLaunchAgent()` — writes/removes `~/Library/LaunchAgents/com.wisprlightning.app.plist` with `Label=com.wisprlightning.app`, `ProgramArguments=[<exec path>]`, `RunAtLoad=true`, `KeepAlive=false`. Fallback exec path `/Applications/Wispr Lightning.app/Contents/MacOS/WisprLightning`. |
| 2 | switch | **Show in Dock** | `showInDock` | `false` | `NSApp.setActivationPolicy(.regular / .accessory)` immediately |
| 3 | switch | **Sound effects** | `enableSounds` | `true` | — |
| 4 | switch | **Mute music while dictating** | `muteMusic` | `false` | — |
| — | `Divider()` | | | | |
| 5 | switch | **Verbose logging** — `"Log full server requests and responses to ~/Library/Logs/WisprLightning.log"` | `verboseLogging` | `false` | — |
| — | `Divider()` | | | | |
| 6 | `HStack`: dropdown labeled **Sound pack** + button **`"Preview"`** (`controlSize .small`) | | `selectedSoundPack` | `nil` | Preview: saves, posts `WisprLightningSettingsChanged`, then **200 ms** later posts `WisprPreviewSoundPack` |

Sound pack dropdown options: first **`"Default"`** → tag `nil`; then one entry per `SoundManager.availablePacks()` **excluding the literal `"default"`**, label = pack name `.capitalized`, tag = pack name. Packs are directory names under the bundled `Sounds` resource folder; if the folder is missing, `availablePacks()` returns `["default"]` (so the dropdown shows only `Default`).

Below the GroupBox, outside it: `Divider()` then `Text("Wispr Lightning v1.0.0")` — `.subheadline`, `.tertiary`. (Note: `Constants.clientVersion` is `"1.4.549"` and is *not* what's displayed here — the version string is hardcoded.)

---

### 3.6 Persistence model
All keys above are properties on `AppSettings` (Codable) serialized as **pretty-printed JSON** to `~/Library/Application Support/WisprLightning/settings.json`. JSON key == Swift property name exactly. Every `save()` posts `Notification.Name("WisprLightningSettingsChanged")`. Additional keys not surfaced in this UI: `hotkeyKeyCode` (59), `hotkeyLabel` (`"Left Control"`), `micDeviceName`, `hotkeyPaused` (`false`), and the derived `activePolishInstructions`.

---

## 4. RecordingOverlay.swift (317 lines)

### 4.1 Window
`NSPanel`, initial content rect `120 × 36`, style mask `[.nonactivatingPanel, .fullSizeContentView]`.

| Property | Value |
|---|---|
| `level` | `.floating` |
| `isOpaque` | `false`, `backgroundColor = .clear` |
| `hasShadow` | `true` |
| `isMovableByWindowBackground` | `false` (not draggable) |
| `collectionBehavior` | `[.canJoinAllSpaces, .stationary]` — visible on every Space, ignores Exposé |
| `animationBehavior` | `.utilityWindow` |

**Focus avoidance:** `.nonactivatingPanel` + always shown with `orderFront(nil)` (**never** `makeKeyAndOrderFront`) and the app never calls `NSApp.activate` for it. The panel therefore never becomes key and never steals keyboard focus from the frontmost app — critical, since dictation types into that app. The Retry/Save/✕ buttons are still clickable because AppKit routes mouse events to non-activating panels.

### 4.2 Content hierarchy
`NSVisualEffectView` (material `.popover`, state `.active`, `cornerRadius 18`, `masksToBounds`) fills the panel. Inside, a horizontal `NSStackView` pinned to all edges, `spacing 8`, `edgeInsets = (top: 0, left: 16, bottom: 0, right: 16)`, containing in order:
1. **dot** — `NSView` 10 × 10, `cornerRadius 5` (perfect circle), default fill `systemRed`.
2. **spinner** — `NSProgressIndicator`, `.spinning`, `controlSize .small`, indeterminate, 16 × 16, initially hidden.
3. **mainLabel** — `NSTextField(labelWithString:)`, `Theme.Fonts.body` (13pt), `labelColor`, initial text `"Listening"`.
4. **timeLabel** — body font, `secondaryLabelColor`, initially hidden.
5. **retryButton** — title `"Retry"`, `.rounded` bezel, `controlSize .small`, hidden.
6. **saveButton** — title `"Save"`, `.rounded` bezel, `controlSize .small`, hidden.
7. **dismissButton** — title `"✕"` (U+2715), `.inline` bezel, `isBordered = false`, hidden.

### 4.3 Positioning
Always bottom-center of `NSScreen.main.visibleFrame`:
`x = visibleFrame.midX − width/2`, `y = visibleFrame.minY + 50`. Height stays **36** in every state. `resizePanel(width:)` is a no-op if the width already matches `currentPanelWidth`; `show()` sets `currentPanelWidth = 0` first to force a reposition (this is how the panel re-centers after a wide error state).

### 4.4 States (trigger → visual → result)

| State | Method | Width | Dot | Spinner | Label text | Background tint | Auto-dismiss |
|---|---|---|---|---|---|---|---|
| **Idle/hidden** | `hide()` | — | reset to red, visible | stopped, hidden | — | — | panel `orderOut` |
| **Listening** (hold-to-talk started) | `show()` | **120** | red `#FF3B30`, **pulsing** | hidden | `"Listening"` | none | no |
| **Recording (locked / hands-free)** | `showLocked()` | **120** | `systemGreen`, still pulsing | hidden | `"Recording"` | none | no |
| **Processing** | `showProcessing()` | **145** | hidden, pulse stopped | visible + animating | `"Processing"` | none | no |
| **Retrying** | `showRetrying(attempt:maxAttempts:)` | **175** | hidden | visible + animating | `"Retrying… (N/M)"` (U+2026 ellipsis) | `systemYellow @ 0.20` | no |
| **Error (transient)** | `showError(message:)` | **180** | hidden | hidden | the message | `systemRed @ 0.30` | **3000 ms**, then `hide()` |
| **Error (retryable, no Save)** | `showRetryableError(…, onSave: nil, …)` | **260** | hidden | hidden | the message | `systemRed @ 0.30` | **never** — persists until user acts |
| **Error (retryable, with Save)** | `showRetryableError(…, onSave: non-nil, …)` | **300** | hidden | hidden | the message | `systemRed @ 0.30` | never |
| **Soft time warning** | `showWarning()` | unchanged | unchanged | unchanged | unchanged | `systemYellow @ 0.30` | no |
| **Final time warning** | `showFinalWarning()` | unchanged | unchanged | unchanged | unchanged | `systemRed @ 0.30` | no |
| **Elapsed time visible** | `updateElapsed(_:)` | **200** (on first reveal) | unchanged | unchanged | unchanged + separate time label | unchanged | no |

**There is no "Done"/success state** — the overlay is simply hidden when a dictation completes.

### 4.5 The pulse animation (there is NO waveform / level meter)
**Important for parity:** the app has **no audio-level visualization, no bars, no waveform**. The only motion during recording is a single opacity pulse on the 10pt dot:
- `CABasicAnimation(keyPath: "opacity")`, `fromValue 1.0` → `toValue 0.3`
- `duration 0.6 s`, `autoreverses = true` (so a full cycle = **1.2 s**)
- `repeatCount = .infinity`, timing function `easeInEaseOut`
- Registered under key `"pulse"`; `stopPulsing()` removes it and forces `opacity = 1.0`.

CSS equivalent: `animation: pulse 1.2s ease-in-out infinite; @keyframes pulse { 0%,100% { opacity:1 } 50% { opacity:.3 } }`.

### 4.6 Elapsed-time label
`updateElapsed(seconds:)` **returns immediately for `seconds < 30`** — the timer is invisible for the first 30 seconds. Format: `String(format: "%d:%02d", seconds/60, seconds%60)` (e.g. `0:30`, `9:05`). If `warningState > 0`, `" ⚠️"` (space + U+26A0 U+FE0F) is appended. First time it becomes visible, the panel widens to **200**.

**Warning state machine** — `warningState` is monotonic (0 → 1 → 2), reset to 0 by `show()`, `showLocked()`, `showProcessing()`.
- `showWarning()` no-ops if `warningState >= 1`; sets 1, yellow 30% tint.
- `showFinalWarning()` no-ops if `warningState >= 2`; sets 2, red 30% tint.

**Driving timings** (from `Services/Constants.swift`, applied by `AppDelegate` on a 1 s timer):
- `warningSeconds = 540` (9:00) → `showWarning()`
- `finalWarningSeconds = 570` (9:30) → `showFinalWarning()`
- `maxRecordingSeconds = 600` (10:00) → auto-stop recording

### 4.7 Buttons & callbacks
- **Retry** → `onRetryAction()`.
- **Save** → `onSaveAction()`, then the button title changes to **`"Saved"`** and it becomes disabled (does not re-enable until the next `show()`).
- **✕** → `onDismissAction()`.
- `show()` and `hide()` clear all three callbacks and reset the Save button title to `"Save"` / enabled.

### 4.8 Exact error message strings that appear in the overlay
From `AppDelegate` directly: `"Mic unavailable"`, `"Mic not responding"`, `"Select text to polish"`, `"Timed out"`, `"Recovered unsent recording"`.
From `TranscriptionError.userMessage`: `"Authentication failed — please sign in again"`, `"Connection failed — check your network"`, `"Server error: <detail>"`, `"Request timed out — server did not respond"`, `"No transcription returned"`.
Retryable errors (get Retry/Save/✕): `connectionFailed`, `timeout`, `serverError`. Non-retryable (transient 3 s toast-style): `authFailed`, `emptyResult`.

### 4.9 Prewarm
`prewarm()` builds the panel at app launch (without showing it) so the first hotkey press has no construction latency.

---

## 5. ToastNotification.swift (105 lines)

**⚠️ Dead code in the shipping app.** `AppDelegate` assigns `toastNotification = ToastNotification()` at line 115 but **never calls `show(wordCount:)` anywhere**. Verified by grep across the whole source tree — the only reference to `show(wordCount:)` is its own definition. The port can either skip it or wire it up deliberately.

### Spec (if reproduced)
- Public API: `show(wordCount: Int)` — **the `wordCount` argument is ignored**. It always renders message `"Done"`, SF Symbol `bolt.fill`, tint `.white`, dwell `1.5 s`.
- Panel: `NSPanel`, `[.nonactivatingPanel, .fullSizeContentView]`, `.floating`, transparent, shadowed, `[.canJoinAllSpaces, .stationary]`, `.utilityWindow`.
- **Size:** height **40**; width **120** when `message.count <= 30`, **340** when `> 30`. With the only real message (`"Done"`, 4 chars) it is always **120 × 40**.
- **Position:** identical to the overlay — `x = visibleFrame.midX − width/2`, `y = visibleFrame.minY + 50` (so a toast would sit exactly on top of the recording overlay).
- **Chrome:** `NSVisualEffectView`, material `.popover`, state `.active`, `cornerRadius 12`, masked.
- **Content:** horizontal `NSStackView`, `alignment .centerY`, `spacing 6`, edge insets `(0, 16, 0, 16)`, centered in the effect view via centerX/centerY constraints (not pinned). Contains: `NSImageView` with the symbol at `pointSize 14, weight .medium`, forced to **18 × 18**, tinted white; then a body-font, `labelColor`, center-aligned label.
- **Animation in:** `alphaValue` 0 → 1 over **0.25 s** (`NSAnimationContext`). The comment says "Slide in" but it is a pure fade — no translation.
- **Dwell:** `DispatchQueue.main.asyncAfter(+1.5 s)` → dismiss.
- **Animation out:** `alphaValue` → 0 over **0.30 s**, then `orderOut` and the panel reference is nilled.
- Calling `show` again dismisses any existing toast first (immediately triggering the 0.3 s fade of the old one).
- **Total lifetime:** ~2.05 s.

---

## 6. HistoryWindow.swift (215 lines) — `HistoryView`

Rendered inside the Settings detail pane (**no separate window**; effective size = 860 − 220 sidebar ≈ **640 × 580** content area, resizable). Wrapped in a `NavigationStack`.

### Search
`.searchable(text: $vm.searchQuery, placement: .toolbar)` — a native search field in the window toolbar (default macOS placeholder **"Search"**). Every keystroke calls `vm.refresh()`; empty query → `historyStore.getEntries()`, otherwise `historyStore.search(query:)`.

### Empty state
Centered `VStack(spacing: 8)`, filling the pane:
- SF Symbol `text.badge.minus`, `.system(size: 36)`, `.tertiary`
- `Text("No dictations yet")`, `.title3`, `.secondary`
No action button. **This same empty state also shows when a search returns nothing** (there is no distinct "no results" state in History, unlike Notes/Dictionary).

### List
`.listStyle(.inset(alternatesRowBackgrounds: true))` — zebra striping.

**Grouping & sort:** entries bucketed by date into `"Today"`, `"Yesterday"`, or `DateFormatter("MMM d")` (e.g. `"Mar 4"`, no year). Within a group, entries sort **newest first** (`timestamp >`). Groups sort by their newest entry, **newest group first**. Section header = group title, `.subheadline.weight(.semibold)`, `.secondary`.

**Row** (`VStack(alignment: .leading, spacing: 4)`, `.padding(.vertical, 4)`):
- Metadata `HStack(spacing: 8)` — all `.subheadline`, `.secondary`, separated by `"·"` in `.quaternary`:
  1. time — `DateFormatter.timeStyle = .short` (locale-dependent, e.g. `"3:42 PM"`)
  2. `entry.appName`
  3. `"<duration>s"` — `String(format: "%.1f")`, e.g. `"4.2s"`
  4. `"<n> words"`
  5. `Spacer()`
  6. Copy button — SF Symbol `doc.on.doc`, `.borderless`, tooltip **`"Copy"`**
  7. Delete button — SF Symbol `trash`, `.borderless`, tooltip **`"Delete"`**
- Body text — `entry.formattedText ?? entry.asrText ?? ""`, `.body`, `.primary`, **`lineLimit(2)`** (truncates with ellipsis).

**There is no context menu on history rows** — only the two inline buttons.

### Footer section
A final unlabeled `Section` containing `HStack { Spacer(); Button("Clear All", role: .destructive) }` — right-aligned, `controlSize .small`, red destructive styling. **Rendered only when the list is non-empty.**

### User actions
| Action | Result |
|---|---|
| Type in search | Live filter via `historyStore.search` |
| Click Copy | `NSPasteboard.general.clearContents()` + writes `formattedText ?? asrText ?? ""` as `.string`. **No visual confirmation.** |
| Click Delete (trash) | Modal `NSAlert`, style `.warning`, messageText **`"Delete this entry?"`**, informativeText **`"This action cannot be undone."`**, buttons **`"Delete"`** (default/first) then **`"Cancel"`**. On Delete → `historyStore.deleteEntry(id:)` + refresh. |
| Click Clear All | Modal `NSAlert`, style `.critical`, messageText **`"Clear all history?"`**, informativeText **`"This will delete all transcript entries. This action cannot be undone."`**, buttons **`"Clear All"`** (default/first) then **`"Cancel"`**. On confirm → `historyStore.clearAll()` + refresh. |

---

## 7. NotesView.swift (202 lines)

Rendered in the Settings detail pane (no separate window). Root `VStack(spacing: 0)`.

### Toolbar row
`HStack`, `.padding(.horizontal, 8)`, `.padding(.vertical, 4)`:
1. Custom search box: `HStack { Image(systemName: "magnifyingglass").secondary; TextField("Search notes…", …).textFieldStyle(.plain) }`, `.padding(6)`, background `controlBackgroundColor`, `cornerRadius 6`. Placeholder text is exactly **`"Search notes…"`** (U+2026). Every keystroke → `vm.refresh()`.
2. Button `Label("New Note", systemImage: "plus")` (default bordered size).

### Empty states
**No notes and no search query** — centered `VStack(spacing: 8)`:
- SF Symbol `note.text`, `.system(size: 36)`, `.tertiary`
- `Text("No notes yet")`, `.title3`, `.secondary`
- `Button("Create Note")`, `controlSize .large`

**No results with a query** — centered `VStack(spacing: 4)` with a single `Text("No results for \"<query>\"")`, `.secondary`. (Query is embedded in literal double quotes.)

### List
`List(selection:)`, `.listStyle(.inset(alternatesRowBackgrounds: true))`. Order comes straight from `notesStore.getNotes()` / `.search(query:)` — the view applies no sorting of its own.

**Row** (`VStack(alignment: .leading, spacing: 4)`, `.padding(.vertical, 2)`):
- `HStack`: title — `note.title` or literal **`"Untitled"`** when empty — `.body.weight(.medium)`; `Spacer()`; `note.modifiedAt` formatted with `dateStyle = .short, timeStyle = .short` (e.g. `"3/4/25, 3:42 PM"`), `.caption`, `.tertiary`.
- `note.contentPreview` if non-empty — `.subheadline`, `.secondary`, **`lineLimit(2)`**.

**Context menu (right-click):** `Button("Edit")` · `Divider()` · `Button("Delete", role: .destructive)`. Delete calls `notesStore.softDelete(id:)` **with no confirmation dialog**.

**Selection behavior:** selecting a row (single left-click) sets `selectedNoteId`, whose `onChange` immediately opens the editor sheet and resets `selectedNoteId = nil`. Net effect: **a single click opens the note editor**, and rows never appear persistently selected.

### Note editor sheet (`NoteEditorSheet`)
- Frame **500 × 400**, `.padding(24)`, `VStack(spacing: 8)`.
- `TextField("Title", …)` — `.roundedBorder`, `.title3` font.
- `TextEditor` — `.body` font, `minHeight: 200`, 1pt `separatorColor` border. **No character limit.**
- Footer `HStack`: `Button("Cancel")` with `.keyboardShortcut(.cancelAction)` (**Esc**); `Spacer()`; `Button("Save")` with `.keyboardShortcut(.defaultAction)` (**Return**, rendered as the blue default button). Save is **never disabled** — an empty note can be saved.

### Create flow
`createNote()` → `notesStore.addNote()` returns a new id → refresh → the new note is looked up and the editor sheet opens on it immediately.

---

## 8. DictionaryView.swift (475 lines)

Rendered in the Settings detail pane. Root `VStack(spacing: 0)`.

### Tab switcher
Unlabeled segmented `Picker`, `.padding(8)`, two segments: **`"Vocabulary"`** (tag 0, **default**) and **`"Snippets"`** (tag 1). Tab state is view-local (`@State`), **not persisted**.

**⚠️ Shared search state:** both tabs bind the *same* `vm.searchQuery`, so switching tabs carries the query over, and `refresh()` always recomputes both `vocabularyEntries` and `snippetEntries`.

### 8.1 Vocabulary tab

**Toolbar** `HStack`, `.padding(.horizontal, 8)`, `.padding(.bottom, 4)`:
1. Search box (identical styling to Notes: magnifyingglass + plain TextField, `.padding(6)`, `controlBackgroundColor`, `cornerRadius 6`) with placeholder **`"Search vocabulary…"`**.
2. Button `Label("Add Word", systemImage: "plus")`.

**Empty states**
- No entries, no query: SF Symbol `character.book.closed` at `size 36` `.tertiary`; `Text("No vocabulary words yet")` `.title3` `.secondary`; `Button("Add Word")` `controlSize .large`. `VStack(spacing: 8)`, centered.
- No entries, query present: `Text("No results for \"<query>\"")`, `.secondary`.

**Row (`VocabularyRow`)** — `HStack`, `.padding(.vertical, 2)`. Columns left→right:
1. `VStack(alignment: .leading, spacing: 2)`: `entry.phrase` `.body.weight(.medium)`; optional `entry.replacement` `.subheadline` `.secondary` `lineLimit(1)`.
2. `Spacer()`
3. Optional `entry.source` badge: `.caption`, `padding(.horizontal, 6)`, `padding(.vertical, 2)`, background `Color.accentColor.opacity(0.1)`, `cornerRadius 4`.
4. Usage count, only when `frequencyUsed > 0`: `"<n>x"`, `.caption`, `.secondary`.
5. `entry.modifiedAt` via `DateFormatter(dateStyle: .short)` (date only, e.g. `"3/4/25"`), `.caption`, `.tertiary`.

**Context menu:** `Button("Edit")` · `Divider()` · `Button("Delete", role: .destructive)` → `dictionaryStore.softDelete(id:)`, **no confirmation**.

**List style:** `.inset(alternatesRowBackgrounds: true)`. Order = whatever `getAllVocabulary()` / `searchEntries(query:snippet:false)` returns; **no client-side sort**.

### 8.2 Snippets tab

Same structure. Differences:
- Search placeholder **`"Search snippets…"`**.
- **Three** toolbar items: search box, `Label("Import CSV", systemImage: "square.and.arrow.down")`, `Label("Add Snippet", systemImage: "plus")`.
- Empty state: SF Symbol `text.snippet`, `Text("No snippets yet")`, `Button("Add Snippet")` (`controlSize .large`).
- **Row (`SnippetRow`)**: `HStack` with a left `VStack(spacing: 2)` then `Spacer()`. Phrase is `.body.weight(.medium)` **and colored `.accentColor`**; replacement is `.subheadline` `.secondary` `lineLimit(2)`. **No source badge, no usage count, no date column.**
- Same Edit/Delete context menu.

**CSV import flow:** `NSOpenPanel`, `allowedContentTypes = [.commaSeparatedText]`, single selection. Cancel → nothing. On OK → `dictionaryStore.importCSV(url:)`, refresh, then a modal `NSAlert` with messageText **`"Import Complete"`** and informativeText either `"Imported <n> entries."` or `"Imported <n> entries with <k> errors:\n<first up to 5 errors joined by newline>"`.

### 8.3 Sheets

**`AddVocabularySheet`** — frame **width 380** (height intrinsic), `.padding(24)`, `VStack(spacing: 16)`:
1. `Text("Add Vocabulary Word")` `.title3.weight(.semibold)`
2. `TextField("Word or phrase (max 60 chars)")` `.roundedBorder` — hard-truncated to **60** characters on every change.
3. `TextField("Replacement (optional)")` `.roundedBorder` — hard-truncated to **200** characters.
4. `HStack`: `Button("Cancel")` `.keyboardShortcut(.cancelAction)` / `Spacer()` / `Button("Add")` `.keyboardShortcut(.defaultAction)`, **disabled while the trimmed phrase is empty**. Blank replacement is stored as `nil`.

**`AddSnippetSheet`** — frame **width 420**, `.padding(24)`, `VStack(spacing: 16)`:
1. `Text("Add Snippet")` `.title3.weight(.semibold)`
2. `TextField("Abbreviation (max 60 chars)")` `.roundedBorder`, truncated to **60**.
3. `VStack(alignment: .leading, spacing: 4)`: caption `Text("Expansion")` `.subheadline` `.secondary`; `TextEditor` `.body`, **fixed height 100**, 1pt `separatorColor` border, truncated to **4000** characters.
4. `HStack`: Cancel (Esc) / Spacer / **Add** (Return), **disabled unless both the trimmed abbreviation and the trimmed expansion are non-empty**.

**`EditEntrySheet`** — shared by both tabs; frame width **420 for snippets, 380 for vocabulary**, `.padding(24)`, spacing 16:
1. Title: `"Edit Snippet"` or `"Edit Vocabulary Word"`.
2. `TextField` placeholder `"Abbreviation"` (snippet) or `"Word or phrase"` (vocabulary), truncated to **60**.
3. Snippet → `"Expansion"` caption + 100pt-tall `TextEditor`, limit **4000**. Vocabulary → `TextField("Replacement (optional)")`, limit **200**.
4. `HStack`: `Button("Cancel")` (Esc) / Spacer / **`Button("Save")`** (Return), **disabled while trimmed phrase is empty**. Blank replacement saves as `nil`. Note: unlike the Add sheet, **Save is not gated on the expansion being non-empty for snippets**.

---

## 9. macOS-only APIs and what they do (Windows reimplementation targets)

| Behavior needed | macOS API used (in these UI files) | Windows/Tauri equivalent |
|---|---|---|
| Floating always-on-top status pill that **never takes keyboard focus** so typing continues into the foreground app | `NSPanel` with `.nonactivatingPanel` + `orderFront(nil)` (RecordingOverlay, ToastNotification) | Tauri window: `alwaysOnTop: true`, `focus: false`, `decorations: false`, `skipTaskbar: true`, `transparent: true`; on Win32 additionally `WS_EX_NOACTIVATE` + `WS_EX_TOOLWINDOW` so clicks don't activate |
| Show overlay on all virtual desktops, ignore Exposé | `collectionBehavior = [.canJoinAllSpaces, .stationary]` | Win32 has no direct analog; must re-show per virtual desktop or accept single-desktop behavior |
| Frosted translucent background for overlay/toast | `NSVisualEffectView(material: .popover)` | CSS `backdrop-filter: blur(30px) saturate(180%)` on a transparent Tauri window (Windows 11 Mica/Acrylic via `window-vibrancy` crate) |
| Position at bottom-center of the *usable* screen area (excludes menu bar & Dock) | `NSScreen.main.visibleFrame` | `tauri::Monitor` work area (`SPI_GETWORKAREA`) |
| Theme colors that auto-invert with the system appearance | `NSColor.windowBackgroundColor`, `.secondaryLabelColor`, `.tertiaryLabelColor`, `.controlAccentColor`, `.systemRed/.systemGreen/.systemYellow`, `.controlBackgroundColor`, `.textBackgroundColor`, `.separatorColor` | CSS custom properties + `@media (prefers-color-scheme)`; the user accent color needs `DwmGetColorizationColor` / `UISettings.GetColorValue(Accent)` |
| Dynamic type sizes | `NSFont.preferredFont(forTextStyle:)` | Fixed rem sizes, optionally scaled by the Windows text-size setting |
| All the glyphs (28 distinct SF Symbols) | `Image(systemName:)`, `NSImage(systemSymbolName:)` | Bundle an icon set (Lucide/Phosphor) — **SF Symbols are not licensed for non-Apple platforms** |
| Capturing a raw hotkey while the Settings window is focused, including bare modifier presses | `NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged])`, `event.keyCode`, `event.modifierFlags`, `charactersIgnoringModifiers` | `keydown`/`keyup` in the webview gives `event.code` for modifiers; a **global** low-level keyboard hook (`SetWindowsHookEx(WH_KEYBOARD_LL)`) is needed for the runtime hotkey itself. **Virtual-key codes differ entirely** from the macOS keycodes stored in `hotkeyKeyCodes` — a migration map is required |
| Distinguishing left vs right Control/Option/Command/Shift and the Fn key | macOS keycodes 54–63 | Windows has `VK_LCONTROL`/`VK_RCONTROL`/`VK_LMENU`/`VK_RMENU`/`VK_LSHIFT`/`VK_RSHIFT`/`VK_LWIN`/`VK_RWIN`; **there is no `Fn` virtual key on Windows** |
| Show/hide the app in the Dock at runtime | `NSApp.setActivationPolicy(.regular / .accessory)` | Tauri `skipTaskbar(true/false)` — semantics differ (taskbar ≠ Dock) |
| Start at login | Writing `~/Library/LaunchAgents/com.wisprlightning.app.plist` | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` registry value, or a Startup-folder shortcut, or the `tauri-plugin-autostart` crate |
| Reading the app's own executable path | `Bundle.main.executablePath` | `std::env::current_exe()` |
| Locating bundled resources (`WisprFlowIcon.png`, `Sounds/` packs) | `Bundle.main.path(forResource:ofType:)`, `Bundle.main.url(forResource:withExtension:)` | Tauri `resolve_resource` / `$RESOURCE` asset protocol |
| Copying text to the clipboard (History → Copy) | `NSPasteboard.general.clearContents()` + `setString(_:forType: .string)` | `tauri-plugin-clipboard-manager` |
| Native modal confirmations (delete entry, clear history, import result) | `NSAlert` with `.warning` / `.critical` styles and `runModal()` | `tauri-plugin-dialog` `ask`/`message`, or an in-webview modal (recommended for consistent wording & button order) |
| Native file open dialog restricted to CSV | `NSOpenPanel` with `allowedContentTypes = [.commaSeparatedText]` | `tauri-plugin-dialog` `open` with `filters: [{name:"CSV", extensions:["csv"]}]` |
| Indeterminate spinner in the overlay | `NSProgressIndicator(.spinning)` | CSS keyframe rotation on an SVG arc |
| Dot pulse animation | `CABasicAnimation` on `layer.opacity` | CSS `@keyframes` |
| Cross-fade window opacity | `NSAnimationContext` + `panel.animator().alphaValue` | Tauri `setOpacity` or CSS opacity on the whole body of a transparent window |
| Enumerating microphones with stable UIDs | `AudioRecorder.listInputDevices()` (CoreAudio, called from the view model) | WASAPI `IMMDeviceEnumerator` device IDs (**UID format is completely different — mic selection cannot be migrated, must fall back to System Default**) |
| Split-view settings window with a native sidebar + unified toolbar and per-window frame autosave | `NavigationSplitView`, `NSWindow.toolbarStyle = .unified`, `setFrameAutosaveName("SettingsWindow")` | Hand-rolled HTML flex layout; persist window geometry yourself (`tauri-plugin-window-state`) |
| Browser-based OAuth sign-in | `AuthService.signInWithBrowser()` (NSWorkspace opens the URL) | `tauri-plugin-shell` `open`, plus a loopback/deep-link callback |
| Locale-aware date/time formatting (`.short` styles, `"MMM d"`, Today/Yesterday) | `DateFormatter`, `Calendar.isDateInToday/isDateInYesterday` | `Intl.DateTimeFormat` in the webview or the `chrono` + `icu` crates |

---

## 10. Parity risks on Windows

1. **Hotkey keycodes are not portable.** `settings.json` stores raw macOS keycodes (`59` = Left Control, `62` = Right Control for Polish). Windows virtual-key codes are a different namespace. A settings migration must remap, and the default `Left Control` hold-to-talk conflicts with far more Windows shortcuts than on macOS.
2. **`Fn` (keycode 63) has no Windows equivalent.** Users who bound Fn lose their hotkey; the keycap list must drop that row on Windows.
3. **Non-activating overlay is the hardest single behavior.** If the overlay window ever takes focus, dictated text lands in the overlay instead of the user's app. Requires `WS_EX_NOACTIVATE` and careful handling of the Retry/Save/✕ buttons, which *must* still receive clicks without activating.
4. **No "all Spaces" analog.** The overlay will only appear on the virtual desktop where it was created unless re-created on desktop switch.
5. **SF Symbols cannot ship on Windows.** All 28 glyphs (`gearshape.fill`, `mic.fill`, `sparkles`, `clock.fill`, `character.book.closed.fill`, `note.text`, `hand.raised.fill`, `desktopcomputer`, `person.crop.circle.fill`, `person.crop.circle.badge.questionmark`, `minus.circle`, `xmark.circle.fill`, `arrow.clockwise`, `magnifyingglass`, `plus`, `square.and.arrow.down`, `doc.on.doc`, `trash`, `text.badge.minus`, `text.snippet`, `character.book.closed`, `bolt.fill`, …) need licensed replacements, which will shift row heights subtly.
6. **Semantic colors and accent color.** `controlAccentColor` follows the user's macOS accent; the language chips (`accent @ 12%`), snippet phrases and vocabulary source badges (`accent @ 10%`) all derive from it. Windows accent extraction is possible but the shade differs, so contrast must be re-checked.
7. **`NSVisualEffectView(.popover)` translucency** has no exact Windows twin. Mica/Acrylic look noticeably different, and on Windows 10 there is effectively no good option — plan a solid fallback.
8. **Mic device UIDs won't survive.** Any saved `micDeviceUID` must be treated as invalid on Windows and reset to System Default, and `micDeviceName` shown as a stale hint at most.
9. **Launch-at-login semantics differ.** The LaunchAgent plist writes silently; the Windows Run key may be blocked by policy or flagged by security software, so failures must surface in the UI (the current code swallows all errors with `try?`).
10. **"Show in Dock" is meaningless on Windows.** Either relabel it ("Show in taskbar") or hide the row — but hiding it changes the System tab layout, so decide deliberately.
11. **Fixed pixel heights assume 1x/2x Retina scaling.** The 220pt language list, 100pt snippet editor, 36pt overlay and 500×400 note sheet will need `rem`/`dvh` treatment under Windows' 125%/150% DPI scaling, which macOS never applies fractionally.
12. **Modal `NSAlert` button order is macOS-idiomatic** (destructive default on the *left*/first, Cancel second). Windows convention is the reverse. Matching macOS exactly would feel wrong on Windows; deviating breaks literal parity — pick one and document it.
13. **`ToastNotification` is dead code.** Porting it faithfully means porting something the user never sees. Recommend dropping it or deliberately wiring `show(wordCount:)` on successful dictation — but note the current signature ignores `wordCount` and always says `"Done"`.
14. **No audio level visualization exists.** If the Tauri port adds a waveform (a natural instinct), that is a *deviation*, not parity. The reference UI has exactly one 10pt dot pulsing 1.0↔0.3 over 1.2 s.
15. **The elapsed timer is invisible below 30 s** and the panel jumps 120 → 200 px wide the moment it appears. This mid-recording resize (and the re-centering) is jarring but is the reference behavior.
16. **Shared `searchQuery` between the Dictionary tabs** is arguably a bug (switching tabs keeps the filter) — but it is observable behavior; changing it is a deviation.
17. **Settings are only loaded once per window instance.** Reopening the Settings window reuses the existing `NSWindow` and `SettingsViewModel`, so external edits to `settings.json` are not picked up until relaunch. A Tauri port with a reactive store will naturally *not* reproduce this.
18. **`polishInstructions` renders in `keys.sorted()` order**, i.e. alphabetical by the English instruction string. Any localization would silently reorder the list.
19. **Version string mismatch.** The System tab hardcodes `"Wispr Lightning v1.0.0"` while `Constants.clientVersion` is `"1.4.549"`. Decide which one the port displays.
20. **History search has no distinct "no results" state** — it shows "No dictations yet", which is misleading. Notes and Dictionary *do* distinguish. Reproducing this faithfully means reproducing an inconsistency.
