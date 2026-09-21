//! Candidate path builders for source DBF/XLSX locations.

use std::path::PathBuf;

use anyhow::Result;

use super::resolve::home_dir;

pub const DEBUG_DIR: &str = "/home/yser/debug";

pub(super) fn debug_bdasu_file(name: &str) -> PathBuf {
    PathBuf::from(DEBUG_DIR).join(name)
}

/// Candidate locations for the shared wells workbook used by several modes.
pub fn skvgdn_candidates(debug_mode: bool) -> Result<Vec<PathBuf>> {
    if debug_mode {
        return Ok(vec![debug_bdasu_file("СкважиныГДН.xlsx")]);
    }

    let home = home_dir()?;
    Ok(vec![
        home.join("mnt")
            .join("ITC")
            .join("СРМиГРР")
            .join("Персональная")
            .join("ЛАБОРАТОРИЯ МЭМ")
            .join("СкважиныГДН.xlsx"),
        PathBuf::from(r"Y:\ЛАБОРАТОРИЯ МЭМ\СкважиныГДН.xlsx"),
        home.join("mnt")
            .join("ITC")
            .join("СРМиГРР")
            .join("Персональная")
            .join("ЛАБОРАТОРИЯ МЭМ")
            .join("модели tNavigator")
            .join("Дизайнер сетей")
            .join("Адаптированные модели")
            .join("Скрипт_режимный лист")
            .join("tNavigator_scripts")
            .join("шаблоныМесторождений")
            .join("СкважиныГДН.xlsx"),
        PathBuf::from(r"Y:\ЛАБОРАТОРИЯ МЭМ\СкважиныГДН.xlsx"),
        PathBuf::from(r"C:\GIS\tNavigator_scripts\шаблоныМесторождений\СкважиныГДН.xlsx"),
    ])
}

pub(super) fn medbegie_dir_candidates(test_mode: bool, dbf_name: &str) -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    let suffix = if test_mode { Some("test") } else { None };

    let mut out = Vec::new();
    for root in [
        home.join("mnt").join("centr"),
        home.join("mnt").join("Centr"),
    ] {
        let mut p = root.join("ОРМ").join("IUS_Geolog").join("Medbegie");
        if let Some(s) = suffix {
            p = p.join(s);
        }
        out.push(p.join(dbf_name));
    }

    out.push(if test_mode {
        PathBuf::from(format!(
            r"\\nadym-dobycha.gazprom.ru\gdn\CENTR\ОРМ\IUS_Geolog\Medbegie\test\{dbf_name}"
        ))
    } else {
        PathBuf::from(format!(
            r"\\nadym-dobycha.gazprom.ru\gdn\CENTR\ОРМ\IUS_Geolog\Medbegie\{dbf_name}"
        ))
    });

    Ok(out)
}

pub(super) fn bdasu_dir_candidates(test_mode: bool, dbf_name: &str) -> Result<Vec<PathBuf>> {
    let home = home_dir()?;
    let mut out = Vec::new();
    let mut linux = home
        .join("mnt")
        .join("ITC")
        .join("СРМиГРР")
        .join("Персональная")
        .join("ЛАБОРАТОРИЯ КПРМ")
        .join("BD_ASU");
    if test_mode {
        linux = linux.join("test");
    }
    out.push(linux.join(dbf_name));

    out.push(if test_mode {
        PathBuf::from(format!(r"Y:\ЛАБОРАТОРИЯ КПРМ\BD_ASU\test\{dbf_name}"))
    } else {
        PathBuf::from(format!(r"Y:\ЛАБОРАТОРИЯ КПРМ\BD_ASU\{dbf_name}"))
    });

    Ok(out)
}
