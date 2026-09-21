//! Data structures and pure helpers for the `SS` report.

use std::sync::Arc;

use crate::domain::Mest;

#[derive(Debug, Clone)]
pub struct Request {
    pub mests: Vec<Mest>,
    pub year: i32,
    pub month: u32,
    pub test_mode: bool,
    pub debug_mode: bool,
    pub only_last: bool,
}

#[derive(Debug, Clone)]
pub(super) struct WellRow {
    pub(super) gp: Option<Arc<str>>,
    pub(super) tkr: Option<f64>,
    pub(super) pkr: Option<f64>,
}

#[derive(Debug, Clone)]
pub(super) struct OutputRow {
    pub(super) date: i64,
    pub(super) gp: Option<Arc<str>>,
    pub(super) well: i64,
    pub(super) rbuf_ata: Option<f64>,
    pub(super) rzat_ata: Option<f64>,
    pub(super) depr_ata: Option<f64>,
    pub(super) rmk_ata: Option<f64>,
    pub(super) rshl_ata: Option<f64>,
    pub(super) tus_c: Option<f64>,
    pub(super) rvh_ata: Option<f64>,
    pub(super) tvh_c: Option<f64>,
    pub(super) q_tmsut: Option<f64>,
    pub(super) speed: f64,
    pub(super) q_water: Option<f64>,
    pub(super) q_sand: Option<f64>,
    pub(super) udk_mm: Option<f64>,
    pub(super) washer_mm: Option<f64>,
    pub(super) smzd: Option<f64>,
}

pub(super) fn get_z(p: f64, t: f64, tkr: f64, pkr: f64) -> f64 {
    let tpr = (t + 273.15) / tkr;
    let ppr = (p / 10.197) / pkr;
    1.0 - 0.01 * (0.76 * tpr.powi(3) - 9.39 * tpr + 13.0) * ppr * (8.0 - ppr) - 0.004
}

pub(super) fn get_vyst(p_in: f64, t_in: f64, z: f64, q: f64, d: f64) -> Option<f64> {
    let p = p_in / 10.197;
    if p == 0.0 {
        return None;
    }
    let t = t_in + 273.15;
    let t0 = 293.15;
    let p0 = 0.1013;
    let value = (q * 4.0 * p0 * z * t) / (std::f64::consts::PI * d * d * p * t0 * 86.4);
    Some((value * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::{get_vyst, get_z};

    #[test]
    fn z_factor_regression() {
        let z = get_z(10.0, 20.0, 190.0, 4.6);
        assert!((z - 0.9743588360379184).abs() < 1e-12, "z = {z}");
    }

    #[test]
    fn vyst_speed_regression_and_zero_pressure() {
        assert_eq!(get_vyst(10.0, 20.0, 0.9, 100.0, 0.1), Some(13.7));
        assert_eq!(get_vyst(0.0, 20.0, 0.9, 100.0, 0.1), None);
    }
}
