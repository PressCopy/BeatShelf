//! Beatshelf core. No UI, no Tauri: everything here runs and is tested on
//! Windows, macOS and Linux in CI.

pub mod audio;
pub mod daw;
pub mod db;
pub mod error;
pub mod inbox;
pub mod cover;
pub mod keys;
pub mod ops;
pub mod parse;
pub mod pathsafe;
pub mod scan;
pub mod template;

pub use error::{Error, Result};
