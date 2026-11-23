//! Course Package model and loading pipeline.
//!
//! Public API is re-exported at this root (`pub use model::*`,
//! `pub use loader::load_package`); import from `crate::course::...`.
//!
//! Submodules:
//! - `model` — manifest/node/step types, serde helpers, inheritance defaults
//! - `loader` — `load_package` pipeline and course tree building
//! - `steps` — step loading and Action file resolution/validation
//! - `methods` — `impl Course` public API
//! - `validation` — package/workspace/requirements validation
//! - `actions` — Action type definitions
//! - `action_narration` — per-action narration model
//! - `audio_asset` — audio asset resolution
//! - `caption_markup` — caption emphasis markup scanner: rendering runs,
//!   validation, and visible text from one traversal
//! - `check` — `PackageReport` validity checks shared by loader and consumers
//! - `diagnostic` — structured authoring diagnostics shared by every consumer
//! - `edit_ops` — pure text-edit semantics shared with the Client harness
//! - `tests` — white-box tests (index in tests/mod.rs)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_WORKSPACE: &str = "workspace";

mod action_narration;
mod actions;
mod audio_asset;
mod caption_markup;
mod check;
mod diagnostic;
mod edit_ops;
mod file_ops;

pub use actions::*;
pub use caption_markup::{
    CaptionSegment, caption_segments, narration_clip_speech_text, validate_caption_markup,
    visible_caption_text,
};
pub use check::{
    PackageLoad, PackageReport, check_course, check_package, load_package_with_report,
};
pub use diagnostic::{Diagnostic, Severity};
pub use edit_ops::*;
pub use file_ops::*;

use action_narration::{
    load_action_narration_phase, load_unit_audio_assets, take_action_narrations,
};
mod loader;
mod methods;
mod model;
mod steps;
mod validation;

pub use loader::load_package;
pub use model::*;

#[cfg(test)]
mod tests;
