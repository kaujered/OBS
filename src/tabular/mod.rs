//! Shared low-level converters for DBF and XLSX tabular data.
//!
//! These helpers intentionally stay small and format-focused so business modules
//! can reuse them without inheriting domain assumptions.

pub(crate) mod cache;
pub(crate) mod dbf;
pub(crate) mod number;
pub(crate) mod report_xlsx;
pub(crate) mod sheet;
pub(crate) mod wells;
pub(crate) mod xlsx;
