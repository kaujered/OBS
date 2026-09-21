//! XLSX rendering for the DBF-based `GDI` report: the shared «gdi-like»
//! writer does the layout, this module only maps rows onto it.

use std::path::Path;

use anyhow::Result;

use crate::tabular::report_xlsx::{
    GdiChartRow, GdiLikeCells, GdiLikeRow, RegimeCell, write_gdi_like_workbook,
};

use super::data::OutputRow;

pub(super) fn write_workbook(
    output_path: &Path,
    rows: &[OutputRow],
    export_charts: bool,
    filter_date: Option<i64>,
) -> Result<()> {
    write_gdi_like_workbook(output_path, rows, export_charts, filter_date)
}

impl GdiLikeRow for OutputRow {
    fn date_key(&self) -> i64 {
        self.date_key
    }

    fn cells(&self) -> GdiLikeCells<'_> {
        GdiLikeCells {
            gp: self.gp.as_deref(),
            plast: self.plast.as_deref(),
            regime_no: RegimeCell::Number(self.regime_no),
            washer: self.washer,
            date_text: format!("{:08}", self.date_key),
            liquid_ratio: self.water.map(|value| value / 1_000_000_000.0),
            gauge: self.gauge,
            tpl: self.tpl,
            water: self.water,
            sand: self.sand,
            max_dep: self.max_dep,
            c: self.c,
            n: self.n,
            max_flo: self.max_flo,
            dop_skv_flo: self.dop_skv_flo,
            dop_skv_flo_percent_5: self.dop_skv_flo_percent_5,
            dop_skv_flo_095: self.dop_skv_flo_095,
            limit_comment: self.limit_comment.as_deref(),
        }
    }
}

impl GdiChartRow for OutputRow {
    fn well(&self) -> i64 {
        self.well
    }

    fn flo(&self) -> Option<f64> {
        self.flo
    }

    fn thp(&self) -> Option<f64> {
        self.thp
    }

    fn bhp(&self) -> Option<f64> {
        self.bhp
    }

    fn dep(&self) -> Option<f64> {
        self.dep
    }

    fn pst(&self) -> Option<f64> {
        self.pst
    }

    fn ppl(&self) -> Option<f64> {
        self.ppl
    }

    fn jones_a(&self) -> Option<f64> {
        self.jones_a
    }

    fn jones_b(&self) -> Option<f64> {
        self.jones_b
    }
}
