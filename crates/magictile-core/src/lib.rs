//! The MagicTile puzzle model: configuration, puzzle building, state, twisting and persistence.
//!
//! Headless, with no threads or file I/O (callers supply those), so it can be tested directly and
//! kept portable to the web.

pub mod cell;
pub mod config;
pub mod controller;
pub mod group_presentation;
pub mod library;
pub mod loader;
pub mod macros;
pub mod netfmt;
pub mod pants;
pub mod puzzle;
pub mod slice_mask;
pub mod state;
pub mod topology;
pub mod twist;
pub mod twist_data;
pub mod xml;

pub use config::PuzzleConfig;
pub use controller::TwistController;
pub use library::Library;
pub use puzzle::{BuildError, BuildProgress, Puzzle};
pub use twist::SingleTwist;
