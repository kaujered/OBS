//! Shared XLSX writer helpers used by report modules.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use rust_xlsxwriter::{
    Chart, ChartFormat, ChartLine, ChartMarker, ChartMarkerType, ChartSolidFill, ChartTrendline,
    ChartTrendlineType, ChartType, Color, Format, FormatAlign, FormatBorder, Workbook, Worksheet,
    XlsxError,
};

pub(crate) const GDI_LIKE_COLUMN_COUNT: usize = 34;
pub(crate) const GDI_LIKE_HEADERS: [&str; GDI_LIKE_COLUMN_COUNT] = [
    "gp",
    "plast",
    "№R",
    "шайба",
    "well",
    "date",
    "thp",
    "tht",
    "flo",
    "wfr",
    "gfr",
    "alq",
    "gauge",
    "bhp",
    "temperature",
    "weight",
    "comment",
    "use",
    "pst",
    "ppl",
    "tpl",
    "WATER",
    "SAND",
    "DEP",
    "max DEP",
    "C",
    "n",
    "max_flo",
    "dop_skv_flo",
    "dop_skv_flo_%5",
    "dop_skv_flo_095",
    "limit",
    "a",
    "b",
];
pub(crate) const GDI_LIKE_HIDDEN_COLUMNS: [u16; 9] = [7, 10, 11, 14, 15, 16, 17, 28, 30];
const GDI_CHARTS_SHEET_NAME: &str = "ГРАФИКИ";
const BAR_PER_KGF_CM2: f64 = 0.980665;

pub(crate) struct GdiLikeFormats {
    pub(crate) header: Format,
    pub(crate) header_well: Format,
    pub(crate) base_green: Format,
    pub(crate) top_green: Format,
    pub(crate) base_red: Format,
    pub(crate) top_red: Format,
    pub(crate) base_orange: Format,
    pub(crate) top_orange: Format,
    pub(crate) top_only: Format,
}

pub(crate) struct SsFormats {
    pub(crate) header: Format,
    pub(crate) row_grey: Format,
    pub(crate) row_top: Format,
    pub(crate) row_grey_top: Format,
}

pub(crate) trait GdiChartRow {
    fn well(&self) -> i64;
    fn flo(&self) -> Option<f64>;
    fn thp(&self) -> Option<f64>;
    fn bhp(&self) -> Option<f64>;
    fn dep(&self) -> Option<f64>;
    fn pst(&self) -> Option<f64>;
    fn ppl(&self) -> Option<f64>;
    fn jones_a(&self) -> Option<f64>;
    fn jones_b(&self) -> Option<f64>;
}

#[derive(Clone, Copy, Default)]
pub(crate) struct GdiLikeAlertInputs {
    pub(crate) thp: Option<f64>,
    pub(crate) bhp: Option<f64>,
    pub(crate) ppl: Option<f64>,
    pub(crate) pst: Option<f64>,
    pub(crate) dep: Option<f64>,
    pub(crate) max_dep: Option<f64>,
    pub(crate) sand: Option<f64>,
}

#[derive(Clone, Copy)]
pub(crate) struct AlertColumns<const N: usize> {
    pub(crate) red: [bool; N],
    pub(crate) orange: [bool; N],
}

impl<const N: usize> Default for AlertColumns<N> {
    fn default() -> Self {
        Self {
            red: [false; N],
            orange: [false; N],
        }
    }
}

impl<const N: usize> AlertColumns<N> {
    pub(crate) fn mark_red(&mut self, columns: &[usize]) {
        for &column in columns {
            self.red[column] = true;
        }
    }

    pub(crate) fn mark_orange(&mut self, columns: &[usize]) {
        for &column in columns {
            self.orange[column] = true;
        }
    }

    pub(crate) fn has_red(&self) -> bool {
        self.red.iter().any(|flag| *flag)
    }
}

pub(crate) fn gdi_like_formats() -> GdiLikeFormats {
    let header = Format::new()
        .set_text_wrap()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_border(FormatBorder::Thin);
    let header_well = Format::new()
        .set_text_wrap()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter)
        .set_border(FormatBorder::Thin)
        .set_background_color(Color::RGB(0xD7E3BC));

    GdiLikeFormats {
        header,
        header_well,
        base_green: Format::new().set_background_color(Color::RGB(0xD7E3BC)),
        top_green: Format::new()
            .set_background_color(Color::RGB(0xD7E3BC))
            .set_border_top(FormatBorder::Thick),
        base_red: Format::new().set_background_color(Color::RGB(0xFF9F9F)),
        top_red: Format::new()
            .set_background_color(Color::RGB(0xFF9F9F))
            .set_border_top(FormatBorder::Thick),
        base_orange: Format::new().set_background_color(Color::RGB(0xFFCF9F)),
        top_orange: Format::new()
            .set_background_color(Color::RGB(0xFFCF9F))
            .set_border_top(FormatBorder::Thick),
        top_only: Format::new().set_border_top(FormatBorder::Thick),
    }
}

pub(crate) fn ss_formats() -> SsFormats {
    SsFormats {
        header: Format::new()
            .set_bold()
            .set_text_wrap()
            .set_align(FormatAlign::Center)
            .set_align(FormatAlign::VerticalCenter)
            .set_border(FormatBorder::Thin),
        row_grey: Format::new().set_background_color(Color::RGB(0xCACACA)),
        row_top: Format::new().set_border_top(FormatBorder::Thick),
        row_grey_top: Format::new()
            .set_background_color(Color::RGB(0xCACACA))
            .set_border_top(FormatBorder::Thick),
    }
}

pub(crate) fn configure_gdi_like_sheet(
    workbook: &mut Workbook,
) -> Result<&mut Worksheet, XlsxError> {
    configure_gdi_sheet_named(workbook, "FULL__GDI")
}

pub(crate) fn configure_gdi_filtered_sheet(
    workbook: &mut Workbook,
) -> Result<&mut Worksheet, XlsxError> {
    configure_gdi_sheet_named(workbook, "FILTRED_GDI")
}

fn configure_gdi_sheet_named<'a>(
    workbook: &'a mut Workbook,
    name: &str,
) -> Result<&'a mut Worksheet, XlsxError> {
    let worksheet = workbook.add_worksheet();
    worksheet.set_name(name)?;
    worksheet.set_freeze_panes(1, 0)?;
    worksheet.set_column_width(5, 13.0)?;
    worksheet.set_column_width(31, 33.7)?;
    for hidden_col in GDI_LIKE_HIDDEN_COLUMNS {
        worksheet.set_column_hidden(hidden_col)?;
    }
    Ok(worksheet)
}

pub(crate) fn add_gdi_charts_sheet<R>(workbook: &mut Workbook, rows: &[R]) -> Result<(), XlsxError>
where
    R: GdiChartRow,
{
    let worksheet = workbook.add_worksheet();
    worksheet.set_name(GDI_CHARTS_SHEET_NAME)?;
    worksheet.set_zoom(80);

    let mut rows_by_well = BTreeMap::<i64, Vec<&R>>::new();
    for row in rows {
        rows_by_well.entry(row.well()).or_default().push(row);
    }

    let mut data_row = 0_u32;
    for (well_index, (well, well_rows)) in rows_by_well.into_iter().enumerate() {
        let chart_row = (well_index as u32) * 21;
        for (metric_index, metric) in ChartMetric::ALL.iter().copied().enumerate() {
            let chart_col = (metric_index as u16) * 10;
            let helper_col = 40 + (metric_index as u16) * 4;
            let ranges = write_chart_helper_data(
                worksheet,
                well_rows.as_slice(),
                metric,
                helper_col,
                &mut data_row,
            )?;
            // Skip charts that would have no series (e.g. wells with all-None flo).
            let would_have_series = ranges.actual_last_row.is_some()
                || ranges
                    .fit_last_row
                    .is_some_and(|last| last.saturating_sub(ranges.fit_first_row) + 1 >= 2);
            if !would_have_series {
                continue;
            }
            let mut chart = build_gdi_chart(well, metric, ranges);
            chart.set_width(640).set_height(340);
            worksheet.insert_chart_with_offset(chart_row, chart_col, &chart, 0, 0)?;
        }
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum ChartMetric {
    Thp,
    Bhp,
    Dep,
    Ipr,
}

impl ChartMetric {
    const ALL: [ChartMetric; 4] = [
        ChartMetric::Thp,
        ChartMetric::Bhp,
        ChartMetric::Dep,
        ChartMetric::Ipr,
    ];

    fn x_value<R: GdiChartRow>(self, row: &R) -> Option<f64> {
        match self {
            ChartMetric::Ipr => row.flo().map(|value| value / 1000.0),
            _ => row.flo(),
        }
    }

    fn y_value<R: GdiChartRow>(self, row: &R) -> Option<f64> {
        match self {
            ChartMetric::Thp => row.thp(),
            ChartMetric::Bhp | ChartMetric::Ipr => row.bhp(),
            ChartMetric::Dep => row.dep(),
        }
    }

    fn anchor_y<R: GdiChartRow>(self, rows: &[&R]) -> Option<f64> {
        match self {
            ChartMetric::Thp => rows.iter().find_map(|row| row.pst()),
            ChartMetric::Bhp => rows.iter().find_map(|row| row.ppl()),
            ChartMetric::Dep => Some(0.0),
            ChartMetric::Ipr => None,
        }
    }

    fn axis_name(self) -> &'static str {
        match self {
            ChartMetric::Thp => "thp",
            ChartMetric::Bhp => "bhp",
            ChartMetric::Dep => "dep",
            ChartMetric::Ipr => "bhp, кгс/см2",
        }
    }

    fn x_axis_name(self) -> &'static str {
        match self {
            ChartMetric::Ipr => "flo, тыс.м3/сут",
            _ => "flo",
        }
    }

    fn title_name(self) -> &'static str {
        match self {
            ChartMetric::Thp => "thp",
            ChartMetric::Bhp => "bhp",
            ChartMetric::Dep => "dep",
            ChartMetric::Ipr => "IPR",
        }
    }
}

#[derive(Clone, Copy)]
struct ChartDataRanges {
    helper_col: u16,
    actual_first_row: u32,
    actual_last_row: Option<u32>,
    fit_first_row: u32,
    fit_last_row: Option<u32>,
    r_squared: Option<f64>,
    /// Explicit Y-axis lower bound derived from actual data so the axis never auto-rounds to 0.
    y_axis_min: Option<f64>,
}

fn write_chart_helper_data<R>(
    worksheet: &mut Worksheet,
    rows: &[&R],
    metric: ChartMetric,
    helper_col: u16,
    data_row: &mut u32,
) -> Result<ChartDataRanges, XlsxError>
where
    R: GdiChartRow,
{
    let actual_first_row = *data_row;
    let mut actual_points = Vec::<(f64, f64)>::new();
    for row in rows {
        if let (Some(x), Some(value)) = (metric.x_value(*row), metric.y_value(*row)) {
            if x.is_finite() && value.is_finite() {
                worksheet.write(*data_row, helper_col, x)?;
                worksheet.write(*data_row, helper_col + 1, value)?;
                actual_points.push((x, value));
                *data_row += 1;
            }
        }
    }
    let actual_last_row = data_row
        .checked_sub(1)
        .filter(|row| *row >= actual_first_row);

    // Compute Y-axis minimum from actual data so Excel never auto-rounds down to 0.
    // Use a 5 % margin below the data minimum for a small visual gap.
    let y_axis_min = {
        let y_min = actual_points
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::INFINITY, f64::min);
        let y_max = actual_points
            .iter()
            .map(|(_, y)| *y)
            .fold(f64::NEG_INFINITY, f64::max);
        if y_min.is_finite() && y_max.is_finite() && y_max > y_min {
            let display_min = y_min - (y_max - y_min) * 0.05;
            display_min.is_finite().then_some(display_min)
        } else {
            None
        }
    };

    let (fit_first_row, fit_last_row, r_squared) = match metric {
        ChartMetric::Ipr => write_ipr_curve_data(worksheet, rows, helper_col, data_row)?,
        _ => write_polynomial_fit_data(worksheet, rows, metric, helper_col, data_row)?,
    };
    *data_row += 1;

    Ok(ChartDataRanges {
        helper_col,
        actual_first_row,
        actual_last_row,
        fit_first_row,
        fit_last_row,
        r_squared,
        y_axis_min,
    })
}

fn write_polynomial_fit_data<R>(
    worksheet: &mut Worksheet,
    rows: &[&R],
    metric: ChartMetric,
    helper_col: u16,
    data_row: &mut u32,
) -> Result<(u32, Option<u32>, Option<f64>), XlsxError>
where
    R: GdiChartRow,
{
    let fit_first_row = *data_row;
    let anchor = metric
        .anchor_y(rows)
        .filter(|value| value.is_finite())
        .map(|y| (0.0, y));

    let actual_points = rows
        .iter()
        .filter(|row| passes_chart_validity(**row))
        .filter_map(|row| {
            let x = metric.x_value(*row)?;
            let y = metric.y_value(*row)?;
            (x.is_finite() && y.is_finite()).then_some((x, y))
        })
        .collect::<Vec<_>>();

    // Anchor constrains the trend through the physically meaningful (flo=0, anchor) point,
    // so it is written together with actual points as input for Excel's polynomial trendline.
    let trend_points = anchor
        .iter()
        .copied()
        .chain(actual_points.iter().copied())
        .collect::<Vec<_>>();

    for (x, y) in &trend_points {
        worksheet.write(*data_row, helper_col + 2, *x)?;
        worksheet.write(*data_row, helper_col + 3, *y)?;
        *data_row += 1;
    }

    let fit_last_row = data_row.checked_sub(1).filter(|row| *row >= fit_first_row);
    // R² is computed over the same points Excel uses for its trendline: trend_points
    // (anchor + valid actuals), so the displayed value matches Excel's trendline R².
    let r_squared = fit_quadratic(&trend_points)
        .map(|coefficients| r_squared_for(&trend_points, |x| coefficients.predict(x)))
        .filter(|value| value.is_finite());

    Ok((fit_first_row, fit_last_row, r_squared))
}

fn write_ipr_curve_data<R>(
    worksheet: &mut Worksheet,
    rows: &[&R],
    helper_col: u16,
    data_row: &mut u32,
) -> Result<(u32, Option<u32>, Option<f64>), XlsxError>
where
    R: GdiChartRow,
{
    let fit_first_row = *data_row;
    let Some((ppl, a, b)) = ipr_curve_coefficients(rows) else {
        return Ok((fit_first_row, None, None));
    };

    let trend_actual_points = rows
        .iter()
        .filter(|row| passes_chart_validity(**row))
        .filter_map(|row| {
            let flo_thousand = row.flo()? / 1000.0;
            let bhp = row.bhp()?;
            (flo_thousand.is_finite() && bhp.is_finite()).then_some((flo_thousand, bhp))
        })
        .collect::<Vec<_>>();

    let max_flo = trend_actual_points
        .iter()
        .map(|(flo_thousand, _)| *flo_thousand)
        .fold(0.0_f64, f64::max);
    if !max_flo.is_finite() || max_flo <= 0.0 {
        return Ok((fit_first_row, None, None));
    }

    let step_count = 24_u32;
    for idx in 0..=step_count {
        let flo_thousand = max_flo * f64::from(idx) / f64::from(step_count);
        if let Some(bhp) = jones_bhp(ppl, a, b, flo_thousand / 1000.0) {
            worksheet.write(*data_row, helper_col + 2, flo_thousand)?;
            worksheet.write(*data_row, helper_col + 3, bhp)?;
            *data_row += 1;
        }
    }

    let fit_last_row = data_row.checked_sub(1).filter(|row| *row >= fit_first_row);
    let r_squared = ipr_r_squared(&trend_actual_points, ppl, a, b);

    Ok((fit_first_row, fit_last_row, r_squared))
}

fn ipr_curve_coefficients<R>(rows: &[&R]) -> Option<(f64, f64, f64)>
where
    R: GdiChartRow,
{
    rows.iter()
        .filter(|row| passes_chart_validity(**row))
        .filter_map(|row| Some((row.ppl()?, row.jones_a()?, row.jones_b()?)))
        .find(|(ppl, a, b)| ppl.is_finite() && a.is_finite() && b.is_finite())
}

// Validity rules: exclude empty values and rows with flo<0, thp<1, bhp<1, ppl<1, bhp>ppl, thp>bhp.
// Applied uniformly to polynomial trends (bhp/thp/dep), IPR, and R² calculations.
fn passes_chart_validity<R: GdiChartRow + ?Sized>(row: &R) -> bool {
    let (Some(flo), Some(thp), Some(bhp), Some(ppl)) = (row.flo(), row.thp(), row.bhp(), row.ppl())
    else {
        return false;
    };

    flo.is_finite()
        && thp.is_finite()
        && bhp.is_finite()
        && ppl.is_finite()
        && flo >= 0.0
        && thp >= 1.0
        && bhp >= 1.0
        && ppl >= 1.0
        && bhp <= ppl
        && thp <= bhp
}

// R² as Excel-style "1 - SSres/SStot" for the Jones IPR curve. Returns None if any actual
// point cannot be predicted (negative bhp² under the square root) — the curve does not cover
// the data set, so a fit quality score is not meaningful.
fn ipr_r_squared(actual_points: &[(f64, f64)], ppl: f64, a: f64, b: f64) -> Option<f64> {
    if actual_points.len() < 2 {
        return None;
    }

    // Include the anchor (flo=0, bhp=ppl) to match the polynomial trendline R² convention:
    // the Jones curve starts at (0, ppl) and that point is part of the scored series.
    // At q=0 jones_bhp returns ppl exactly, so its residual is 0.
    let all_points: Vec<(f64, f64)> = std::iter::once((0.0_f64, ppl))
        .chain(actual_points.iter().copied())
        .collect();

    let predictions = all_points
        .iter()
        .map(|(flo_thousand, _)| jones_bhp(ppl, a, b, flo_thousand / 1000.0))
        .collect::<Option<Vec<_>>>()?;

    let mean = all_points.iter().map(|(_, y)| *y).sum::<f64>() / all_points.len() as f64;
    let mut sse = 0.0;
    let mut sst = 0.0;
    for ((_, actual), predicted) in all_points.iter().zip(predictions.iter()) {
        sse += (actual - predicted).powi(2);
        sst += (actual - mean).powi(2);
    }

    if sst <= f64::EPSILON {
        return (sse <= f64::EPSILON).then_some(1.0);
    }
    let r2 = 1.0 - sse / sst;
    r2.is_finite().then_some(r2)
}

fn build_gdi_chart(well: i64, metric: ChartMetric, ranges: ChartDataRanges) -> Chart {
    let mut chart = Chart::new(ChartType::Scatter);
    chart.show_hidden_data();
    let title = match ranges.r_squared {
        Some(value) if value.is_finite() => {
            format!("well № {well} {} R2 = {:.2}", metric.title_name(), value)
        }
        _ => format!("well № {well} {} R2 = --", metric.title_name()),
    };
    chart.title().set_name(&title);
    chart.x_axis().set_name(metric.x_axis_name());
    let y_axis = chart.y_axis();
    y_axis.set_name(metric.axis_name());
    if let Some(min) = ranges.y_axis_min {
        y_axis.set_min(min);
    }
    chart.legend().set_hidden();

    if let Some(last_row) = ranges.actual_last_row {
        chart
            .add_series()
            .set_name("fact")
            .set_categories((
                GDI_CHARTS_SHEET_NAME,
                ranges.actual_first_row,
                ranges.helper_col,
                last_row,
                ranges.helper_col,
            ))
            .set_values((
                GDI_CHARTS_SHEET_NAME,
                ranges.actual_first_row,
                ranges.helper_col + 1,
                last_row,
                ranges.helper_col + 1,
            ))
            .set_format(ChartFormat::new().set_no_line())
            .set_marker(
                ChartMarker::new()
                    .set_type(ChartMarkerType::Circle)
                    .set_size(5)
                    .set_format(
                        ChartFormat::new()
                            .set_solid_fill(ChartSolidFill::new().set_color("#FF0000"))
                            .set_border(ChartLine::new().set_color("#FF0000")),
                    ),
            );
    }

    if let Some(last_row) = ranges
        .fit_last_row
        .filter(|last_row| last_row.saturating_sub(ranges.fit_first_row) + 1 >= 2)
    {
        let fit_series = chart
            .add_series()
            .set_name("trend")
            .set_categories((
                GDI_CHARTS_SHEET_NAME,
                ranges.fit_first_row,
                ranges.helper_col + 2,
                last_row,
                ranges.helper_col + 2,
            ))
            .set_values((
                GDI_CHARTS_SHEET_NAME,
                ranges.fit_first_row,
                ranges.helper_col + 3,
                last_row,
                ranges.helper_col + 3,
            ))
            .set_format(ChartFormat::new().set_no_line())
            .set_marker(ChartMarker::new().set_none());

        if matches!(metric, ChartMetric::Ipr) {
            fit_series.set_format(ChartLine::new().set_color("#0000FF").set_width(1.5));
        } else if last_row.saturating_sub(ranges.fit_first_row) + 1 >= 3 {
            let mut trendline = ChartTrendline::new();
            trendline
                .set_type(ChartTrendlineType::Polynomial(2))
                .set_format(ChartLine::new().set_color("#0000FF").set_width(1.5))
                .delete_from_legend(true);
            fit_series.set_trendline(&trendline);
        }
    }

    chart
}

#[derive(Clone, Copy)]
struct QuadraticFit {
    offset: f64,
    scale: f64,
    a: f64,
    b: f64,
    c: f64,
}

impl QuadraticFit {
    fn predict(self, x: f64) -> f64 {
        let normalized_x = (x - self.offset) / self.scale;
        self.a * normalized_x * normalized_x + self.b * normalized_x + self.c
    }
}

fn fit_quadratic(points: &[(f64, f64)]) -> Option<QuadraticFit> {
    if points.len() < 3 {
        return None;
    }

    let offset = points.iter().map(|(x, _)| *x).sum::<f64>() / points.len() as f64;
    let scale = points
        .iter()
        .map(|(x, _)| (*x - offset).abs())
        .fold(0.0_f64, f64::max)
        .max(1.0);

    let mut sx = 0.0;
    let mut sx2 = 0.0;
    let mut sx3 = 0.0;
    let mut sx4 = 0.0;
    let mut sy = 0.0;
    let mut sxy = 0.0;
    let mut sx2y = 0.0;

    for &(x, y) in points {
        let normalized_x = (x - offset) / scale;
        let x2 = normalized_x * normalized_x;
        sx += normalized_x;
        sx2 += x2;
        sx3 += x2 * normalized_x;
        sx4 += x2 * x2;
        sy += y;
        sxy += normalized_x * y;
        sx2y += x2 * y;
    }

    let (a, b, c) = solve_3x3([
        [sx4, sx3, sx2, sx2y],
        [sx3, sx2, sx, sxy],
        [sx2, sx, points.len() as f64, sy],
    ])?;

    Some(QuadraticFit {
        offset,
        scale,
        a,
        b,
        c,
    })
}

fn solve_3x3(mut matrix: [[f64; 4]; 3]) -> Option<(f64, f64, f64)> {
    for pivot_col in 0..3 {
        let pivot_row = (pivot_col..3).max_by(|&left, &right| {
            matrix[left][pivot_col]
                .abs()
                .partial_cmp(&matrix[right][pivot_col].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

        if matrix[pivot_row][pivot_col].abs() <= 1.0e-12 {
            return None;
        }

        if pivot_row != pivot_col {
            matrix.swap(pivot_row, pivot_col);
        }

        let pivot = matrix[pivot_col][pivot_col];
        for col in pivot_col..4 {
            matrix[pivot_col][col] /= pivot;
        }

        for row in 0..3 {
            if row == pivot_col {
                continue;
            }
            let factor = matrix[row][pivot_col];
            for col in pivot_col..4 {
                matrix[row][col] -= factor * matrix[pivot_col][col];
            }
        }
    }

    Some((matrix[0][3], matrix[1][3], matrix[2][3]))
}

fn jones_bhp(ppl: f64, a: f64, b: f64, q_mln: f64) -> Option<f64> {
    let ppl_bar = kgf_cm2_to_bar(ppl);
    let bhp_squared_bar = ppl_bar * ppl_bar - a * q_mln * q_mln - b * q_mln;
    (bhp_squared_bar >= 0.0 && bhp_squared_bar.is_finite())
        .then(|| bar_to_kgf_cm2(bhp_squared_bar.sqrt()))
}

fn kgf_cm2_to_bar(value: f64) -> f64 {
    value * BAR_PER_KGF_CM2
}

fn bar_to_kgf_cm2(value: f64) -> f64 {
    value / BAR_PER_KGF_CM2
}

fn r_squared_for(points: &[(f64, f64)], predict: impl Fn(f64) -> f64) -> f64 {
    if points.len() < 2 {
        return f64::NAN;
    }

    let mean = points.iter().map(|(_, y)| *y).sum::<f64>() / points.len() as f64;
    let mut sse = 0.0;
    let mut sst = 0.0;
    for &(x, y) in points {
        let predicted = predict(x);
        if !predicted.is_finite() {
            return f64::NAN;
        }
        let residual = y - predicted;
        sse += residual * residual;
        let centered = y - mean;
        sst += centered * centered;
    }

    if sst <= f64::EPSILON {
        if sse <= f64::EPSILON { 1.0 } else { f64::NAN }
    } else {
        1.0 - sse / sst
    }
}

pub(crate) fn configure_ss_sheet<'a>(
    workbook: &'a mut Workbook,
    headers: &[&str],
) -> Result<&'a mut Worksheet, XlsxError> {
    let worksheet = workbook.add_worksheet();
    worksheet.set_freeze_panes(1, 0)?;
    worksheet.set_row_height(0, 30.0)?;
    for col in 0..headers.len() as u16 {
        worksheet.set_column_width(col, 10.0)?;
    }
    Ok(worksheet)
}

pub(crate) fn write_headers(
    worksheet: &mut Worksheet,
    headers: &[&str],
    default_format: &Format,
    special_column: Option<(usize, &Format)>,
) -> Result<(), XlsxError> {
    for (col, header) in headers.iter().enumerate() {
        let format = match special_column {
            Some((special_col, special_format)) if special_col == col => special_format,
            _ => default_format,
        };
        worksheet.write_with_format(0, col as u16, *header, format)?;
    }
    Ok(())
}

/// Ячейки строки отчёта «как GDI». Обе ветки (DBF и KOTS) пишут одну и ту
/// же сетку колонок и различаются только «№ режима», форматом даты и
/// колонкой удельной жидкости — эти значения строка отдаёт готовыми.
pub(crate) struct GdiLikeCells<'a> {
    pub(crate) gp: Option<&'a str>,
    pub(crate) plast: Option<&'a str>,
    pub(crate) regime_no: RegimeCell<'a>,
    pub(crate) washer: Option<f64>,
    pub(crate) date_text: String,
    pub(crate) liquid_ratio: Option<f64>,
    pub(crate) gauge: Option<f64>,
    pub(crate) tpl: Option<f64>,
    pub(crate) water: Option<f64>,
    pub(crate) sand: Option<f64>,
    pub(crate) max_dep: Option<f64>,
    pub(crate) c: Option<f64>,
    pub(crate) n: Option<f64>,
    pub(crate) max_flo: Option<f64>,
    pub(crate) dop_skv_flo: Option<f64>,
    pub(crate) dop_skv_flo_percent_5: Option<f64>,
    pub(crate) dop_skv_flo_095: Option<f64>,
    pub(crate) limit_comment: Option<&'a str>,
}

/// «№ режима»: в DBF-ветке это число, в KOTS — текст.
pub(crate) enum RegimeCell<'a> {
    Number(Option<f64>),
    Text(Option<&'a str>),
}

/// Строка отчёта «как GDI» для общего писателя.
pub(crate) trait GdiLikeRow: GdiChartRow {
    fn date_key(&self) -> i64;
    fn cells(&self) -> GdiLikeCells<'_>;
}

/// Книга отчёта «как GDI»: основной лист, опциональный FILTRED_GDI
/// (строки не раньше filter_date) и опциональный лист графиков.
pub(crate) fn write_gdi_like_workbook<R: GdiLikeRow>(
    path: &Path,
    rows: &[R],
    export_charts: bool,
    filter_date: Option<i64>,
) -> Result<()> {
    crate::paths::ensure_parent_dir(path)?;
    let mut workbook = Workbook::new();
    let formats = gdi_like_formats();
    {
        let worksheet = configure_gdi_like_sheet(&mut workbook)?;
        write_headers(
            worksheet,
            &GDI_LIKE_HEADERS,
            &formats.header,
            Some((4, &formats.header_well)),
        )?;
        write_gdi_like_rows(worksheet, &formats, rows.iter())?;
    }

    if let Some(date_key) = filter_date {
        let worksheet = configure_gdi_filtered_sheet(&mut workbook)?;
        write_headers(
            worksheet,
            &GDI_LIKE_HEADERS,
            &formats.header,
            Some((4, &formats.header_well)),
        )?;
        write_gdi_like_rows(
            worksheet,
            &formats,
            rows.iter().filter(|row| row.date_key() >= date_key),
        )?;
    }

    if export_charts {
        add_gdi_charts_sheet(&mut workbook, rows)?;
    }

    workbook
        .save(path)
        .with_context(|| format!("Не удалось сохранить {}", path.display()))?;
    Ok(())
}

fn write_gdi_like_rows<'a, R: GdiLikeRow + 'a>(
    worksheet: &mut Worksheet,
    formats: &GdiLikeFormats,
    rows: impl Iterator<Item = &'a R>,
) -> Result<(), XlsxError> {
    let mut prev: Option<(i64, i64)> = None;
    for (idx, row) in rows.enumerate() {
        let excel_row = (idx + 1) as u32;
        let top_border =
            prev.is_some_and(|(well, date)| well != row.well() || date != row.date_key());
        let cells = row.cells();

        let mut alerts = AlertColumns::<GDI_LIKE_COLUMN_COUNT>::default();
        apply_gdi_like_alerts(
            &mut alerts,
            GdiLikeAlertInputs {
                thp: row.thp(),
                bhp: row.bhp(),
                ppl: row.ppl(),
                pst: row.pst(),
                dep: row.dep(),
                max_dep: cells.max_dep,
                sand: cells.sand,
            },
        );

        for col in 0..GDI_LIKE_HEADERS.len() {
            let format = gdi_like_format(formats, &alerts, col, top_border);
            write_gdi_like_cell(worksheet, excel_row, col, row, &cells, format)?;
        }
        prev = Some((row.well(), row.date_key()));
    }
    Ok(())
}

fn write_gdi_like_cell<R: GdiLikeRow>(
    worksheet: &mut Worksheet,
    row_index: u32,
    col: usize,
    row: &R,
    cells: &GdiLikeCells<'_>,
    format: Option<&Format>,
) -> Result<(), XlsxError> {
    let col_index = col as u16;
    match col {
        0 => write_optional_text(worksheet, row_index, col_index, cells.gp, format),
        1 => write_optional_text(worksheet, row_index, col_index, cells.plast, format),
        2 => match cells.regime_no {
            RegimeCell::Number(value) => {
                write_optional_number(worksheet, row_index, col_index, value, format)
            }
            RegimeCell::Text(value) => {
                write_optional_text(worksheet, row_index, col_index, value, format)
            }
        },
        3 => write_optional_number(worksheet, row_index, col_index, cells.washer, format),
        4 => write_number(worksheet, row_index, col_index, row.well() as f64, format),
        5 => write_optional_text(
            worksheet,
            row_index,
            col_index,
            Some(cells.date_text.as_str()),
            format,
        ),
        6 => write_optional_number(worksheet, row_index, col_index, row.thp(), format),
        8 => write_optional_number(worksheet, row_index, col_index, row.flo(), format),
        9 => write_optional_number(worksheet, row_index, col_index, cells.liquid_ratio, format),
        10..=11 => write_optional_number(worksheet, row_index, col_index, Some(0.0), format),
        12 => write_optional_number(worksheet, row_index, col_index, cells.gauge, format),
        13 => write_optional_number(worksheet, row_index, col_index, row.bhp(), format),
        15 => write_optional_number(worksheet, row_index, col_index, Some(1.0), format),
        17 => write_optional_text(worksheet, row_index, col_index, Some("True"), format),
        18 => write_optional_number(worksheet, row_index, col_index, row.pst(), format),
        19 => write_optional_number(worksheet, row_index, col_index, row.ppl(), format),
        20 => write_optional_number(worksheet, row_index, col_index, cells.tpl, format),
        21 => write_optional_number(worksheet, row_index, col_index, cells.water, format),
        22 => write_optional_number(worksheet, row_index, col_index, cells.sand, format),
        23 => write_optional_number(worksheet, row_index, col_index, row.dep(), format),
        24 => write_optional_number(worksheet, row_index, col_index, cells.max_dep, format),
        25 => write_optional_number(worksheet, row_index, col_index, cells.c, format),
        26 => write_optional_number(worksheet, row_index, col_index, cells.n, format),
        27 => write_optional_number(worksheet, row_index, col_index, cells.max_flo, format),
        28 => write_optional_number(worksheet, row_index, col_index, cells.dop_skv_flo, format),
        29 => write_optional_number(
            worksheet,
            row_index,
            col_index,
            cells.dop_skv_flo_percent_5,
            format,
        ),
        30 => write_optional_number(
            worksheet,
            row_index,
            col_index,
            cells.dop_skv_flo_095,
            format,
        ),
        31 => write_optional_text(worksheet, row_index, col_index, cells.limit_comment, format),
        32 => write_optional_number(worksheet, row_index, col_index, row.jones_a(), format),
        33 => write_optional_number(worksheet, row_index, col_index, row.jones_b(), format),
        // 7, 14, 16 — служебные колонки, всегда "None"
        _ => write_optional_text(worksheet, row_index, col_index, Some("None"), format),
    }
}

pub(crate) fn gdi_like_format<'a>(
    formats: &'a GdiLikeFormats,
    alerts: &AlertColumns<GDI_LIKE_COLUMN_COUNT>,
    col: usize,
    top_border: bool,
) -> Option<&'a Format> {
    if alerts.red[col] {
        Some(if top_border {
            &formats.top_red
        } else {
            &formats.base_red
        })
    } else if alerts.orange[col] {
        Some(if top_border {
            &formats.top_orange
        } else {
            &formats.base_orange
        })
    } else if col == 4 {
        Some(if top_border {
            &formats.top_green
        } else {
            &formats.base_green
        })
    } else if top_border {
        Some(&formats.top_only)
    } else {
        None
    }
}

pub(crate) fn ss_row_format(
    formats: &SsFormats,
    grey_row: bool,
    top_border: bool,
) -> Option<&Format> {
    match (grey_row, top_border) {
        (true, true) => Some(&formats.row_grey_top),
        (true, false) => Some(&formats.row_grey),
        (false, true) => Some(&formats.row_top),
        (false, false) => None,
    }
}

pub(crate) fn apply_gdi_like_alerts(
    alerts: &mut AlertColumns<GDI_LIKE_COLUMN_COUNT>,
    inputs: GdiLikeAlertInputs,
) {
    if inputs.sand.unwrap_or(0.0) > 2.0 {
        alerts.mark_red(&[22]);
    }

    let dep = inputs.dep.unwrap_or(0.0);
    let max_dep = inputs.max_dep.unwrap_or(0.0);
    let ppl = inputs.ppl.unwrap_or(0.0);
    let pst = inputs.pst.unwrap_or(0.0);
    let bhp = inputs.bhp.unwrap_or(0.0);
    let thp = inputs.thp.unwrap_or(0.0);

    if ppl <= pst {
        alerts.mark_orange(&[18, 19]);
    }
    if thp >= bhp {
        alerts.mark_orange(&[6, 13]);
    }
    if ppl <= bhp {
        alerts.mark_orange(&[13, 19]);
    }
    if pst <= thp {
        alerts.mark_orange(&[6, 18]);
    }
    if thp <= 1.0 {
        alerts.mark_orange(&[6]);
    }
    if bhp <= 1.0 {
        alerts.mark_orange(&[13]);
    }
    if dep <= 0.0 {
        alerts.mark_orange(&[23]);
    }
    if dep > max_dep {
        alerts.mark_red(&[23]);
    }
    if alerts.has_red() {
        alerts.mark_red(&[8]);
    }
}

pub(crate) fn write_number(
    worksheet: &mut Worksheet,
    row: u32,
    col: u16,
    value: f64,
    format: Option<&Format>,
) -> Result<(), XlsxError> {
    match format {
        Some(format) => worksheet
            .write_with_format(row, col, value, format)
            .map(|_| ()),
        None => worksheet.write_number(row, col, value).map(|_| ()),
    }
}

pub(crate) fn write_optional_number(
    worksheet: &mut Worksheet,
    row: u32,
    col: u16,
    value: Option<f64>,
    format: Option<&Format>,
) -> Result<(), XlsxError> {
    match (value, format) {
        (Some(value), Some(format)) => worksheet
            .write_with_format(row, col, value, format)
            .map(|_| ()),
        (Some(value), None) => worksheet.write_number(row, col, value).map(|_| ()),
        (None, Some(format)) => worksheet.write_blank(row, col, format).map(|_| ()),
        (None, None) => Ok(()),
    }
}

pub(crate) fn write_optional_text(
    worksheet: &mut Worksheet,
    row: u32,
    col: u16,
    value: Option<&str>,
    format: Option<&Format>,
) -> Result<(), XlsxError> {
    match (value, format) {
        (Some(value), Some(format)) => worksheet
            .write_with_format(row, col, value, format)
            .map(|_| ()),
        (Some(value), None) => worksheet.write_string(row, col, value).map(|_| ()),
        (None, Some(format)) => worksheet.write_blank(row, col, format).map(|_| ()),
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct ChartRow {
        well: i64,
        flo: f64,
        thp: f64,
        bhp: f64,
        dep: f64,
        pst: f64,
        ppl: f64,
        jones_a: f64,
        jones_b: f64,
    }

    impl GdiChartRow for ChartRow {
        fn well(&self) -> i64 {
            self.well
        }

        fn flo(&self) -> Option<f64> {
            Some(self.flo)
        }

        fn thp(&self) -> Option<f64> {
            Some(self.thp)
        }

        fn bhp(&self) -> Option<f64> {
            Some(self.bhp)
        }

        fn dep(&self) -> Option<f64> {
            Some(self.dep)
        }

        fn pst(&self) -> Option<f64> {
            Some(self.pst)
        }

        fn ppl(&self) -> Option<f64> {
            Some(self.ppl)
        }

        fn jones_a(&self) -> Option<f64> {
            Some(self.jones_a)
        }

        fn jones_b(&self) -> Option<f64> {
            Some(self.jones_b)
        }
    }

    #[test]
    fn writes_gdi_charts_sheet() {
        let rows = [
            chart_row(101, 10_000.0, 20.0, 90.0, 10.0),
            chart_row(101, 20_000.0, 30.0, 80.0, 20.0),
            chart_row(101, 30_000.0, 40.0, 70.0, 30.0),
        ];

        let mut workbook = Workbook::new();
        add_gdi_charts_sheet(&mut workbook, &rows).unwrap();

        let path = std::env::temp_dir().join(format!(
            "gdi_charts_{}.xlsx",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        workbook.save(&path).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn calculates_stable_quadratic_r_squared_for_large_flo_values() {
        let points = [
            (0.0, 100.0),
            (1_000_000.0, 96.0),
            (2_000_000.0, 84.0),
            (3_000_000.0, 64.0),
        ];

        let fit = fit_quadratic(&points).unwrap();
        let r_squared = r_squared_for(&points, |x| fit.predict(x));

        assert!((r_squared - 1.0).abs() < 1.0e-10);
    }

    #[test]
    fn calculates_ipr_bhp_in_kgf_from_bar_jones_coefficients() {
        let ppl = 100.0;
        let bhp = 90.0;
        let q_mln = 0.1;
        let b = (kgf_cm2_to_bar(ppl).powi(2) - kgf_cm2_to_bar(bhp).powi(2)) / q_mln;

        let calculated_bhp = jones_bhp(ppl, 0.0, b, q_mln).unwrap();

        assert!((calculated_bhp - bhp).abs() < 1.0e-10);
    }

    #[test]
    fn r_squared_matches_excel_trendline_formula() {
        // 1 - SSres/SStot with actual {100, 80, 70} and predicted {98, 82, 69}:
        //   SSres = 4 + 4 + 1 = 9, SStot = 4200/9, R² = 1 - 81/4200 ≈ 0.980714285714286
        let r2 = r_squared_for(&[(100.0, 100.0), (80.0, 80.0), (70.0, 70.0)], |x| match x {
            v if (v - 100.0).abs() < 1e-9 => 98.0,
            v if (v - 80.0).abs() < 1e-9 => 82.0,
            v if (v - 70.0).abs() < 1e-9 => 69.0,
            _ => f64::NAN,
        });

        assert!((r2 - 0.980_714_285_714_285_7).abs() < 1.0e-12);
    }

    #[test]
    fn calculates_ipr_r_squared_from_jones_curve_at_actual_flo() {
        let ppl = 100.0;
        let first_q_mln = 0.1;
        let second_q_mln = 0.2;
        let first_bhp = 90.0;
        let second_bhp = 70.0;
        let first_delta = kgf_cm2_to_bar(ppl).powi(2) - kgf_cm2_to_bar(first_bhp).powi(2);
        let second_delta = kgf_cm2_to_bar(ppl).powi(2) - kgf_cm2_to_bar(second_bhp).powi(2);
        let a = (second_delta / second_q_mln - first_delta / first_q_mln)
            / (second_q_mln - first_q_mln);
        let b = first_delta / first_q_mln - a * first_q_mln;

        let r_squared = ipr_r_squared(
            &[
                (first_q_mln * 1000.0, first_bhp),
                (second_q_mln * 1000.0, second_bhp),
            ],
            ppl,
            a,
            b,
        )
        .unwrap();

        assert!((r_squared - 1.0).abs() < 1.0e-12);
    }

    #[test]
    fn ipr_r_squared_returns_none_when_jones_curve_fails_for_actual_point() {
        // b=100_000 bar/Mm³ makes bhp²<0 at actual flo → jones_bhp returns None → R² is None
        let r_squared = ipr_r_squared(&[(100.0, 90.0), (200.0, 80.0)], 100.0, 0.0, 100_000.0);

        assert!(r_squared.is_none());
    }

    #[test]
    fn ipr_coefficients_ignore_non_finite_values() {
        let invalid = ChartRow {
            jones_a: f64::NAN,
            ..chart_row(101, 10_000.0, 20.0, 90.0, 10.0)
        };
        let valid = ChartRow {
            jones_a: 12.0,
            jones_b: 34.0,
            ..chart_row(101, 20_000.0, 30.0, 80.0, 20.0)
        };
        let rows = [&invalid, &valid];

        assert_eq!(ipr_curve_coefficients(&rows), Some((100.0, 12.0, 34.0)));
    }

    #[test]
    fn chart_validity_rejects_invalid_pressure_order_and_low_values() {
        // Rejection rules: flo<0, thp<1, bhp<1, ppl<1, bhp>ppl, thp>bhp.
        let valid = chart_row(101, 20_000.0, 30.0, 80.0, 20.0);
        let thp_above_bhp = ChartRow {
            thp: 95.0,
            ..chart_row(101, 10_000.0, 20.0, 90.0, 10.0)
        };
        let bhp_above_ppl = ChartRow {
            bhp: 120.0,
            ..chart_row(101, 10_000.0, 20.0, 90.0, 10.0)
        };
        let low_thp = ChartRow {
            thp: 0.5,
            ..chart_row(101, 10_000.0, 20.0, 90.0, 10.0)
        };
        let negative_flo = chart_row(101, -1.0, 20.0, 90.0, 10.0);

        assert!(passes_chart_validity(&valid));
        assert!(!passes_chart_validity(&thp_above_bhp));
        assert!(!passes_chart_validity(&bhp_above_ppl));
        assert!(!passes_chart_validity(&low_thp));
        assert!(!passes_chart_validity(&negative_flo));
    }

    fn chart_row(well: i64, flo: f64, thp: f64, bhp: f64, dep: f64) -> ChartRow {
        ChartRow {
            well,
            flo,
            thp,
            bhp,
            dep,
            pst: 50.0,
            ppl: 100.0,
            jones_a: 10_000.0,
            jones_b: 1_000.0,
        }
    }
}
