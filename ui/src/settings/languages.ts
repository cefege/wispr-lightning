/**
 * Dictation languages Deepgram actually accepts, in the Swift app's display
 * order.
 *
 * Order is load-bearing: the picker renders in it, as the Swift implementation
 * did by filtering a `Set` through this master array rather than tracking
 * selection order (ui-spec 3.5, MATRIX SET-046).
 *
 * This table was inherited from a build that fronted a different provider, and
 * carried 104 languages of which Deepgram rejected 48 outright -- selecting one
 * produced an HTTP 400 at dictation time, with nothing in the UI to warn the
 * user. Every entry below was verified against `/v1/listen` with both models.
 *
 * `nova2` records whether Nova 2 also accepts the language; 18 of these are
 * Nova 3 only, so the picker filters on the selected model. Nothing here is
 * Nova 2 only.
 *
 * The code is the picker's own spelling, not always Deepgram's tag --
 * `deepgram_language_tag` in `crates/wl-providers/src/deepgram.rs` translates
 * `engb`, `dech`, `zhcn`, `zh` and `yue`. That function and this table must
 * agree; `zh` is Traditional here, which is the only reason it maps to
 * `zh-Hant` there.
 */

export interface Language {
  code: string;
  name: string;
  flag: string;
  /** Nova 2 accepts this language too. Nova 3 accepts every entry here. */
  nova2: boolean;
}

export const LANGUAGES: readonly Language[] = [
  { code: "en", name: "English", flag: "🇺🇸", nova2: true },
  { code: "engb", name: "English — British", flag: "🇬🇧", nova2: true },
  { code: "zh", name: "Chinese — Traditional (繁體中文)", flag: "🇹🇼", nova2: true },
  { code: "zhcn", name: "Chinese — Simplified (简体中文)", flag: "🇨🇳", nova2: true },
  { code: "de", name: "German (Deutsch)", flag: "🇩🇪", nova2: true },
  { code: "dech", name: "German — Swiss (Deutsch)", flag: "🇨🇭", nova2: true },
  { code: "es", name: "Spanish (Español)", flag: "🇪🇸", nova2: true },
  { code: "ru", name: "Russian (Русский)", flag: "🇷🇺", nova2: true },
  { code: "ko", name: "Korean (한국어)", flag: "🇰🇷", nova2: true },
  { code: "fr", name: "French (Français)", flag: "🇫🇷", nova2: true },
  { code: "ja", name: "Japanese (日本語)", flag: "🇯🇵", nova2: true },
  { code: "pt", name: "Portuguese (Português)", flag: "🇧🇷", nova2: true },
  { code: "tr", name: "Turkish (Türkçe)", flag: "🇹🇷", nova2: true },
  { code: "pl", name: "Polish (Polski)", flag: "🇵🇱", nova2: true },
  { code: "ca", name: "Catalan (Català)", flag: "🇪🇸", nova2: true },
  { code: "nl", name: "Dutch (Nederlands)", flag: "🇳🇱", nova2: true },
  { code: "ar", name: "Arabic (العربية)", flag: "🇸🇦", nova2: false },
  { code: "sv", name: "Swedish (Svenska)", flag: "🇸🇪", nova2: true },
  { code: "it", name: "Italian (Italiano)", flag: "🇮🇹", nova2: true },
  { code: "id", name: "Indonesian (Bahasa)", flag: "🇮🇩", nova2: true },
  { code: "hi", name: "Hindi (हिन्दी)", flag: "🇮🇳", nova2: true },
  { code: "fi", name: "Finnish (Suomi)", flag: "🇫🇮", nova2: true },
  { code: "vi", name: "Vietnamese (Tiếng Việt)", flag: "🇻🇳", nova2: true },
  { code: "he", name: "Hebrew (עברית)", flag: "🇮🇱", nova2: false },
  { code: "uk", name: "Ukrainian (Українська)", flag: "🇺🇦", nova2: true },
  { code: "el", name: "Greek (Ελληνικά)", flag: "🇬🇷", nova2: true },
  { code: "ms", name: "Malay (Bahasa Melayu)", flag: "🇲🇾", nova2: true },
  { code: "cs", name: "Czech (Čeština)", flag: "🇨🇿", nova2: true },
  { code: "ro", name: "Romanian (Română)", flag: "🇷🇴", nova2: true },
  { code: "da", name: "Danish (Dansk)", flag: "🇩🇰", nova2: true },
  { code: "hu", name: "Hungarian (Magyar)", flag: "🇭🇺", nova2: true },
  { code: "ta", name: "Tamil (தமிழ்)", flag: "🇮🇳", nova2: false },
  { code: "no", name: "Norwegian (Norsk)", flag: "🇳🇴", nova2: true },
  { code: "th", name: "Thai (ไทย)", flag: "🇹🇭", nova2: true },
  { code: "ur", name: "Urdu (اردو)", flag: "🇵🇰", nova2: false },
  { code: "hr", name: "Croatian (Hrvatski)", flag: "🇭🇷", nova2: false },
  { code: "bg", name: "Bulgarian (Български)", flag: "🇧🇬", nova2: true },
  { code: "lt", name: "Lithuanian (Lietuvių)", flag: "🇱🇹", nova2: true },
  { code: "sk", name: "Slovak (Slovenčina)", flag: "🇸🇰", nova2: true },
  { code: "te", name: "Telugu (తెలుగు)", flag: "🇮🇳", nova2: false },
  { code: "fa", name: "Persian (فارسی)", flag: "🇮🇷", nova2: false },
  { code: "lv", name: "Latvian (Latviešu)", flag: "🇱🇻", nova2: true },
  { code: "bn", name: "Bengali (বাংলা)", flag: "🇧🇩", nova2: false },
  { code: "sr", name: "Serbian (Српски)", flag: "🇷🇸", nova2: false },
  { code: "sl", name: "Slovenian (Slovenščina)", flag: "🇸🇮", nova2: false },
  { code: "kn", name: "Kannada (ಕನ್ನಡ)", flag: "🇮🇳", nova2: false },
  { code: "et", name: "Estonian (Eesti)", flag: "🇪🇪", nova2: true },
  { code: "mk", name: "Macedonian (Македонски)", flag: "🇲🇰", nova2: false },
  { code: "ne", name: "Nepali (नेपाली)", flag: "🇳🇵", nova2: false },
  { code: "bs", name: "Bosnian (Bosanski)", flag: "🇧🇦", nova2: false },
  { code: "mr", name: "Marathi (मराठी)", flag: "🇮🇳", nova2: false },
  { code: "be", name: "Belarusian (Беларуская)", flag: "🇧🇾", nova2: false },
  { code: "gu", name: "Gujarati (ગુજરાતી)", flag: "🇮🇳", nova2: false },
  { code: "tl", name: "Tagalog", flag: "🇵🇭", nova2: false },
  { code: "yue", name: "Cantonese (粵語)", flag: "🇭🇰", nova2: true },
];
