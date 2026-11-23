//! Course Package model and loading pipeline.
//!
//! The implementation lives in the shared `cynapse-course` crate so that the
//! Client and Cynapse Studio validate one contract. This module only forwards
//! the public surface, keeping `crate::course::...` import paths stable for
//! existing Client code.
//!
//! Import new code from `crate::course::...` (Client) or `cynapse_course::...`
//! (any other consumer); do not depend on this shim from a new crate.

pub use cynapse_course::*;
