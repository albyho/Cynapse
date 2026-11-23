//! Declarative GPUI views.
//!
//! - `main`: main-window composition, toolbar, dialogs, and reset overlay host.
//! - `challenge_panel`: active-challenge instructions, hint, status, and actions.
//! - `course_tree`: Course Tree rendering and per-Unit actions.
//! - `course_import_dialog`: URL entry and overwrite choice for network courses.
//! - `busy_overlay`: shared full-window busy layer for reset and Start Learning.
//! - `settings`: settings form and caption appearance controls.
//! - `caption`: caption overlay window.
//! - `color`: shared six-digit hex color parsing for caption controls.
//!
//! GPUI component and test idioms live in
//! `docs/research/2026-11-13-gpui-ui-notes.md`.

pub mod busy_overlay;
pub mod caption;
pub mod challenge_panel;
pub(crate) mod color;
pub mod course_import_dialog;
pub mod course_tree;
pub mod main;
pub mod settings;
