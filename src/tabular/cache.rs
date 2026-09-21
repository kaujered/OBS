//! Кэш прочитанных источников в памяти процесса.

/// Кэш «последний результат» в памяти процесса: повторный запрос с тем же
/// ключом (обычно mtime + размер источников плюс параметры) не перечитывает
/// данные. Актуален для GUI, где процесс живёт между выгрузками; изменение
/// любого источника меняет ключ и сбрасывает кэш.
pub(crate) struct SourceCache<K, V> {
    slot: std::sync::Mutex<Option<(K, std::sync::Arc<V>)>>,
}

impl<K: PartialEq, V> SourceCache<K, V> {
    pub(crate) const fn new() -> Self {
        Self {
            slot: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn get_or_load(
        &self,
        key: K,
        load: impl FnOnce() -> anyhow::Result<V>,
    ) -> anyhow::Result<std::sync::Arc<V>> {
        if let Some((cached, value)) = self.slot.lock().expect("source cache lock").as_ref()
            && *cached == key
        {
            return Ok(std::sync::Arc::clone(value));
        }
        let value = std::sync::Arc::new(load()?);
        *self.slot.lock().expect("source cache lock") = Some((key, std::sync::Arc::clone(&value)));
        Ok(value)
    }
}

/// mtime + размер файла для ключей кэша (None — файл недоступен).
pub(crate) fn file_stamp(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}
