//! Crate-private lowering owners for the unified compiled pipeline.
//!
//! This module is intentionally internal. It keeps the public-facing law small
//! while grouping internal owners by responsibility: lowering validation,
//! sealed runtime images, and resident descriptor views. Attach never uses transient
//! materialization glue.

pub(crate) mod images;
pub(crate) mod lowering;
