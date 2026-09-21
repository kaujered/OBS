//! Data structures and pure helpers for the DBF-based `GDI` report.

use std::sync::Arc;

use crate::domain::Mest;

pub(super) const KSMEST_GAUGE_FIELDS: [&str; 12] = [
    "BE22", "BE23", "BE24", "BE25", "BE26", "BE27", "BE29", "BE30", "BE31", "BE32", "BE33", "BE34",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DataSourceChoice {
    #[default]
    Dbf,
    Kots,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub mests: Vec<Mest>,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub test_mode: bool,
    pub debug_mode: bool,
    pub only_last: bool,
    pub include_rejected: bool,
    pub bngkm_source: DataSourceChoice,
    pub hgkm_source: DataSourceChoice,
    pub export_charts: bool,
    /// When `Some`, a second sheet "FILTRED_GDI" is added with rows whose
    /// `date_key` (YYYYMMDD as i64) is >= this value.
    pub filter_date: Option<i64>,
}

#[derive(Debug, Clone)]
pub(super) struct WellRow {
    pub(super) plast: Option<Arc<str>>,
    pub(super) gp: Option<Arc<str>>,
    pub(super) depr: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct KsmestLast {
    pub(super) gauge_values: [Option<f64>; 12],
}

#[derive(Debug, Clone)]
pub(super) struct PlastLast {
    pub(super) tpl_kelvin: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct Stand1Last {
    pub(super) c: Option<f64>,
    pub(super) n: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct Stand2Row {
    pub(super) regime_no: Option<f64>,
    pub(super) washer: Option<f64>,
    pub(super) well: i64,
    pub(super) date_key: i64,
    pub(super) thp: Option<f64>,
    pub(super) flo: Option<f64>,
    pub(super) bhp: Option<f64>,
    pub(super) pst: Option<f64>,
    pub(super) ppl: Option<f64>,
    pub(super) water: Option<f64>,
    pub(super) sand: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct OutputRow {
    pub(super) gp: Option<Arc<str>>,
    pub(super) plast: Option<Arc<str>>,
    pub(super) regime_no: Option<f64>,
    pub(super) washer: Option<f64>,
    pub(super) well: i64,
    pub(super) date_key: i64,
    pub(super) thp: Option<f64>,
    pub(super) flo: Option<f64>,
    pub(super) gauge: Option<f64>,
    pub(super) bhp: Option<f64>,
    pub(super) pst: Option<f64>,
    pub(super) ppl: Option<f64>,
    pub(super) tpl: Option<f64>,
    pub(super) water: Option<f64>,
    pub(super) sand: Option<f64>,
    pub(super) dep: Option<f64>,
    pub(super) max_dep: Option<f64>,
    pub(super) c: Option<f64>,
    pub(super) n: Option<f64>,
    pub(super) max_flo: Option<f64>,
    pub(super) dop_skv_flo: Option<f64>,
    pub(super) dop_skv_flo_percent_5: Option<f64>,
    pub(super) dop_skv_flo_095: Option<f64>,
    pub(super) limit_comment: Option<String>,
    pub(super) jones_a: Option<f64>,
    pub(super) jones_b: Option<f64>,
}

impl super::limits::LimitTarget for OutputRow {
    fn group_key(&self) -> (i64, i64) {
        (self.well, self.date_key)
    }

    fn limit_input(&self) -> super::limits::LimitInput {
        super::limits::LimitInput {
            regime_no: self.regime_no,
            flo: self.flo,
            sand: self.sand,
            dep: self.dep,
            max_dep: self.max_dep,
        }
    }

    fn jones_input(&self) -> super::limits::JonesInput {
        super::limits::JonesInput {
            flo: self.flo,
            thp: self.thp,
            ppl: self.ppl,
            bhp: self.bhp,
        }
    }

    fn apply(&mut self, limits: &super::limits::LimitResult, jones: &super::limits::JonesResult) {
        self.max_flo = limits.max_flo;
        self.dop_skv_flo = limits.dop_skv_flo;
        self.dop_skv_flo_percent_5 = limits.dop_skv_flo_percent_5;
        self.dop_skv_flo_095 = limits.dop_skv_flo_095;
        self.limit_comment = Some(limits.comment.clone());
        self.jones_a = jones.a;
        self.jones_b = jones.b;
    }
}

#[inline]
pub(super) fn mean_non_zero(values: &[Option<f64>]) -> Option<f64> {
    let mut sum = 0.0;
    let mut count = 0usize;
    for value in values.iter().flatten() {
        if *value != 0.0 {
            sum += *value;
            count += 1;
        }
    }
    if count == 0 {
        None
    } else {
        Some(sum / count as f64)
    }
}

#[inline]
pub(super) fn cmp_opt_f64(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}
