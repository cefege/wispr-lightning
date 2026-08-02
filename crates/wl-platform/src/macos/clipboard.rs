//! Full-fidelity `NSPasteboard` snapshot and restore.
//!
//! A pasteboard holds *N items*, each declaring *M representations*. Copying a
//! cell out of Numbers puts plain text, RTF, HTML and a private Numbers
//! archive on one item; copying three files puts three items. Saving only the
//! string and writing it back destroys everything else, which is data loss the
//! user did not ask for when they dictated a sentence.
//!
//! `arboard` cannot express this — its vocabulary is text/HTML/image/file-list
//! (see `docs/parity/research-input.md` §4) — so this goes straight to AppKit.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};

/// Every representation of every item that was on the pasteboard.
///
/// Owned plain data rather than `Retained<NSData>` so the snapshot is `Send`
/// and can be handed across the restore timer's thread boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteboardSnapshot {
    items: Vec<Vec<(String, Vec<u8>)>>,
}

impl PasteboardSnapshot {
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn general() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

/// Capture the entire pasteboard.
///
/// Types whose data cannot be read are skipped rather than failing the whole
/// snapshot: promised-file and lazily-provided representations legitimately
/// return nil until someone asks for them, and losing one of those is far
/// better than refusing to dictate.
pub fn snapshot() -> PasteboardSnapshot {
    let pb = general();
    let Some(items) = pb.pasteboardItems() else {
        return PasteboardSnapshot::default();
    };

    let mut saved = Vec::with_capacity(items.len());
    for item in items.iter() {
        let types = item.types();
        let mut reps = Vec::with_capacity(types.len());
        for ty in types.iter() {
            if let Some(data) = item.dataForType(&ty) {
                reps.push((ty.to_string(), data.to_vec()));
            }
        }
        if !reps.is_empty() {
            saved.push(reps);
        }
    }
    PasteboardSnapshot { items: saved }
}

/// Put the snapshot back, replacing whatever is on the pasteboard now.
///
/// Items are written one at a time because `writeObjects:` appends: a single
/// call with all items would still work, but writing individually preserves
/// the original order even when an item fails to serialize.
pub fn restore(snapshot: &PasteboardSnapshot) {
    let pb = general();
    pb.clearContents();
    for reps in &snapshot.items {
        let item = NSPasteboardItem::new();
        for (ty, bytes) in reps {
            let ns_type = NSString::from_str(ty);
            let data = NSData::with_bytes(bytes);
            item.setData_forType(&data, &ns_type);
        }
        let writer: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
            ProtocolObject::from_retained(item);
        pb.writeObjects(&NSArray::from_retained_slice(&[writer]));
    }
}

/// Replace the pasteboard with a single plain-text item.
pub fn set_string(text: &str) -> bool {
    let pb = general();
    pb.clearContents();
    // SAFETY: `NSPasteboardTypeString` is an immortal AppKit string constant.
    let ty = unsafe { objc2_app_kit::NSPasteboardTypeString };
    pb.setString_forType(&NSString::from_str(text), ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The snapshot must survive being moved to the deferred-restore thread,
    /// which is the whole reason it holds owned bytes instead of `NSData`.
    #[test]
    fn a_snapshot_can_be_moved_between_threads() {
        let snap = PasteboardSnapshot {
            items: vec![vec![("public.utf8-plain-text".into(), b"hi".to_vec())]],
        };
        let moved = std::thread::spawn(move || snap.item_count());
        assert_eq!(moved.join().expect("thread panicked"), 1);
    }
}
