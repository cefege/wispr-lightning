//! Short audio cues for recording start and stop.

use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
    Error,
}

pub trait SoundPlayer: Send + Sync {
    /// Play a cue. Fire-and-forget: must return immediately and must not queue
    /// behind a previous cue, because start and stop can fire back to back.
    fn play(&self, cue: Cue);

    /// Switch sound packs. `None` restores the built-in set.
    fn set_pack(&self, pack: Option<&str>) -> Result<()>;

    /// Packs discovered on disk, for the settings picker.
    fn available_packs(&self) -> Vec<String>;

    fn set_enabled(&self, enabled: bool);
}
