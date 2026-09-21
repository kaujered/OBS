//! Centralized path resolution for input DBF/XLSX sources and output folders.
//!
//! This module is the best entrypoint when you need to answer:
//! - "где лежат обязательные DBF?";
//! - "какие пути допустимы в test/debug режимах";
//! - "как резолвится входной Excel?"

mod candidates;
mod models;
mod resolve;

pub use candidates::{DEBUG_DIR, skvgdn_candidates};
pub use models::{GdiPaths, GdmPaths, PplPaths, SsPaths, TelemetryPaths, VedPaths};
pub(crate) use resolve::format_candidate_paths;
pub use resolve::{
    OutputGroup, ensure_parent_dir, gis_output_dir, preferred_result_dirs, replicate_output_files,
    report_output_dir, shared_db_dir,
};

use std::path::PathBuf;

use anyhow::Result;

use self::resolve::{
    available_additional_output_dir, first_existing_dir, home_dir, resolve_dbf, resolve_existing,
    resolve_existing_dir,
};

/// Выгрузки ГДМ живут в домашней папке пользователя одинаково на всех ОС:
/// каталог `GDM` наполняет тНавигатор, `GDM/result` создаётся при записи.
pub fn resolve_gdm_paths() -> Result<GdmPaths> {
    let root = home_dir()?.join("tNavigator_scripts").join("GDM");
    Ok(GdmPaths {
        link: root.join("справочник.xlsx"),
        result_dir: root.join("result"),
        root,
    })
}

pub fn resolve_ppl_paths(test_mode: bool, debug_mode: bool) -> Result<PplPaths> {
    Ok(PplPaths {
        plast_dbf: resolve_dbf("plast.dbf", test_mode, debug_mode)?,
        wells_xlsx: resolve_existing("СкважиныГДН.xlsx", &skvgdn_candidates(debug_mode)?)?,
    })
}

pub fn resolve_telemetry_paths(year: i32) -> Result<TelemetryPaths> {
    let home = home_dir()?;
    let year_folder = format!("СВОДКИ ежедневные за {year}г");

    // Порядок важен: сначала текущее монтирование шары, затем прежние — на
    // машинах, где ещё живёт старая схема, режим продолжает работать.
    let mut roots = Vec::new();
    if !cfg!(windows) {
        // Шара монтируется в сессионный каталог пользователя: у каждого свой uid,
        // но путь всегда совпадает с $XDG_RUNTIME_DIR.
        if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            roots.push(
                PathBuf::from(runtime_dir)
                    .join("fly-fm-vfs")
                    .join("smb")
                    .join("share-filials.nadym-dobycha.gazprom.ru")
                    .join("jgpu")
                    .join("ПДС")
                    .join("Общая")
                    .join("Ежедневные СВОДКИ"),
            );
        }
    }
    if cfg!(windows) {
        roots.push(PathBuf::from(
            r"\\nadym-dobycha.gazprom.ru\gdn\ЯГПУ\ПДС\Общая\Ежедневные СВОДКИ",
        ));
    } else {
        roots.push(
            home.join("mnt")
                .join("GDN")
                .join("ЯГПУ")
                .join("ПДС")
                .join("Общая")
                .join("Ежедневные СВОДКИ"),
        );
    }

    let source_candidates = roots
        .iter()
        .flat_map(|root| [root.join(&year_folder), root.clone()])
        .collect::<Vec<_>>();

    let output_dir = shared_db_dir("5_ТЕЛЕМЕТРИЯ")
        .ok_or_else(|| anyhow::anyhow!("Не удалось определить папку «5 БД/5_ТЕЛЕМЕТРИЯ»"))?;

    Ok(TelemetryPaths {
        source_dir: resolve_existing_dir("telemetry source", &source_candidates)?,
        output_dir,
    })
}

pub fn resolve_ss_paths(test_mode: bool, debug_mode: bool) -> Result<SsPaths> {
    Ok(SsPaths {
        eer1_dbf: resolve_dbf("eer1.dbf", test_mode, debug_mode)?,
        wells_xlsx: resolve_existing("СкважиныГДН.xlsx", &skvgdn_candidates(debug_mode)?)?,
    })
}

pub fn resolve_gdi_paths(test_mode: bool, debug_mode: bool) -> Result<GdiPaths> {
    Ok(GdiPaths {
        stand2_dbf: resolve_dbf("stand2.dbf", test_mode, debug_mode)?,
        ksmest_dbf: resolve_dbf("ksmest.dbf", test_mode, debug_mode)?,
        plast_dbf: resolve_dbf("plast.dbf", test_mode, debug_mode)?,
        stand1_dbf: resolve_dbf("stand1.dbf", test_mode, debug_mode)?,
        wells_xlsx: resolve_existing("СкважиныГДН.xlsx", &skvgdn_candidates(debug_mode)?)?,
    })
}

pub fn resolve_ved_paths(test_mode: bool, debug_mode: bool) -> Result<VedPaths> {
    if debug_mode {
        let debug_root = PathBuf::from(DEBUG_DIR);
        let regime_dir = debug_root.join("Режимный лист");
        return Ok(VedPaths {
            ss_dbf: resolve_dbf("eer1.dbf", test_mode, debug_mode)?,
            ppl_dbf: resolve_dbf("plast.dbf", test_mode, debug_mode)?,
            wells_xlsx: resolve_existing("СкважиныГДН.xlsx", &skvgdn_candidates(debug_mode)?)?,
            templates_dir: resolve_existing_dir("ved templates", &[debug_root.join("Шаблоны")])?,
            output_dir: debug_root.join("Ведомость"),
            regime_dir: regime_dir.is_dir().then_some(regime_dir),
        });
    }

    let home = home_dir()?;

    let template_candidates = if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\GIS\Ведомость\Шаблоны"),
            PathBuf::from(r"C:\GIS\Ведомости техрежима\Шаблоны"),
            PathBuf::from(r"C:\GIS\Ведомости техрежима"),
        ]
    } else {
        vec![
            home.join("tNavigator_scripts")
                .join("Ведомость")
                .join("Шаблоны"),
            home.join("tNavigator_scripts")
                .join("Ведомости техрежима")
                .join("Шаблоны"),
            home.join("tNavigator_scripts").join("Ведомости техрежима"),
        ]
    };

    let output_candidates = if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\GIS\Ведомость"),
            PathBuf::from(r"C:\GIS\Ведомости техрежима"),
        ]
    } else {
        vec![
            home.join("tNavigator_scripts").join("Ведомость"),
            home.join("tNavigator_scripts").join("Ведомости техрежима"),
        ]
    };

    Ok(VedPaths {
        ss_dbf: resolve_dbf("eer1.dbf", test_mode, debug_mode)?,
        ppl_dbf: resolve_dbf("plast.dbf", test_mode, debug_mode)?,
        wells_xlsx: resolve_existing("СкважиныГДН.xlsx", &skvgdn_candidates(debug_mode)?)?,
        templates_dir: resolve_existing_dir("ved templates", &template_candidates)?,
        output_dir: first_existing_dir(&output_candidates)
            .unwrap_or_else(|| output_candidates[0].clone()),
        regime_dir: available_additional_output_dir(OutputGroup::RegimeSheet),
    })
}
