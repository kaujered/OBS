// Parallel packing of the xlsx container.
//
// The stock `ZipWriter` compresses entries one after another in a single
// thread, which on large workbooks costs about half of the save time. Here the
// parts are collected in memory first, then deflated on all available cores,
// and finally handed to the archive as ready-made streams via `raw_copy_file`,
// so no part is compressed twice.
//
// The type mirrors the small slice of the `ZipWriter` API that `Packager`
// uses -- `new`, `start_file`, `Write` and `finish` -- so the packager code
// itself is unchanged apart from the type name.

use std::io::{Cursor, Seek, Write};

use rayon::prelude::*;
use zip::result::{ZipError, ZipResult};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

// One package part, buffered until the parallel compression stage.
struct Part {
    name: String,
    options: SimpleFileOptions,
    data: Vec<u8>,
}

pub(crate) struct ParallelZipWriter<W: Write + Seek> {
    inner: W,
    parts: Vec<Part>,
}

impl<W: Write + Seek> ParallelZipWriter<W> {
    pub(crate) fn new(inner: W) -> ParallelZipWriter<W> {
        ParallelZipWriter {
            inner,
            parts: vec![],
        }
    }

    // Open a new part. Everything written afterwards belongs to it, until the
    // next call.
    pub(crate) fn start_file<S: ToString>(
        &mut self,
        name: S,
        options: SimpleFileOptions,
    ) -> ZipResult<()> {
        self.parts.push(Part {
            name: name.to_string(),
            options,
            data: vec![],
        });

        Ok(())
    }

    // Compress every part in parallel and write the archive.
    pub(crate) fn finish(self) -> ZipResult<W> {
        let ParallelZipWriter { inner, parts } = self;

        // Each part is packed into a single-entry archive of its own. Parts are
        // consumed as they go so that the uncompressed bytes are released
        // during the stage rather than after it.
        let packed: Vec<Vec<u8>> = parts
            .into_par_iter()
            .map(pack_part)
            .collect::<ZipResult<Vec<Vec<u8>>>>()?;

        let mut zip = ZipWriter::new(inner);
        for buffer in packed {
            let mut archive = ZipArchive::new(Cursor::new(buffer))?;
            let entry = archive.by_index_raw(0)?;
            zip.raw_copy_file(entry)?;
        }

        zip.finish()
    }
}

impl<W: Write + Seek> Write for ParallelZipWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.parts.last_mut() {
            Some(part) => {
                part.data.extend_from_slice(buf);
                Ok(buf.len())
            }
            None => Err(std::io::Error::other(
                "xlsx part written before start_file()",
            )),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Pack one part into a single-entry archive, so that its compressed stream can
// be copied into the workbook without being deflated again.
fn pack_part(part: Part) -> ZipResult<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(part.name, part.options)?;
    zip.write_all(&part.data).map_err(ZipError::Io)?;

    Ok(zip.finish()?.into_inner())
}
