//! Shared GDI limit calculation used by DBF and KOTS/XLSX branches.

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LimitInput {
    pub(crate) regime_no: Option<f64>,
    pub(crate) flo: Option<f64>,
    pub(crate) sand: Option<f64>,
    pub(crate) dep: Option<f64>,
    pub(crate) max_dep: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct LimitResult {
    pub(crate) max_flo: Option<f64>,
    pub(crate) dop_skv_flo: Option<f64>,
    pub(crate) dop_skv_flo_percent_5: Option<f64>,
    pub(crate) dop_skv_flo_095: Option<f64>,
    pub(crate) comment: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct JonesInput {
    pub(crate) flo: Option<f64>,
    pub(crate) thp: Option<f64>,
    pub(crate) ppl: Option<f64>,
    pub(crate) bhp: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct JonesResult {
    pub(crate) a: Option<f64>,
    pub(crate) b: Option<f64>,
}

/// Строка отчёта, к которой применяются ограничения и коэффициенты Джонса.
/// Обе ветки GDI (DBF и KOTS) различаются только источником «№ режима».
pub(crate) trait LimitTarget {
    /// Группа расчёта: (скважина, дата исследования).
    fn group_key(&self) -> (i64, i64);
    fn limit_input(&self) -> LimitInput;
    fn jones_input(&self) -> JonesInput;
    fn apply(&mut self, limits: &LimitResult, jones: &JonesResult);
}

/// Расчёт и запись ограничений по группам (скважина, дата).
pub(crate) fn apply_limit_results<R: LimitTarget>(rows: &mut [R]) {
    let mut groups = ahash::AHashMap::<(i64, i64), Vec<usize>>::new();
    for (idx, row) in rows.iter().enumerate() {
        groups.entry(row.group_key()).or_default().push(idx);
    }

    for indexes in groups.values() {
        let inputs: Vec<LimitInput> = indexes.iter().map(|&idx| rows[idx].limit_input()).collect();
        let limits = calculate_limits(&inputs);
        let jones_inputs: Vec<JonesInput> =
            indexes.iter().map(|&idx| rows[idx].jones_input()).collect();
        let jones = calculate_jones_coefficients(&jones_inputs);
        for &idx in indexes {
            rows[idx].apply(&limits, &jones);
        }
    }
}

/// Оставляет по каждой скважине только строки последней даты
/// (режим «только последние исследования»).
pub(crate) fn retain_last_date_per_well<T>(rows: &mut Vec<T>, key: impl Fn(&T) -> (i64, i64)) {
    let mut last_dates = ahash::AHashMap::<i64, i64>::with_capacity(rows.len());
    for row in rows.iter() {
        let (well, date) = key(row);
        last_dates
            .entry(well)
            .and_modify(|value| {
                if date > *value {
                    *value = date;
                }
            })
            .or_insert(date);
    }
    rows.retain(|row| {
        let (well, date) = key(row);
        last_dates.get(&well) == Some(&date)
    });
}

#[derive(Debug, Clone, Copy)]
struct LimitRow {
    regime_no: Option<f64>,
    flo: Option<f64>,
    sand: f64,
    dep: Option<f64>,
    max_dep: Option<f64>,
}

const MAX_SAND: f64 = 2.0;
const BAR_PER_KGF_CM2: f64 = 0.980665;

pub(crate) fn calculate_limits(group: &[LimitInput]) -> LimitResult {
    if group.is_empty() {
        return LimitResult::default();
    }

    let mut rows = group
        .iter()
        .map(|row| LimitRow {
            regime_no: row.regime_no,
            flo: row.flo,
            sand: row.sand.unwrap_or(0.0),
            dep: row.dep,
            max_dep: row.max_dep,
        })
        // Regimes with zero (or missing) flow carry no information — drop them.
        .filter(|row| row.flo.is_some_and(|flo| flo > 0.0))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return LimitResult::default();
    }
    rows.sort_by(compare_by_regime_no);

    let max_dep_limit = rows[0].max_dep.unwrap_or(f64::INFINITY);
    let max_flo = max_flo(rows.iter().filter_map(|row| row.flo));

    let (limit_sand_flo, mut comment) = calculate_sand_limit(&rows, max_flo);
    let limit_dep_flo = calculate_dep_limit(&rows, max_flo, max_dep_limit, &mut comment);
    let final_flo = limit_sand_flo.min(limit_dep_flo);

    if limit_dep_flo < limit_sand_flo && comment == "ПЕСОК + ДЕПРЕССИЯ" {
        comment = "ДЕПРЕССИЯ + ПЕСОК".to_string();
    }

    let Some(dop_skv_flo) = round_half_up_to_thousand(final_flo) else {
        return LimitResult {
            max_flo,
            comment,
            ..LimitResult::default()
        };
    };
    let dop_skv_flo_percent_5 = round_down_to_five_thousand(dop_skv_flo);
    let dop_skv_flo_095 = round_down_to_five_thousand(dop_skv_flo_percent_5 * 0.95);

    LimitResult {
        max_flo,
        dop_skv_flo: Some(dop_skv_flo),
        dop_skv_flo_percent_5: Some(dop_skv_flo_percent_5),
        dop_skv_flo_095: Some(dop_skv_flo_095),
        comment,
    }
}

pub(crate) fn calculate_jones_coefficients(group: &[JonesInput]) -> JonesResult {
    let points = group
        .iter()
        .filter_map(|row| {
            if !is_valid_jones_regime(row) {
                return None;
            }
            let q = row.flo? / 1_000_000.0;
            let ppl = kgf_cm2_to_bar(row.ppl?);
            let bhp = kgf_cm2_to_bar(row.bhp?);
            let pressure_delta_squared = ppl * ppl - bhp * bhp;
            (q > 0.0 && q.is_finite() && pressure_delta_squared.is_finite())
                .then_some((q, pressure_delta_squared))
        })
        .collect::<Vec<_>>();

    if points.is_empty() {
        return JonesResult::default();
    }

    let (a, b) = non_negative_jones_fit(&points);
    JonesResult {
        a: Some(a),
        b: Some(b),
    }
}

// Validity rules: exclude empty values and points with flo<0, thp<1, bhp<1, ppl<1, bhp>ppl, thp>bhp.
fn is_valid_jones_regime(row: &JonesInput) -> bool {
    let (Some(flo), Some(thp), Some(bhp), Some(ppl)) = (row.flo, row.thp, row.bhp, row.ppl) else {
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

fn kgf_cm2_to_bar(value: f64) -> f64 {
    value * BAR_PER_KGF_CM2
}

fn non_negative_jones_fit(points: &[(f64, f64)]) -> (f64, f64) {
    let mut sum_q2 = 0.0;
    let mut sum_q3 = 0.0;
    let mut sum_q4 = 0.0;
    let mut sum_q_y = 0.0;
    let mut sum_q2_y = 0.0;

    for &(q, y) in points {
        let q2 = q * q;
        sum_q2 += q2;
        sum_q3 += q2 * q;
        sum_q4 += q2 * q2;
        sum_q_y += q * y;
        sum_q2_y += q2 * y;
    }

    let determinant = sum_q4 * sum_q2 - sum_q3 * sum_q3;
    if determinant.abs() > f64::EPSILON {
        let a = (sum_q2_y * sum_q2 - sum_q_y * sum_q3) / determinant;
        let b = (sum_q4 * sum_q_y - sum_q3 * sum_q2_y) / determinant;
        if a >= 0.0 && b >= 0.0 && a.is_finite() && b.is_finite() {
            return (a, b);
        }
    }

    let a_only = (sum_q4 > f64::EPSILON).then_some((sum_q2_y / sum_q4).max(0.0));
    let b_only = (sum_q2 > f64::EPSILON).then_some((sum_q_y / sum_q2).max(0.0));
    [
        (a_only.unwrap_or(0.0), 0.0),
        (0.0, b_only.unwrap_or(0.0)),
        (0.0, 0.0),
    ]
    .into_iter()
    .min_by(|left, right| {
        jones_error(points, *left)
            .partial_cmp(&jones_error(points, *right))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
    .unwrap_or((0.0, 0.0))
}

fn jones_error(points: &[(f64, f64)], coefficients: (f64, f64)) -> f64 {
    let (a, b) = coefficients;
    points
        .iter()
        .map(|(q, y)| {
            let residual = a * q * q + b * q - y;
            residual * residual
        })
        .sum()
}

fn calculate_sand_limit(rows: &[LimitRow], max_flo_value: Option<f64>) -> (f64, String) {
    // `rows` are pre-sorted by regime_no, i.e. in the order the regimes were run.
    if !rows.iter().any(|row| row.sand > MAX_SAND) {
        return (max_flo_or_nan(max_flo_value), "ГДИ".to_string());
    }
    if rows.iter().all(|row| row.sand > MAX_SAND) {
        return (0.0, "ПЕСОК НА ВСЕХ РЕЖИМАХ!".to_string());
    }

    // Sandy regimes before the first clean one are treated as initial cleanup and do not
    // bound the rate.
    let first_clean = rows
        .iter()
        .position(|row| row.sand <= MAX_SAND)
        .expect("a clean regime exists");
    // A sandy regime bounds the rate only if the well never flowed clean above it afterwards.
    // A later clean regime at a higher rate shows the earlier sand was transient (a one-off
    // spike) and must not cap the rate. The binding sand is the lowest such regime.
    let min_sandy_flo = rows
        .iter()
        .enumerate()
        .skip(first_clean)
        .filter(|(_, row)| row.sand > MAX_SAND)
        .filter(|(idx, row)| {
            !rows[idx + 1..].iter().any(|later| {
                later.sand <= MAX_SAND && later.flo.zip(row.flo).is_some_and(|(l, c)| l > c)
            })
        })
        .filter_map(|(_, row)| row.flo)
        .fold(f64::INFINITY, f64::min);

    // The safe rate is the highest clean rate that stayed below the binding sand.
    let limit = max_flo(
        rows.iter()
            .filter(|row| row.sand <= MAX_SAND)
            .filter_map(|row| row.flo)
            .filter(|&flo| flo < min_sandy_flo),
    )
    .unwrap_or(0.0);

    (limit, "ПЕСОК".to_string())
}

fn calculate_dep_limit(
    rows: &[LimitRow],
    max_flo_value: Option<f64>,
    max_dep_limit: f64,
    comment: &mut String,
) -> f64 {
    // Sand-spoiled regimes are limited by the sand rule; depression uses clean ones only.
    let dep_rows = rows
        .iter()
        .filter(|row| row.sand <= MAX_SAND && row.dep.is_some() && row.flo.is_some())
        .copied()
        .collect::<Vec<_>>();

    if dep_rows.is_empty() {
        note_dep_on_sandy_regimes(rows, max_dep_limit, comment);
        return max_flo_or_nan(max_flo_value);
    }

    let bad_dep_count = dep_rows
        .iter()
        .filter(|row| row.dep.is_some_and(|dep| dep > max_dep_limit))
        .count();
    if bad_dep_count == 0 {
        note_dep_on_sandy_regimes(rows, max_dep_limit, comment);
        return max_flo_or_nan(max_flo_value);
    }

    // Every regime exceeds the limit: there is no point below it to bracket against, so
    // draw a line through the origin and the lowest-flow regime and read it at the limit.
    if bad_dep_count == dep_rows.len() {
        update_comment_for_all_bad_dep(comment);
        let weakest = dep_rows
            .iter()
            .min_by(|left, right| cmp_f64(left.flo, right.flo))
            .expect("dep_rows is non-empty");
        let (x_up, y_up) = (weakest.dep.unwrap(), weakest.flo.unwrap());
        if x_up.abs() <= f64::EPSILON {
            return 0.0;
        }
        return max_dep_limit * y_up / x_up;
    }

    // A real indicator curve rises with depression. If every regime above the limit flowed
    // less than the well's weakest in-limit regime, the above-limit points are inverted (the
    // well did worse at higher drawdown) — an unstable measurement, not a genuine ceiling.
    // Depression then does not cap the rate: note it in the comment and defer to the sand rule.
    let max_bad_flo = dep_rows
        .iter()
        .filter(|row| row.dep.is_some_and(|dep| dep > max_dep_limit))
        .filter_map(|row| row.flo)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_good_flo = dep_rows
        .iter()
        .filter(|row| row.dep.is_some_and(|dep| dep <= max_dep_limit))
        .filter_map(|row| row.flo)
        .fold(f64::INFINITY, f64::min);
    if max_bad_flo < min_good_flo {
        update_comment_for_dep(comment);
        return max_flo_or_nan(max_flo_value);
    }

    update_comment_for_dep(comment);

    // Upper anchor: lowest-depression regime above the limit. On equal depression take the
    // lowest flow (the well's weakest showing at that depression — a conservative estimate).
    let Some(upper) = dep_rows
        .iter()
        .filter(|row| row.dep.is_some_and(|dep| dep > max_dep_limit))
        .min_by(|left, right| cmp_f64(left.dep, right.dep).then(cmp_f64(left.flo, right.flo)))
    else {
        return max_flo_or_nan(max_flo_value);
    };
    let (x_up, y_up) = (upper.dep.unwrap(), upper.flo.unwrap());

    // Lower anchor: highest-depression regime at/below the limit (envelope: max flow on
    // ties). Use it only if the segment is physical (flow non-decreasing with depression);
    // otherwise anchor at the origin (0, 0).
    let lower = dep_rows
        .iter()
        .filter(|row| row.dep.is_some_and(|dep| dep <= max_dep_limit))
        .max_by(|left, right| cmp_f64(left.dep, right.dep).then(cmp_f64(left.flo, right.flo)));
    let (x_lo, y_lo) = match lower {
        Some(lower) if lower.flo.unwrap() <= y_up => (lower.dep.unwrap(), lower.flo.unwrap()),
        _ => (0.0, 0.0),
    };

    if (x_up - x_lo).abs() <= f64::EPSILON {
        return y_lo;
    }
    y_lo + (max_dep_limit - x_lo) * (y_up - y_lo) / (x_up - x_lo)
}

fn cmp_f64(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    left.unwrap_or(f64::NEG_INFINITY)
        .partial_cmp(&right.unwrap_or(f64::NEG_INFINITY))
        .unwrap_or(std::cmp::Ordering::Equal)
}

// Depression exceeded only on sand-spoiled regimes is bounded by the sand rule, not by
// depression, so it does not move the numeric limit — but it is still reported in the comment.
fn note_dep_on_sandy_regimes(rows: &[LimitRow], max_dep_limit: f64, comment: &mut String) {
    if rows
        .iter()
        .any(|row| row.dep.is_some_and(|dep| dep > max_dep_limit))
    {
        update_comment_for_dep(comment);
    }
}

fn update_comment_for_all_bad_dep(comment: &mut String) {
    *comment = match comment.as_str() {
        "ПЕСОК" => "ДЕПРЕССИЯ НА ВСЕХ РЕЖИМАХ! + ПЕСОК",
        "ПЕСОК НА ВСЕХ РЕЖИМАХ!" => {
            "ДЕПРЕССИЯ НА ВСЕХ РЕЖИМАХ! + ПЕСОК НА ВСЕХ РЕЖИМАХ!"
        }
        "ГДИ" => "ДЕПРЕССИЯ НА ВСЕХ РЕЖИМАХ!",
        other => other,
    }
    .to_string();
}

fn update_comment_for_dep(comment: &mut String) {
    *comment = match comment.as_str() {
        "ПЕСОК" => "ПЕСОК + ДЕПРЕССИЯ",
        "ПЕСОК НА ВСЕХ РЕЖИМАХ!" => "ПЕСОК НА ВСЕХ РЕЖИМАХ! + ДЕПРЕССИЯ",
        "ГДИ" => "ДЕПРЕССИЯ",
        other => other,
    }
    .to_string();
}

fn compare_by_regime_no(left: &LimitRow, right: &LimitRow) -> std::cmp::Ordering {
    match (left.regime_no, right.regime_no) {
        (Some(left), Some(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn max_flo(values: impl Iterator<Item = f64>) -> Option<f64> {
    values.fold(None, |acc, value| {
        Some(acc.map_or(value, |best: f64| best.max(value)))
    })
}

fn max_flo_or_nan(value: Option<f64>) -> f64 {
    value.unwrap_or(f64::NAN)
}

fn round_half_up_to_thousand(value: f64) -> Option<f64> {
    value
        .is_finite()
        .then(|| (value / 1000.0 + 0.5).floor() * 1000.0)
}

fn round_down_to_five_thousand(value: f64) -> f64 {
    let thousands = (value / 1000.0).floor();
    (thousands - thousands % 5.0) * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_gdi_when_sand_and_dep_are_ok() {
        let result = calculate_limits(&[
            input(1.0, 10_000.0, 0.0, 10.0, 50.0),
            input(2.0, 20_000.0, 1.0, 20.0, 50.0),
        ]);

        assert_eq!(result.max_flo, Some(20_000.0));
        assert_eq!(result.dop_skv_flo, Some(20_000.0));
        assert_eq!(result.dop_skv_flo_percent_5, Some(20_000.0));
        assert_eq!(result.dop_skv_flo_095, Some(15_000.0));
        assert_eq!(result.comment, "ГДИ");
    }

    #[test]
    fn limits_by_sand() {
        let result = calculate_limits(&[
            input(1.0, 10_000.0, 0.0, 10.0, 50.0),
            input(2.0, 20_000.0, 3.0, 20.0, 50.0),
            input(3.0, 30_000.0, 4.0, 30.0, 50.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(10_000.0));
        assert_eq!(result.comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_well_1019_trusts_clean_above_earlier_sand() {
        // Скв. 1019: ранний песок на 98300 (режим 2) не связывает — после него скважина
        // чисто выходит выше (101400, режим 6). Связывающий песок — 107500 (режим 7);
        // максимальный чистый дебит ниже него → 101400 (а не 94200).
        let rows = [
            sand_row(1.0, 71_400.0, 0.0),
            sand_row(2.0, 98_300.0, 3.42),
            sand_row(3.0, 69_500.0, 0.0),
            sand_row(4.0, 80_600.0, 0.0),
            sand_row(5.0, 94_200.0, 0.0),
            sand_row(6.0, 101_400.0, 0.0),
            sand_row(7.0, 107_500.0, 6.25),
            sand_row(8.0, 100_500.0, 0.0),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 101_400.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_uses_first_clean_run_after_leading_sand() {
        // Песок на первых режимах (110700, 112100), затем чистый блок 56900/71700,
        // затем снова песок до конца. Берём максимум первого чистого блока → 71700.
        let rows = [
            sand_row(1.0, 110_700.0, 42.28),
            sand_row(2.0, 112_100.0, 17.13),
            sand_row(3.0, 56_900.0, 0.0),
            sand_row(4.0, 71_700.0, 0.0),
            sand_row(5.0, 86_100.0, 5.58),
            sand_row(6.0, 97_500.0, 7.39),
            sand_row(7.0, 104_300.0, 32.22),
            sand_row(8.0, 84_400.0, 6.83),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 71_700.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_takes_trailing_clean_above_earlier_sand() {
        // Ранние песочные режимы (140400, 166200, 180100) перекрыты последующими чистыми
        // выше (180500, 200000) → не связывают. Связывает только 202100 (после него чисто
        // выше не было). Максимальный чистый ниже 202100 → 200000.
        let rows = [
            sand_row(1.0, 164_700.0, 0.0),
            sand_row(2.0, 90_900.0, 0.0),
            sand_row(3.0, 115_800.0, 0.0),
            sand_row(4.0, 140_400.0, 14.36),
            sand_row(5.0, 166_200.0, 15.16),
            sand_row(6.0, 180_100.0, 2.8),
            sand_row(7.0, 202_100.0, 7.48),
            sand_row(8.0, 180_500.0, 1.4),
            sand_row(9.0, 165_500.0, 3.05),
            sand_row(10.0, 200_000.0, 0.0),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 200_000.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_well_209_ignores_leading_cleanup_sand() {
        // Песок на режимах 1–2 (98700, 105500) — начальная промывка до первого чистого
        // блока. min_sandy считается только по песку ПОСЛЕ блока (121500), поэтому 96000
        // (≤ 121500−5000) проходит.
        let rows = [
            sand_row(1.0, 98_700.0, 24.32),
            sand_row(2.0, 105_500.0, 6.82),
            sand_row(3.0, 62_300.0, 0.0),
            sand_row(4.0, 70_500.0, 0.0),
            sand_row(5.0, 86_000.0, 0.0),
            sand_row(6.0, 96_000.0, 1.88),
            sand_row(7.0, 121_500.0, 19.26),
            sand_row(8.0, 135_800.0, 265.08),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 96_000.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn limits_well_215_ignores_zero_flow_regime() {
        // Режим 9 (flo=0, dep=16) игнорируется. Без него тест кончается чистым r8 →
        // «хвостовые чистые» → 35100 (округление вверх до 1000 → 35000).
        let result = calculate_limits(&[
            input(1.0, 76_300.0, 53.48, 2.22, 4.0),
            input(2.0, 31_100.0, 0.0, 1.32, 4.0),
            input(3.0, 33_400.0, 0.0, 1.36, 4.0),
            input(4.0, 34_500.0, 0.0, 1.39, 4.0),
            input(5.0, 37_700.0, 104.95, 1.49, 4.0),
            input(6.0, 48_200.0, 32.87, 1.64, 4.0),
            input(7.0, 31_000.0, 0.0, 1.37, 4.0),
            input(8.0, 35_100.0, 0.0, 1.39, 4.0),
            input(9.0, 0.0, 0.0, 16.0, 4.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(35_000.0));
        assert_eq!(result.comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_ignores_trailing_clean_below_earlier_peak() {
        // Скв. 112: песок на режимах 2 (100600) и 7 (49200). После последнего песка идут
        // чистые 33000/23200, но они не «потолок» — берём максимальный чистый дебит ниже
        // связывающего песка (min_sandy=49200) → 35800, а не хвостовые 33000.
        let rows = [
            sand_row(1.0, 16_800.0, 0.0),
            sand_row(2.0, 100_600.0, 1264.77),
            sand_row(3.0, 26_500.0, 0.0),
            sand_row(4.0, 34_500.0, 0.0),
            sand_row(5.0, 32_200.0, 0.0),
            sand_row(6.0, 35_800.0, 0.0),
            sand_row(7.0, 49_200.0, 503.85),
            sand_row(8.0, 33_000.0, 0.0),
            sand_row(9.0, 23_200.0, 0.0),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 35_800.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn dep_limit_well_422_all_bad_uses_min_flow() {
        // Все режимы превышают депрессию. Якорь (0,0) и режим с минимальным дебитом
        // (31700, 5.94): 4.0 * 31700 / 5.94.
        let rows = [
            dep_row(1.0, 47_800.0, 0.0, 4.95),
            dep_row(2.0, 54_400.0, 0.44, 4.95),
            dep_row(3.0, 40_000.0, 0.0, 4.52),
            dep_row(4.0, 38_000.0, 0.0, 4.95),
            dep_row(5.0, 40_600.0, 0.0, 4.95),
            dep_row(6.0, 34_600.0, 0.0, 5.38),
            dep_row(7.0, 31_700.0, 0.0, 5.94),
        ];
        let mut comment = String::from("ГДИ");
        let limit = calculate_dep_limit(
            &rows,
            max_flo(rows.iter().filter_map(|row| row.flo)),
            4.0,
            &mut comment,
        );

        assert!((limit - 4.0 * 31_700.0 / 5.94).abs() < 1.0, "limit={limit}");
        assert_eq!(comment, "ДЕПРЕССИЯ НА ВСЕХ РЕЖИМАХ!");
    }

    #[test]
    fn sand_limit_well_156_max_clean_below_binding_sand() {
        // min_sandy=37700. Чистые дебиты: 34300, 31200, 38100. Ниже 37700 проходят
        // 34300 и 31200 (38100 отброшен как ≥ min_sandy) → 34300.
        let rows = [
            sand_row(1.0, 34_300.0, 0.0),
            sand_row(2.0, 52_800.0, 45.48),
            sand_row(3.0, 31_200.0, 0.0),
            sand_row(4.0, 38_100.0, 1.89),
            sand_row(5.0, 40_600.0, 8.88),
            sand_row(6.0, 41_000.0, 26.37),
            sand_row(7.0, 53_000.0, 543.75),
            sand_row(8.0, 37_700.0, 4.78),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 34_300.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn sand_limit_well_203_max_clean_below_binding_sand() {
        // min_sandy=144700. Максимальный чистый дебит строго ниже него → 142400
        // (173400/170600 отброшены как ≥ min_sandy).
        let rows = [
            sand_row(1.0, 173_400.0, 0.0),
            sand_row(2.0, 76_100.0, 0.0),
            sand_row(3.0, 98_100.0, 0.0),
            sand_row(4.0, 123_500.0, 0.0),
            sand_row(5.0, 142_400.0, 0.0),
            sand_row(6.0, 170_600.0, 0.0),
            sand_row(7.0, 198_200.0, 36.32),
            sand_row(8.0, 144_700.0, 3.32),
        ];
        let (limit, comment) =
            calculate_sand_limit(&rows, max_flo(rows.iter().filter_map(|row| row.flo)));

        assert_eq!(limit, 142_400.0);
        assert_eq!(comment, "ПЕСОК");
    }

    #[test]
    fn dep_limit_well_105_uses_origin_on_negative_slope() {
        // Нижняя точка (3.66, 28400) даёт убывающий отрезок к (4.18, 24200) → якорь в
        // (0,0): 4.0 * 24200 / 4.18.
        let rows = [
            dep_row(1.0, 28_400.0, 0.0, 3.66),
            dep_row(2.0, 40_300.0, 0.0, 6.09),
            dep_row(3.0, 24_200.0, 0.0, 4.18),
            dep_row(4.0, 28_600.0, 0.0, 4.72),
            dep_row(5.0, 32_100.0, 0.0, 5.24),
            dep_row(6.0, 35_300.0, 0.0, 5.66),
            dep_row(7.0, 34_500.0, 0.0, 6.12),
            dep_row(8.0, 33_200.0, 0.0, 5.84),
        ];
        let mut comment = String::from("ГДИ");
        let limit = calculate_dep_limit(
            &rows,
            max_flo(rows.iter().filter_map(|row| row.flo)),
            4.0,
            &mut comment,
        );

        assert!((limit - 4.0 * 24_200.0 / 4.18).abs() < 1.0, "limit={limit}");
    }

    #[test]
    fn dep_limit_well_218_brackets_clean_points() {
        // Песочный режим 1 (sand 82.85) исключается. Нижняя точка — максимальный по
        // дебиту среди dep≤4 при равной депрессии 1.36: (1.36, 19900); верхняя — (18.21,
        // 21500). Наклон положительный → интерполяция между ними на 4.0.
        let rows = [
            dep_row(1.0, 32_600.0, 82.85, 1.25),
            dep_row(2.0, 15_700.0, 0.0, 1.25),
            dep_row(3.0, 19_000.0, 0.0, 0.93),
            dep_row(4.0, 18_700.0, 0.0, 1.25),
            dep_row(5.0, 19_300.0, 0.0, 1.25),
            dep_row(6.0, 18_400.0, 0.0, 1.25),
            dep_row(7.0, 19_900.0, 0.0, 1.36),
            dep_row(8.0, 21_500.0, 0.0, 18.21),
            dep_row(9.0, 16_200.0, 0.0, 1.36),
        ];
        let mut comment = String::from("ГДИ");
        let limit = calculate_dep_limit(
            &rows,
            max_flo(rows.iter().filter_map(|row| row.flo)),
            4.0,
            &mut comment,
        );

        let expected = 19_900.0 + (4.0 - 1.36) * (21_500.0 - 19_900.0) / (18.21 - 1.36);
        assert!((limit - expected).abs() < 1.0, "limit={limit}");
    }

    #[test]
    fn dep_limit_picks_min_flow_on_equal_depression() {
        // Среди режимов выше лимита при равной депрессии для верхней точки берётся
        // МИНИМАЛЬНЫЙ дебит: верх = (5.0, 20000) [не 30000], низ = (1.0, 10000).
        let rows = [
            dep_row(1.0, 10_000.0, 0.0, 1.0),
            dep_row(2.0, 30_000.0, 0.0, 5.0),
            dep_row(3.0, 20_000.0, 0.0, 5.0),
        ];
        let mut comment = String::from("ГДИ");
        let limit = calculate_dep_limit(
            &rows,
            max_flo(rows.iter().filter_map(|row| row.flo)),
            4.0,
            &mut comment,
        );

        let expected = 10_000.0 + (4.0 - 1.0) * (20_000.0 - 10_000.0) / (5.0 - 1.0);
        assert!((limit - expected).abs() < 1.0, "limit={limit}");
    }

    #[test]
    fn dep_limit_ignores_inverted_lone_high_depression_regime() {
        // Скважина со скриншота: единственный режим выше лимита (R1, dep 4.47) дал 83600 —
        // меньше, чем самый слабый режим в пределах лимита (98200). Индикаторная линия
        // растёт до 166000 при dep 2.0, затем единичная точка при 4.47 «проваливается» —
        // это нестабильный замер, а не потолок. Депрессия не ограничивает: число задаёт
        // песок (188000/166000 — связывающий песок → 166000), депрессия лишь отмечается.
        let result = calculate_limits(&[
            input(1.0, 83_600.0, 0.0, 4.47, 4.0),
            input(2.0, 191_600.0, 12.28, 2.64, 4.0),
            input(3.0, 188_000.0, 5.36, 2.34, 4.0),
            input(4.0, 166_000.0, 1.08, 2.0, 4.0),
            input(5.0, 136_500.0, 0.26, 1.47, 4.0),
            input(6.0, 117_400.0, 0.0, 1.1, 4.0),
            input(7.0, 98_200.0, 0.0, 0.84, 4.0),
            input(8.0, 136_900.0, 0.26, 1.5, 4.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(166_000.0));
        assert_eq!(result.comment, "ПЕСОК + ДЕПРЕССИЯ");
    }

    #[test]
    fn interpolates_dep_limit() {
        let result = calculate_limits(&[
            input(1.0, 10_000.0, 0.0, 10.0, 15.0),
            input(2.0, 20_000.0, 0.0, 20.0, 15.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(15_000.0));
        assert_eq!(result.comment, "ДЕПРЕССИЯ");
    }

    #[test]
    fn limits_by_min_flow_line_when_all_dep_points_are_bad() {
        // Депрессия превышена на всех режимах. Линия через (0,0) и режим с минимальным
        // дебитом (16000 при dep=40), снятая на max_dep=30: 30 * 16000 / 40 = 12000.
        let result = calculate_limits(&[
            input(1.0, 16_000.0, 0.0, 40.0, 30.0),
            input(2.0, 25_000.0, 0.0, 50.0, 30.0),
            input(3.0, 36_000.0, 0.0, 60.0, 30.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(12_000.0));
        assert_eq!(result.comment, "ДЕПРЕССИЯ НА ВСЕХ РЕЖИМАХ!");
    }

    #[test]
    fn comment_notes_depression_exceeded_only_on_sandy_regimes() {
        // Скв. 74: депрессия превышена (4.24, 6.18) только на песочных режимах. Песок их
        // и так ограничивает, поэтому число не меняется (дебит по песку = 25700 → 26000),
        // но в комментарии депрессия отмечается: «ПЕСОК + ДЕПРЕССИЯ».
        let result = calculate_limits(&[
            input(1.0, 78_600.0, 274.96, 4.24, 4.0),
            input(2.0, 78_700.0, 457.61, 6.18, 4.0),
            input(3.0, 44_300.0, 105.72, 2.83, 4.0),
            input(4.0, 58_100.0, 154.92, 3.82, 4.0),
            input(5.0, 32_300.0, 16.7, 1.27, 4.0),
            input(6.0, 25_700.0, 0.0, 1.25, 4.0),
            input(7.0, 18_900.0, 0.0, 0.88, 4.0),
            input(8.0, 46_100.0, 117.03, 2.5, 4.0),
        ]);

        assert_eq!(result.dop_skv_flo, Some(26_000.0));
        assert_eq!(result.comment, "ПЕСОК + ДЕПРЕССИЯ");
    }

    #[test]
    fn calculates_jones_coefficients() {
        let expected_a = 8.0;
        let expected_b = 3.0;
        let result = calculate_jones_coefficients(&[
            jones_input(1.0, expected_a, expected_b),
            jones_input(2.0, expected_a, expected_b),
            jones_input(3.0, expected_a, expected_b),
        ]);

        assert!((result.a.unwrap() - expected_a).abs() < 1e-9);
        assert!((result.b.unwrap() - expected_b).abs() < 1e-9);
    }

    #[test]
    fn keeps_jones_coefficients_non_negative() {
        let result = calculate_jones_coefficients(&[
            JonesInput {
                flo: Some(1_000_000.0),
                thp: Some(5.0),
                ppl: Some(10.0),
                bhp: Some(9.0),
            },
            JonesInput {
                flo: Some(2_000_000.0),
                thp: Some(5.0),
                ppl: Some(10.0),
                bhp: Some(9.5),
            },
        ]);

        assert!(result.a.unwrap() >= 0.0);
        assert!(result.b.unwrap() >= 0.0);
    }

    #[test]
    fn filters_jones_coefficients_by_pressure_order() {
        let expected_a = 8.0;
        let expected_b = 3.0;
        let invalid_high_thp = JonesInput {
            thp: Some(105.0),
            ..jones_input(1.0, 800.0, 300.0)
        };
        let invalid_bhp_above_ppl = JonesInput {
            ppl: Some(110.0),
            bhp: Some(120.0),
            ..jones_input(2.0, 800.0, 300.0)
        };

        let result = calculate_jones_coefficients(&[
            invalid_high_thp,
            jones_input(1.0, expected_a, expected_b),
            invalid_bhp_above_ppl,
            jones_input(2.0, expected_a, expected_b),
            jones_input(3.0, expected_a, expected_b),
        ]);

        assert!((result.a.unwrap() - expected_a).abs() < 1e-9);
        assert!((result.b.unwrap() - expected_b).abs() < 1e-9);
    }

    fn sand_row(regime_no: f64, flo: f64, sand: f64) -> LimitRow {
        LimitRow {
            regime_no: Some(regime_no),
            flo: Some(flo),
            sand,
            dep: None,
            max_dep: None,
        }
    }

    fn dep_row(regime_no: f64, flo: f64, sand: f64, dep: f64) -> LimitRow {
        LimitRow {
            regime_no: Some(regime_no),
            flo: Some(flo),
            sand,
            dep: Some(dep),
            max_dep: Some(4.0),
        }
    }

    fn input(regime_no: f64, flo: f64, sand: f64, dep: f64, max_dep: f64) -> LimitInput {
        LimitInput {
            regime_no: Some(regime_no),
            flo: Some(flo),
            sand: Some(sand),
            dep: Some(dep),
            max_dep: Some(max_dep),
        }
    }

    fn jones_input(q_mln: f64, a: f64, b: f64) -> JonesInput {
        let bhp_bar = 100.0 * BAR_PER_KGF_CM2;
        let pressure_delta_squared = a * q_mln * q_mln + b * q_mln;
        let ppl_bar = (bhp_bar * bhp_bar + pressure_delta_squared).sqrt();
        JonesInput {
            flo: Some(q_mln * 1_000_000.0),
            thp: Some(50.0),
            ppl: Some(ppl_bar / BAR_PER_KGF_CM2),
            bhp: Some(100.0),
        }
    }
}
