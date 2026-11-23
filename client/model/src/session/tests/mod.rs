//! SessionService integration tests.
//!
//! Shared fake Adapter, course writer, and wait helpers live in `support.rs`.
//! Test modules are grouped by the behavior they exercise.

use super::*;

mod adapter_loss;
mod adapterless;
mod challenge;
mod jupyterlab;
mod learning;
mod lifecycle;
mod model;
mod support;
