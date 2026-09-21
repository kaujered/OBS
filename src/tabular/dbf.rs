//! Shared DBF readers and field converters.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use rayon::prelude::*;

use super::number::parse_loose_f64;

const DBF_HEADER_LEN: usize = 32;
const DBF_DESCRIPTOR_LEN: usize = 32;
const DBF_DELETED_FLAG: u8 = b'*';
/// Размер чанка обратного чтения.
const DBF_TAIL_CHUNK_BYTES: usize = 8 * 1024 * 1024;
/// Сколько чанков дочитывается после чанка без релевантных записей —
/// страховка от локальных нарушений порядка дат в файле.
const DBF_TAIL_GRACE_CHUNKS: usize = 2;

/// Быстрый ридер DBF для больших файлов: заголовок разбирается один раз,
/// записи фиксированной ширины декодируются параллельно и только по
/// запрошенным полям. `dbase::Reader` декодирует каждое поле каждой записи —
/// на сводках в сотни мегабайт это на порядок медленнее.
pub(crate) struct DbfTable {
    file: std::fs::File,
    fields: Vec<(String, DbfField)>,
    records_start: u64,
    record_len: usize,
    record_count: usize,
}

/// Смещение поля внутри записи.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DbfField {
    offset: usize,
    len: usize,
    is_character: bool,
}

/// Сырые байты одной записи (байт 0 — флаг удаления).
pub(crate) struct DbfRecordView<'a> {
    bytes: &'a [u8],
}

impl DbfTable {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("Не удалось открыть {}", path.display()))?;
        let file_len = file.metadata()?.len();

        let mut head = [0u8; DBF_HEADER_LEN];
        file.read_exact(&mut head)
            .with_context(|| format!("DBF короче заголовка: {}", path.display()))?;
        let header_count = u32::from_le_bytes(head[4..8].try_into().expect("len 4")) as usize;
        let records_start = u16::from_le_bytes(head[8..10].try_into().expect("len 2")) as usize;
        let record_len = u16::from_le_bytes(head[10..12].try_into().expect("len 2")) as usize;
        ensure!(
            record_len > 0 && records_start >= DBF_HEADER_LEN && records_start as u64 <= file_len,
            "Некорректный заголовок DBF: {}",
            path.display()
        );

        let mut descriptors = vec![0u8; records_start - DBF_HEADER_LEN];
        file.read_exact(&mut descriptors)?;
        let mut fields = Vec::new();
        // Байт 0 записи — флаг удаления, поля идут следом подряд.
        let mut offset = 1usize;
        let mut pos = 0usize;
        while pos + DBF_DESCRIPTOR_LEN <= descriptors.len() && descriptors[pos] != 0x0D {
            let descriptor = &descriptors[pos..pos + DBF_DESCRIPTOR_LEN];
            let name_len = descriptor[..11]
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(11);
            let name = String::from_utf8_lossy(&descriptor[..name_len])
                .trim()
                .to_string();
            let len = descriptor[16] as usize;
            ensure!(
                offset + len <= record_len,
                "Поля DBF шире записи: {}",
                path.display()
            );
            fields.push((
                name,
                DbfField {
                    offset,
                    len,
                    is_character: descriptor[11] == b'C',
                },
            ));
            offset += len;
            pos += DBF_DESCRIPTOR_LEN;
        }

        let record_count =
            header_count.min(((file_len - records_start as u64) / record_len as u64) as usize);
        Ok(Self {
            file,
            fields,
            records_start: records_start as u64,
            record_len,
            record_count,
        })
    }

    pub(crate) fn field(&self, name: &str) -> Result<DbfField> {
        self.fields
            .iter()
            .find(|(field_name, _)| field_name == name)
            .map(|(_, field)| *field)
            .with_context(|| format!("В DBF нет поля {name}"))
    }

    /// Все неудалённые записи файла, разбор параллельный.
    pub(crate) fn par_filter_map<T, F>(&mut self, map: F) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(&DbfRecordView<'_>) -> Option<T> + Sync,
    {
        let mut buffer = vec![0u8; self.record_count * self.record_len];
        self.file.seek(SeekFrom::Start(self.records_start))?;
        self.file.read_exact(&mut buffer)?;
        Ok(par_map_chunk(&buffer, self.record_len, map))
    }

    /// Чтение с конца файла: свежие записи в DBF дописываются в хвост,
    /// поэтому чанки обрабатываются от конца к началу, и чтение
    /// останавливается, когда несколько чанков подряд не содержат ни одной
    /// записи с `is_relevant`. Порядок результата — как в файле.
    pub(crate) fn par_filter_map_from_end<T, F, R>(
        &mut self,
        is_relevant: R,
        map: F,
    ) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(&DbfRecordView<'_>) -> Option<T> + Sync,
        R: Fn(&DbfRecordView<'_>) -> bool + Sync,
    {
        let chunk_records = (DBF_TAIL_CHUNK_BYTES / self.record_len).max(1);
        self.par_filter_map_from_end_chunked(chunk_records, is_relevant, map)
    }

    fn par_filter_map_from_end_chunked<T, F, R>(
        &mut self,
        chunk_records: usize,
        is_relevant: R,
        map: F,
    ) -> Result<Vec<T>>
    where
        T: Send,
        F: Fn(&DbfRecordView<'_>) -> Option<T> + Sync,
        R: Fn(&DbfRecordView<'_>) -> bool + Sync,
    {
        let mut chunks = Vec::new();
        let mut irrelevant_run = 0usize;
        let mut end = self.record_count;
        let mut buffer = vec![0u8; chunk_records * self.record_len];

        while end > 0 {
            let start = end.saturating_sub(chunk_records);
            let bytes = &mut buffer[..(end - start) * self.record_len];
            self.file.seek(SeekFrom::Start(
                self.records_start + (start * self.record_len) as u64,
            ))?;
            self.file.read_exact(bytes)?;

            let parsed: Vec<(Option<T>, bool)> = bytes
                .par_chunks_exact(self.record_len)
                .map(|record| {
                    if record[0] == DBF_DELETED_FLAG {
                        return (None, false);
                    }
                    let view = DbfRecordView { bytes: record };
                    (map(&view), is_relevant(&view))
                })
                .collect();
            let relevant = parsed.iter().any(|(_, relevant)| *relevant);
            chunks.push(
                parsed
                    .into_iter()
                    .filter_map(|(value, _)| value)
                    .collect::<Vec<_>>(),
            );

            if relevant {
                irrelevant_run = 0;
            } else {
                irrelevant_run += 1;
                if irrelevant_run > DBF_TAIL_GRACE_CHUNKS {
                    break;
                }
            }
            end = start;
        }

        chunks.reverse();
        Ok(chunks.into_iter().flatten().collect())
    }
}

fn par_map_chunk<T, F>(bytes: &[u8], record_len: usize, map: F) -> Vec<T>
where
    T: Send,
    F: Fn(&DbfRecordView<'_>) -> Option<T> + Sync,
{
    bytes
        .par_chunks_exact(record_len)
        .filter_map(|record| {
            if record[0] == DBF_DELETED_FLAG {
                return None;
            }
            map(&DbfRecordView { bytes: record })
        })
        .collect()
}

impl DbfRecordView<'_> {
    fn trimmed(&self, field: DbfField) -> &[u8] {
        let mut bytes = &self.bytes[field.offset..field.offset + field.len];
        while let [b' ' | 0, rest @ ..] = bytes {
            bytes = rest;
        }
        while let [rest @ .., b' ' | 0] = bytes {
            bytes = rest;
        }
        bytes
    }

    pub(crate) fn f64(&self, field: DbfField) -> Option<f64> {
        let text = std::str::from_utf8(self.trimmed(field)).ok()?;
        parse_loose_f64(text)
    }

    pub(crate) fn i64(&self, field: DbfField) -> Option<i64> {
        self.f64(field).map(|value| value.round() as i64)
    }

    pub(crate) fn i32(&self, field: DbfField) -> Option<i32> {
        self.i64(field).and_then(|value| i32::try_from(value).ok())
    }

    /// Номер скважины: для текстовых полей — первый блок ASCII-цифр
    /// (суффиксы вроде «123А» отбрасываются независимо от кодировки),
    /// для числовых — округлённое целое.
    pub(crate) fn well_i64(&self, field: DbfField) -> Option<i64> {
        if field.is_character {
            leading_digits_i64(&self.bytes[field.offset..field.offset + field.len])
        } else {
            self.i64(field)
        }
    }
}

fn leading_digits_i64(bytes: &[u8]) -> Option<i64> {
    let mut parsed: Option<i64> = None;
    for &byte in bytes {
        if byte.is_ascii_digit() {
            let digit = i64::from(byte - b'0');
            parsed = Some(parsed.unwrap_or(0).checked_mul(10)?.checked_add(digit)?);
        } else if parsed.is_some() {
            break;
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbase::{FieldValue, TableWriterBuilder};

    fn write_test_dbf(path: &Path, rows: &[(&str, i32, i64, Option<f64>)]) {
        let mut writer = TableWriterBuilder::new()
            .add_character_field("WELL".try_into().unwrap(), 10)
            .add_numeric_field("CODE".try_into().unwrap(), 3, 0)
            .add_numeric_field("DT".try_into().unwrap(), 8, 0)
            .add_numeric_field("VAL".try_into().unwrap(), 12, 3)
            .build_with_file_dest(path)
            .unwrap();
        for &(well, code, date, value) in rows {
            let mut record = dbase::Record::default();
            record.insert("WELL".into(), FieldValue::Character(Some(well.into())));
            record.insert("CODE".into(), FieldValue::Numeric(Some(f64::from(code))));
            record.insert("DT".into(), FieldValue::Numeric(Some(date as f64)));
            record.insert("VAL".into(), FieldValue::Numeric(value));
            writer.write_record(&record).unwrap();
        }
    }

    fn test_rows() -> Vec<(&'static str, i32, i64, Option<f64>)> {
        vec![
            ("099", 1, 202511, Some(9.9)),
            ("100", 1, 202512, Some(5.0)),
            ("101", 1, 202601, Some(1.5)),
            ("102A", 1, 202601, None),
            ("103", 2, 202602, Some(-0.25)),
            ("104", 1, 202602, Some(10.125)),
            ("105", 1, 202603, Some(3.0)),
        ]
    }

    fn parse_all(table: &mut DbfTable) -> Vec<(i64, i32, i64, Option<f64>)> {
        let well = table.field("WELL").unwrap();
        let code = table.field("CODE").unwrap();
        let date = table.field("DT").unwrap();
        let value = table.field("VAL").unwrap();
        table
            .par_filter_map(|record| {
                Some((
                    record.well_i64(well)?,
                    record.i32(code)?,
                    record.i64(date)?,
                    record.f64(value),
                ))
            })
            .unwrap()
    }

    #[test]
    fn fast_reader_parses_fields_by_offset() {
        let path = std::env::temp_dir().join(format!("dbf_fast_{}.dbf", std::process::id()));
        write_test_dbf(&path, &test_rows());
        let mut table = DbfTable::open(&path).unwrap();
        let parsed = parse_all(&mut table);
        std::fs::remove_file(&path).ok();
        assert_eq!(
            parsed,
            vec![
                (99, 1, 202511, Some(9.9)),
                (100, 1, 202512, Some(5.0)),
                (101, 1, 202601, Some(1.5)),
                (102, 1, 202601, None),
                (103, 2, 202602, Some(-0.25)),
                (104, 1, 202602, Some(10.125)),
                (105, 1, 202603, Some(3.0)),
            ]
        );
    }

    #[test]
    fn tail_scan_stops_after_grace_chunks_and_keeps_file_order() {
        let path = std::env::temp_dir().join(format!("dbf_tail_{}.dbf", std::process::id()));
        write_test_dbf(&path, &test_rows());
        let mut table = DbfTable::open(&path).unwrap();
        let date = table.field("DT").unwrap();
        let well = table.field("WELL").unwrap();
        // чанк = 1 запись: релевантны даты >= 202602, страховка — 2 чанка
        // плюс чанк, на котором счётчик превысил лимит, поэтому хвост
        // дочитывается до скв 100, а до 099 скан не доходит
        let parsed = table
            .par_filter_map_from_end_chunked(
                1,
                |record| record.i64(date).is_some_and(|value| value >= 202602),
                |record| record.well_i64(well),
            )
            .unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(parsed, vec![100, 101, 102, 103, 104, 105]);
    }
}
