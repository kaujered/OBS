//! Data structures shared across the `PPL` pipeline.

use std::sync::Arc;

use crate::domain::Mest;

#[derive(Debug, Clone)]
pub struct Request {
    pub mests: Vec<Mest>,
    pub year: i32,
    pub test_mode: bool,
    pub debug_mode: bool,
    pub only_last: bool,
}

#[derive(Debug, Clone)]
pub(super) struct WellRow {
    pub(super) gp: Option<Arc<str>>,
    pub(super) plast: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub(super) struct SourceRow {
    pub(super) well: i64,
    pub(super) date_yyyymm: i64,
    pub(super) pst: Option<f64>,
    pub(super) ppl: Option<f64>,
    pub(super) zamer: Option<f64>,
}
