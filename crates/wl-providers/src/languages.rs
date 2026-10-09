//! Dictation languages Deepgram accepts, in the Swift app's display order.
//!
//! Order is load-bearing: the picker renders in it, as the Swift implementation
//! did by filtering a `Set` through this master array rather than tracking
//! selection order (ui-spec 3.5, MATRIX SET-046).
//!
//! This table was inherited from a build that fronted a different provider, and
//! carried 104 languages of which Deepgram rejected 48 outright. Every entry
//! here was verified against `/v1/listen` with both models.
//!
//! `nova2` records whether Nova 2 also accepts the language; 18 are Nova 3 only.
//! Nothing here is Nova 2 only. The code is the picker's spelling, not always
//! Deepgram's tag: `deepgram_language_tag` translates `engb`, `dech`, `zhcn`,
//! `zh` and `yue`. `zh` is Traditional here, which is why it maps to `zh-Hant`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    pub nova2: bool,
}

pub const AUTO_DETECT: &str = crate::deepgram::AUTO_DETECT;
pub const LEGACY_MULTI: &str = crate::deepgram::MULTI_SELECT;

pub const LANGUAGES: &[Language] = &[
    Language { code: "en", name: "English", nova2: true },
    Language { code: "engb", name: "English — British", nova2: true },
    Language { code: "zh", name: "Chinese — Traditional (繁體中文)", nova2: true },
    Language { code: "zhcn", name: "Chinese — Simplified (简体中文)", nova2: true },
    Language { code: "de", name: "German (Deutsch)", nova2: true },
    Language { code: "dech", name: "German — Swiss (Deutsch)", nova2: true },
    Language { code: "es", name: "Spanish (Español)", nova2: true },
    Language { code: "ru", name: "Russian (Русский)", nova2: true },
    Language { code: "ko", name: "Korean (한국어)", nova2: true },
    Language { code: "fr", name: "French (Français)", nova2: true },
    Language { code: "ja", name: "Japanese (日本語)", nova2: true },
    Language { code: "pt", name: "Portuguese (Português)", nova2: true },
    Language { code: "tr", name: "Turkish (Türkçe)", nova2: true },
    Language { code: "pl", name: "Polish (Polski)", nova2: true },
    Language { code: "ca", name: "Catalan (Català)", nova2: true },
    Language { code: "nl", name: "Dutch (Nederlands)", nova2: true },
    Language { code: "ar", name: "Arabic (العربية)", nova2: false },
    Language { code: "sv", name: "Swedish (Svenska)", nova2: true },
    Language { code: "it", name: "Italian (Italiano)", nova2: true },
    Language { code: "id", name: "Indonesian (Bahasa)", nova2: true },
    Language { code: "hi", name: "Hindi (हिन्दी)", nova2: true },
    Language { code: "fi", name: "Finnish (Suomi)", nova2: true },
    Language { code: "vi", name: "Vietnamese (Tiếng Việt)", nova2: true },
    Language { code: "he", name: "Hebrew (עברית)", nova2: false },
    Language { code: "uk", name: "Ukrainian (Українська)", nova2: true },
    Language { code: "el", name: "Greek (Ελληνικά)", nova2: true },
    Language { code: "ms", name: "Malay (Bahasa Melayu)", nova2: true },
    Language { code: "cs", name: "Czech (Čeština)", nova2: true },
    Language { code: "ro", name: "Romanian (Română)", nova2: true },
    Language { code: "da", name: "Danish (Dansk)", nova2: true },
    Language { code: "hu", name: "Hungarian (Magyar)", nova2: true },
    Language { code: "ta", name: "Tamil (தமிழ்)", nova2: false },
    Language { code: "no", name: "Norwegian (Norsk)", nova2: true },
    Language { code: "th", name: "Thai (ไทย)", nova2: true },
    Language { code: "ur", name: "Urdu (اردو)", nova2: false },
    Language { code: "hr", name: "Croatian (Hrvatski)", nova2: false },
    Language { code: "bg", name: "Bulgarian (Български)", nova2: true },
    Language { code: "lt", name: "Lithuanian (Lietuvių)", nova2: true },
    Language { code: "sk", name: "Slovak (Slovenčina)", nova2: true },
    Language { code: "te", name: "Telugu (తెలుగు)", nova2: false },
    Language { code: "fa", name: "Persian (فارسی)", nova2: false },
    Language { code: "lv", name: "Latvian (Latviešu)", nova2: true },
    Language { code: "bn", name: "Bengali (বাংলা)", nova2: false },
    Language { code: "sr", name: "Serbian (Српски)", nova2: false },
    Language { code: "sl", name: "Slovenian (Slovenščina)", nova2: false },
    Language { code: "kn", name: "Kannada (ಕನ್ನಡ)", nova2: false },
    Language { code: "et", name: "Estonian (Eesti)", nova2: true },
    Language { code: "mk", name: "Macedonian (Македонски)", nova2: false },
    Language { code: "ne", name: "Nepali (नेपाली)", nova2: false },
    Language { code: "bs", name: "Bosnian (Bosanski)", nova2: false },
    Language { code: "mr", name: "Marathi (मराठी)", nova2: false },
    Language { code: "be", name: "Belarusian (Беларуская)", nova2: false },
    Language { code: "gu", name: "Gujarati (ગુજરાતી)", nova2: false },
    Language { code: "tl", name: "Tagalog", nova2: false },
    Language { code: "yue", name: "Cantonese (粵語)", nova2: true },
];
