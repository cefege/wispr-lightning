//! Cross-layer contract: the on-disk settings object and the frontend's
//! `Settings` interface must describe the same keys.
//!
//! # Why this test exists
//!
//! The two declarations are in different languages and nothing links them.
//! `#[serde(default)]` and the flattened unknown-key map deliberately tolerate
//! future settings, while the frontend casts the IPC payload. Without this
//! test, a misspelled field can save successfully and then appear to forget.
//!
//! # How the two sides are derived
//!
//! The Rust key set comes from serialising `Settings::default()`, not from
//! parsing the struct. The serialized wire format is the actual contract and
//! includes every `#[serde(rename)]` automatically.
//!
//! # It must never skip
//!
//! A missing or unparsable `ipc.ts` is a **failure**, not a reason to pass. A
//! guard that quietly stops guarding is worse than no guard, because it still
//! reads green. If the frontend file moves, re-point the path below; do not
//! delete the assertions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wl_core::settings::Settings;

/// Located from `CARGO_MANIFEST_DIR` so the test does not care which directory
/// `cargo` was invoked from.
fn ipc_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/ipc.ts")
}

/// The frontend IPC source, or a panic explaining what the reader has to fix.
fn ipc_source() -> String {
    let path = ipc_path();
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the frontend IPC bindings at {}: {e}\n\n\
             This is a failure, not a skip. This test is the only thing keeping \
             `wl_core::settings::Settings` and the TypeScript `Settings` interface \
             in agreement. If the file moved, re-point this test; do not delete it.",
            path.display()
        )
    })
}

// ---------------------------------------------------------------------------
// A very small TypeScript reader
//
// Enough to read a flat interface body: comments, optional markers and nested
// type literals. Not a parser — it deliberately understands nothing it does
// not have to.
// ---------------------------------------------------------------------------

/// `src` with `//` and `/* */` comments blanked out, string literals intact.
///
/// Newlines inside comments are preserved so that blanking a doc comment
/// cannot silently weld two field declarations together.
fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        match c {
            '/' if next == Some('/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if next == Some('*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
                i = (i + 2).min(chars.len());
            }
            '"' | '\'' | '`' => {
                let quote = c;
                out.push(c);
                i += 1;
                while i < chars.len() {
                    let s = chars[i];
                    out.push(s);
                    i += 1;
                    if s == '\\' {
                        if let Some(&escaped) = chars.get(i) {
                            out.push(escaped);
                            i += 1;
                        }
                        continue;
                    }
                    if s == quote {
                        break;
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }

    out
}

/// The `{ ... }` body of `export interface <name>`, comments already stripped.
///
/// The name is matched whole, so `Settings` does not also match
/// `SettingsStatus`.
fn interface_body(source: &str, name: &str) -> String {
    let src = strip_comments(source);
    let needle = format!("interface {name}");
    let mut search_from = 0;

    let open = loop {
        let at = src[search_from..]
            .find(&needle)
            .map(|rel| search_from + rel)
            .unwrap_or_else(|| {
                panic!(
                    "no `export interface {name}` in {}.\n\n\
                     Either it was renamed — in which case this test is no longer \
                     guarding anything and must be re-pointed — or the frontend's \
                     settings type is gone entirely.",
                    ipc_path().display()
                )
            });

        // Guard against `interface SettingsStatus`: the name must end where the
        // needle does, and the next thing must be the body's `{`.
        let rest = &src[at + needle.len()..];
        let brace = rest
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .filter(|&(_, c)| c == '{');
        if rest.starts_with(char::is_whitespace) {
            if let Some((off, _)) = brace {
                break at + needle.len() + off;
            }
        }
        search_from = at + needle.len();
    };

    let bytes = src.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return src[open + 1..open + offset].to_string();
                }
            }
            _ => {}
        }
    }

    panic!(
        "the `{name}` interface in {} is never closed; this test's reader is \
         confused or the file is truncated.",
        ipc_path().display()
    )
}

/// The declared field names of an interface body, top level only.
///
/// Nesting counts `{`, `[` and `(`, so field names inside an inline object
/// literal, a tuple or a function signature are not mistaken for fields of the
/// interface itself.
fn interface_fields(body: &str) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    let mut depth = 0usize;
    let mut segment = String::new();

    let flush = |segment: &mut String, fields: &mut BTreeSet<String>| {
        if let Some(name) = field_name(segment) {
            fields.insert(name);
        }
        segment.clear();
    };

    for c in body.chars() {
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                segment.push(c);
            }
            '}' | ']' | ')' => {
                depth = depth.saturating_sub(1);
                segment.push(c);
            }
            ';' | ',' | '\n' if depth == 0 => flush(&mut segment, &mut fields),
            _ => segment.push(c),
        }
    }
    flush(&mut segment, &mut fields);

    fields
}

/// `foo?: Bar` -> `foo`. `None` for anything that is not a plain declaration,
/// such as an index signature or a stray fragment of a wrapped type.
fn field_name(segment: &str) -> Option<String> {
    let mut depth = 0usize;
    let colon = segment.char_indices().find(|&(_, c)| match c {
        '{' | '[' | '(' | '<' => {
            depth += 1;
            false
        }
        '}' | ']' | ')' | '>' => {
            depth = depth.saturating_sub(1);
            false
        }
        ':' => depth == 0,
        _ => false,
    })?;

    let name = segment[..colon.0].trim().trim_end_matches('?').trim();
    let mut chars = name.chars();
    let head_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
    let tail_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');

    (head_ok && tail_ok).then(|| name.to_string())
}

/// The keys `Settings::default()` actually writes to `settings.json`.
fn rust_settings_keys() -> BTreeSet<String> {
    let defaults = Settings::default();

    // `Settings` carries `#[serde(flatten)] unknown: BTreeMap<..>`, which
    // splices its entries in as top-level keys. It is empty in `default()`, so
    // it contributes none and the key set below is exactly the declared
    // fields. Asserted rather than assumed: a non-empty default would make
    // this test compare the frontend against a moving target.
    assert!(
        defaults.unknown.is_empty(),
        "Settings::default().unknown is no longer empty, so `#[serde(flatten)]` \
         is splicing extra top-level keys into the wire format that no frontend \
         interface could ever declare: {:?}",
        defaults.unknown.keys().collect::<Vec<_>>()
    );

    let value = serde_json::to_value(&defaults).expect("Settings must serialise");
    value
        .as_object()
        .expect("Settings must serialise to a JSON object")
        .keys()
        .cloned()
        .collect()
}

fn bullets(keys: &BTreeSet<String>) -> String {
    keys.iter()
        .map(|k| format!("\n    - {k}"))
        .collect::<String>()
}

#[test]
fn the_frontend_settings_interface_declares_exactly_the_keys_the_backend_writes() {
    let rust = rust_settings_keys();
    let ts = interface_fields(&interface_body(&ipc_source(), "Settings"));

    assert!(
        !ts.is_empty(),
        "read zero fields out of the `Settings` interface in {}. The interface \
         reader in this test is broken — fix it, because as written it would \
         pass against an empty frontend type.",
        ipc_path().display()
    );

    let missing_in_ts: BTreeSet<String> = rust.difference(&ts).cloned().collect();
    let missing_in_rust: BTreeSet<String> = ts.difference(&rust).cloned().collect();

    if missing_in_ts.is_empty() && missing_in_rust.is_empty() {
        return;
    }

    let mut report =
        String::from("the settings wire format and the frontend `Settings` interface disagree.\n");

    if !missing_in_ts.is_empty() {
        report.push_str(&format!(
            "\n  {} key(s) the backend writes that the frontend does not declare:{}\n\
             \n  The settings pane cannot read or write these at all. Add them to \
             `export interface Settings` in ui/src/lib/ipc.ts.\n",
            missing_in_ts.len(),
            bullets(&missing_in_ts),
        ));
    }

    if !missing_in_rust.is_empty() {
        report.push_str(&format!(
            "\n  {} key(s) the frontend declares that the backend does not write:{}\n\
             \n  Anything the pane stores under these is discarded by `serde` on the \
             next load, silently — this is exactly the `provider` bug. Check the \
             `#[serde(rename)]` on the field in crates/wl-core/src/settings.rs (the \
             Rust field name is NOT always the JSON key), then either correct the \
             name in ui/src/lib/ipc.ts or remove it.\n",
            missing_in_rust.len(),
            bullets(&missing_in_rust),
        ));
    }

    report.push_str(&format!(
        "\n  {} backend key(s), {} frontend key(s).",
        rust.len(),
        ts.len()
    ));

    panic!("{report}");
}

/// The interface reader above is the load-bearing half of this file: if it
/// under-reads it produces confusing false failures, and if it over-reads —
/// picking up the fields of a nested type literal — it can invent agreement
/// that is not there. This pins its behaviour against a synthetic source so
/// that a bug in the reader is a failure here rather than a mystery there.
#[test]
fn the_interface_reader_handles_the_shapes_that_actually_occur() {
    let source = r#"
export interface SettingsStatus {
  decoy: boolean;
}

export interface Settings {
  /** A doc comment, with an `interface Settings {` inside it for good measure. */
  plain: string;
  // A line comment.
  optional?: string | null;
  snake_case_key: Hotkey[];
  nested: { notAField: string; alsoNotAField?: number };
  listOfObjects: Array<{ stillNotAField: string }>;
  quoted: "a" | "b";
}
"#;

    let fields = interface_fields(&interface_body(source, "Settings"));

    assert_eq!(
        fields.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "listOfObjects",
            "nested",
            "optional",
            "plain",
            "quoted",
            "snake_case_key",
        ],
        "the reader must take every top-level field once, and nothing from a \
         nested type literal or a neighbouring interface"
    );
}
