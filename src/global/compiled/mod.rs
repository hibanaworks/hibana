//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It keeps the public-facing law small
//! while grouping internal owners by phase: lowering validation, sealed runtime
//! images, and transient materialization glue.

pub(crate) mod images;
pub(crate) mod layout;
pub(crate) mod lowering;
pub(crate) mod materialize;
