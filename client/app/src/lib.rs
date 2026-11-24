//! Cynapse Client GPUI library.
//!
//! The binary is a thin entry point; keeping the views and platform layer in a
//! library target lets headless GPUI tests construct and drive production views.

pub mod assets;
pub mod audio;
pub mod caption_metrics;
pub mod platform;
pub mod settings;
pub mod view_model;
pub mod views;
