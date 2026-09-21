//! Дисковый кэш разобранных строк KOTS в папке шаблонов: книги прошлых лет
//! не меняются, и их повторный разбор при каждой выгрузке — потеря времени.
//! Кэш привязан к mtime и размеру исходника и СкважиныГДН: при несовпадении
//! или отсутствии кэша строки разбираются заново и кэш перезаписывается.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use super::data::OutputRow;
use crate::tabular::cache::file_stamp;

/// Версия формата: сменить при изменении структуры OutputRow.
const MAGIC: &[u8; 8] = b"GDIKOTS1";

pub(super) fn load_rows_cached(
    cache_dir: &Path,
    cache_name: &str,
    source: &Path,
    wells_path: &Path,
    parse: impl FnOnce() -> Result<Vec<OutputRow>>,
) -> Result<Vec<OutputRow>> {
    let cache_path = cache_dir.join(format!("{cache_name}.bin"));
    let source_stamp = file_stamp(source).map(encode_stamp);
    let wells_stamp = file_stamp(wells_path).map(encode_stamp);

    if let (Some(source_stamp), Some(wells_stamp)) = (source_stamp, wells_stamp)
        && let Ok(bytes) = std::fs::read(&cache_path)
        && let Some(rows) = decode(&bytes, source_stamp, wells_stamp)
    {
        return Ok(rows);
    }

    let rows = parse()?;
    // Ошибка записи кэша не мешает выгрузке.
    if let (Some(source_stamp), Some(wells_stamp)) = (source_stamp, wells_stamp)
        && std::fs::create_dir_all(cache_dir).is_ok()
    {
        let _ = std::fs::write(&cache_path, encode(source_stamp, wells_stamp, &rows));
    }
    Ok(rows)
}

type Stamp = (u64, u32, u64);

fn encode_stamp((mtime, len): (SystemTime, u64)) -> Stamp {
    let since_epoch = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    (since_epoch.as_secs(), since_epoch.subsec_nanos(), len)
}

fn encode(source_stamp: Stamp, wells_stamp: Stamp, rows: &[OutputRow]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64 + rows.len() * 160);
    buf.extend_from_slice(MAGIC);
    put_stamp(&mut buf, source_stamp);
    put_stamp(&mut buf, wells_stamp);
    buf.extend_from_slice(&(rows.len() as u32).to_le_bytes());
    for row in rows {
        put_opt_str(&mut buf, row.gp.as_deref());
        put_opt_str(&mut buf, row.plast.as_deref());
        put_opt_str(&mut buf, row.regime_no.as_deref());
        buf.extend_from_slice(&row.well.to_le_bytes());
        buf.extend_from_slice(&row.date_key.to_le_bytes());
        for value in [
            row.washer,
            row.thp,
            row.flo,
            row.gauge,
            row.bhp,
            row.pst,
            row.ppl,
            row.tpl,
            row.water,
            row.sand,
            row.dep,
            row.max_dep,
            row.c,
            row.n,
            row.max_flo,
            row.dop_skv_flo,
            row.dop_skv_flo_percent_5,
            row.dop_skv_flo_095,
            row.jones_a,
            row.jones_b,
        ] {
            put_opt_f64(&mut buf, value);
        }
        put_opt_str(&mut buf, row.limit_comment.as_deref());
    }
    buf
}

fn decode(bytes: &[u8], source_stamp: Stamp, wells_stamp: Stamp) -> Option<Vec<OutputRow>> {
    let mut reader = Reader { bytes, pos: 0 };
    if reader.take(MAGIC.len())? != MAGIC {
        return None;
    }
    if reader.stamp()? != source_stamp || reader.stamp()? != wells_stamp {
        return None;
    }
    let count = reader.u32()? as usize;
    let mut rows = Vec::with_capacity(count);
    for _ in 0..count {
        let gp = reader.opt_str()?.map(Arc::from);
        let plast = reader.opt_str()?.map(Arc::from);
        let regime_no = reader.opt_str()?;
        let well = reader.i64()?;
        let date_key = reader.i64()?;
        let mut values = [None; 20];
        for value in &mut values {
            *value = reader.opt_f64()?;
        }
        let [
            washer,
            thp,
            flo,
            gauge,
            bhp,
            pst,
            ppl,
            tpl,
            water,
            sand,
            dep,
            max_dep,
            c,
            n,
            max_flo,
            dop_skv_flo,
            dop_skv_flo_percent_5,
            dop_skv_flo_095,
            jones_a,
            jones_b,
        ] = values;
        let limit_comment = reader.opt_str()?;
        rows.push(OutputRow {
            gp,
            plast,
            regime_no,
            washer,
            well,
            date_key,
            thp,
            flo,
            gauge,
            bhp,
            pst,
            ppl,
            tpl,
            water,
            sand,
            dep,
            max_dep,
            c,
            n,
            max_flo,
            dop_skv_flo,
            dop_skv_flo_percent_5,
            dop_skv_flo_095,
            limit_comment,
            jones_a,
            jones_b,
        });
    }
    // хвостовой мусор — признак повреждения
    (reader.pos == bytes.len()).then_some(rows)
}

fn put_stamp(buf: &mut Vec<u8>, (secs, nanos, len): Stamp) {
    buf.extend_from_slice(&secs.to_le_bytes());
    buf.extend_from_slice(&nanos.to_le_bytes());
    buf.extend_from_slice(&len.to_le_bytes());
}

fn put_opt_f64(buf: &mut Vec<u8>, value: Option<f64>) {
    match value {
        Some(value) => {
            buf.push(1);
            buf.extend_from_slice(&value.to_le_bytes());
        }
        None => buf.push(0),
    }
}

fn put_opt_str(buf: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            buf.push(1);
            buf.extend_from_slice(&(value.len() as u32).to_le_bytes());
            buf.extend_from_slice(value.as_bytes());
        }
        None => buf.push(0),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let taken = self.bytes.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(taken)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    fn stamp(&mut self) -> Option<Stamp> {
        Some((self.u64()?, self.u32()?, self.u64()?))
    }

    fn opt_f64(&mut self) -> Option<Option<f64>> {
        match self.take(1)?[0] {
            0 => Some(None),
            1 => Some(Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))),
            _ => None,
        }
    }

    fn opt_str(&mut self) -> Option<Option<String>> {
        match self.take(1)?[0] {
            0 => Some(None),
            1 => {
                let len = self.u32()? as usize;
                let text = std::str::from_utf8(self.take(len)?).ok()?;
                Some(Some(text.to_string()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> OutputRow {
        OutputRow {
            gp: Some(Arc::from("ГП-1")),
            plast: None,
            regime_no: Some("3-а".to_string()),
            washer: Some(12.5),
            well: 1042,
            date_key: 20250115,
            thp: None,
            flo: Some(250_000.0),
            gauge: Some(0.75),
            bhp: None,
            pst: Some(6.4),
            ppl: Some(7.1),
            tpl: None,
            water: Some(0.0),
            sand: None,
            dep: Some(0.7),
            max_dep: None,
            c: Some(1.25),
            n: Some(0.55),
            max_flo: None,
            dop_skv_flo: Some(300.0),
            dop_skv_flo_percent_5: None,
            dop_skv_flo_095: Some(285.0),
            limit_comment: Some("огранич.".to_string()),
            jones_a: None,
            jones_b: Some(0.002),
        }
    }

    fn rows_equal(left: &OutputRow, right: &OutputRow) -> bool {
        left.gp == right.gp
            && left.plast == right.plast
            && left.regime_no == right.regime_no
            && left.well == right.well
            && left.date_key == right.date_key
            && left.washer == right.washer
            && left.flo == right.flo
            && left.limit_comment == right.limit_comment
            && left.jones_b == right.jones_b
            && left.dop_skv_flo_095 == right.dop_skv_flo_095
    }

    #[test]
    fn roundtrip_preserves_rows_and_checks_stamps() {
        let rows = vec![sample_row()];
        let source = (1_700_000_000, 123, 5000);
        let wells = (1_600_000_000, 7, 900);
        let bytes = encode(source, wells, &rows);

        let decoded = decode(&bytes, source, wells).expect("кэш валиден");
        assert_eq!(decoded.len(), 1);
        assert!(rows_equal(&decoded[0], &rows[0]));

        // другой штамп источника или скважин — кэш невалиден
        assert!(decode(&bytes, (1, 2, 3), wells).is_none());
        assert!(decode(&bytes, source, (1, 2, 3)).is_none());
        // повреждённый хвост — тоже
        let mut broken = bytes.clone();
        broken.push(0);
        assert!(decode(&broken, source, wells).is_none());
    }
}
