//! The shell-agnostic core of Wispr Lightning: the dictation pipeline, the
//! spool, the overlay and tray models, settings state, and the IPC operations.
//! Nothing here names a windowing toolkit; each app supplies a [`host::Host`].

pub mod host;
pub mod logging;
pub mod ops;
pub mod overlay_geometry;
pub mod pipeline;
pub mod spool;
pub mod state;
pub mod tray_model;
pub mod ui;
