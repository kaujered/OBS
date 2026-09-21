//! Business modules.
//!
//! Every mode follows the same high-level contract:
//! - accept a typed `Request`;
//! - resolve required paths via `crate::paths`;
//! - read DBF/XLSX sources;
//! - transform rows into report-specific structures;
//! - write one or more output `.xlsx` files.

pub mod gdi;
pub mod gdm;
pub mod ppl;
pub mod ss;
pub mod telemetry;
pub mod ved;
