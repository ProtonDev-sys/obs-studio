//! Safe core for `libobs/util/file-serializer.c`.
//!
//! The C ABI shim lives in [`crate::ffi::file_serializer`]. Reads and writes
//! go through [`std::fs::File`], which is `fopen` / `fread` / `fwrite` on
//! Unix and the wide Win32 APIs on Windows, same as `os_fopen`.

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

/// `enum serialize_seek_type` from `serializer.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekType {
    Start,
    Current,
    End,
}

/// C initializes the origin to `SEEK_SET` and only overwrites it for the
/// three named enum values, so any other code seeks from the start.
pub fn seek_type_from_c(v: i32) -> SeekType {
    match v {
        1 => SeekType::Current,
        2 => SeekType::End,
        _ => SeekType::Start,
    }
}

/// Temp path built the way `dstr_copy` + `dstr_cat_ch` + `dstr_cat` do.
/// An empty `path` still gets the dot and the extension. A leading dot on
/// `ext` suppresses the extra dot.
pub fn temp_file_name(path: &[u8], ext: &[u8]) -> Vec<u8> {
    let mut name = path.to_vec();
    if ext.first().copied() != Some(b'.') {
        name.push(b'.');
    }
    name.extend_from_slice(ext);
    name
}

fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
    Some(crate::platform::path_from_bytes(bytes))
}

fn read_file(file: &mut File, buf: &mut [u8]) -> usize {
    let mut got = 0;
    while got < buf.len() {
        match file.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    got
}

fn write_file<W: Write>(file: &mut W, mut buf: &[u8]) -> usize {
    let mut put = 0;
    while !buf.is_empty() {
        match file.write(buf) {
            Ok(0) => break,
            Ok(n) => {
                put += n;
                buf = &buf[n..];
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    put
}

pub fn seek_file<F: Seek>(file: &mut F, offset: i64, kind: SeekType) -> i64 {
    let from = match kind {
        SeekType::Start => {
            if offset < 0 {
                return -1;
            }
            SeekFrom::Start(offset as u64)
        }
        SeekType::Current => SeekFrom::Current(offset),
        SeekType::End => SeekFrom::End(offset),
    };
    match file.seek(from) {
        Ok(pos) => pos as i64,
        Err(_) => -1,
    }
}

pub fn file_pos<F: Seek>(file: &mut F) -> i64 {
    match file.stream_position() {
        Ok(pos) => pos as i64,
        Err(_) => -1,
    }
}

pub struct Input {
    file: File,
}

impl Input {
    pub fn open(path: &[u8]) -> Option<Self> {
        let path = path_from_bytes(path)?;
        Some(Self {
            file: File::open(path).ok()?,
        })
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        read_file(&mut self.file, buf)
    }

    pub fn seek(&mut self, offset: i64, kind: SeekType) -> i64 {
        seek_file(&mut self.file, offset, kind)
    }

    pub fn pos(&mut self) -> i64 {
        file_pos(&mut self.file)
    }
}

pub struct Output {
    /// Buffered like `fwrite`: small writes succeed and a full disk is only
    /// reported when the buffer is flushed.
    file: Option<BufWriter<File>>,
    /// `(final path, temp path)`. `None` for a plain `init`.
    commit: Option<(PathBuf, PathBuf)>,
    /// A write came up short; a safe save then keeps the original file.
    failed: bool,
}

impl Output {
    pub fn create(path: &[u8]) -> Option<Self> {
        let path = path_from_bytes(path)?;
        Some(Self {
            file: Some(BufWriter::new(File::create(path).ok()?)),
            commit: None,
            failed: false,
        })
    }

    /// `None` when `ext` is empty, or when either path cannot be opened.
    /// The destination file is not created until this value is dropped.
    pub fn create_safe(path: &[u8], ext: &[u8]) -> Option<Self> {
        if ext.is_empty() {
            return None;
        }
        let final_path = path_from_bytes(path)?;
        let temp_path = path_from_bytes(&temp_file_name(path, ext))?;
        let file = File::create(&temp_path).ok()?;
        Some(Self {
            file: Some(BufWriter::new(file)),
            commit: Some((final_path, temp_path)),
            failed: false,
        })
    }

    pub fn write(&mut self, buf: &[u8]) -> usize {
        match self.file.as_mut() {
            Some(file) => {
                let put = write_file(file, buf);
                if put < buf.len() {
                    self.failed = true;
                }
                put
            }
            None => 0,
        }
    }

    pub fn seek(&mut self, offset: i64, kind: SeekType) -> i64 {
        match self.file.as_mut() {
            Some(file) => seek_file(file, offset, kind),
            None => -1,
        }
    }

    pub fn pos(&mut self) -> i64 {
        match self.file.as_mut() {
            Some(file) => file_pos(file),
            None => -1,
        }
    }

    fn commit_now(&mut self) {
        if let Some(mut file) = self.file.take() {
            if file.flush().is_err() {
                self.failed = true;
            }
            // Keep the final close result, as C does with fclose. Taking the
            // parts also avoids retrying a failed flush in BufWriter's drop.
            let (file, _) = file.into_parts();
            if crate::platform::close_file(file).is_err() {
                self.failed = true;
            }
        }
        if let Some((final_path, temp_path)) = self.commit.take() {
            // As C (#71): replace only after a clean write, and never unlink
            // the destination first. rename replaces atomically on Unix and
            // uses MoveFileExW(MOVEFILE_REPLACE_EXISTING) on Windows. C also
            // logs a warning on failure; the safe core has no logger.
            if !self.failed {
                let _ = std::fs::rename(&temp_path, &final_path);
            }
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.commit_now();
    }
}
