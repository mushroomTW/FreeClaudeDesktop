use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct PendingWrite {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}

impl PendingWrite {
    /// 建立新的實例。
    pub fn new(path: PathBuf, contents: Vec<u8>) -> Self {
        Self { path, contents }
    }
}

struct StagedWrite {
    target: PathBuf,
    temp: PathBuf,
    original: Option<Vec<u8>>,
}

impl StagedWrite {
    /// 執行 `create` 對應的處理流程。
    fn create(write: PendingWrite) -> io::Result<Self> {
        let original = if write.path.exists() {
            Some(fs::read(&write.path)?)
        } else {
            None
        };
        let temp = stage_temp(&write.path, &write.contents)?;
        Ok(Self {
            target: write.path,
            temp,
            original,
        })
    }

    /// 執行 `discard` 對應的處理流程。
    fn discard(self) {
        let _ = fs::remove_file(self.temp);
    }

    /// 執行 `restore` 對應的處理流程。
    fn restore(&self) -> io::Result<()> {
        match &self.original {
            Some(contents) => {
                let temp = stage_temp(&self.target, contents)?;
                match replace_file(&temp, &self.target) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        let _ = fs::remove_file(temp);
                        Err(error)
                    }
                }
            }
            None => match fs::remove_file(&self.target) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        }
    }
}

/// 儲存 `write_transaction` 所處理的資料。
pub fn write_transaction(writes: Vec<PendingWrite>) -> io::Result<()> {
    let mut staged = Vec::with_capacity(writes.len());
    for write in writes {
        match StagedWrite::create(write) {
            Ok(write) => staged.push(write),
            Err(error) => {
                for write in staged {
                    write.discard();
                }
                return Err(error);
            }
        }
    }
    commit_staged(staged)
}

/// 執行 `stage_temp` 對應的處理流程。
fn stage_temp(target: &Path, contents: &[u8]) -> io::Result<PathBuf> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let filename = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no filename"))?
        .to_string_lossy();

    loop {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(".{filename}.{}.{counter}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(contents).and_then(|_| file.sync_all()) {
                    drop(file);
                    let _ = fs::remove_file(&temp);
                    return Err(error);
                }
                return Ok(temp);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// 執行 `commit_staged` 對應的處理流程。
fn commit_staged(staged: Vec<StagedWrite>) -> io::Result<()> {
    let mut committed = Vec::with_capacity(staged.len());
    let mut remaining = staged.into_iter();
    while let Some(write) = remaining.next() {
        if let Err(error) = replace_file(&write.temp, &write.target) {
            write.discard();
            for write in remaining {
                write.discard();
            }

            let rollback_errors: Vec<String> = committed
                .iter()
                .rev()
                .filter_map(|write: &StagedWrite| write.restore().err())
                .map(|error| error.to_string())
                .collect();
            if rollback_errors.is_empty() {
                return Err(error);
            }
            return Err(io::Error::new(
                error.kind(),
                format!("{error}; rollback failed: {}", rollback_errors.join("; ")),
            ));
        }
        committed.push(write);
    }
    Ok(())
}

#[cfg(not(windows))]
/// 執行 `replace_file` 對應的處理流程。
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
/// 執行 `replace_file` 對應的處理流程。
fn replace_file(source: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::winbase::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 驗證 `staging_failure_keeps_every_original_file` 的行為符合預期。
    fn staging_failure_keeps_every_original_file() {
        let root = std::env::temp_dir().join(format!("fcd-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.json");
        let blocker = root.join("blocker");
        std::fs::write(&first, b"old").unwrap();
        std::fs::write(&blocker, b"not-a-directory").unwrap();

        let result = write_transaction(vec![
            PendingWrite::new(first.clone(), b"new".to_vec()),
            PendingWrite::new(blocker.join("second.json"), b"new".to_vec()),
        ]);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&first).unwrap(), b"old");
        let _ = std::fs::remove_dir_all(&root);
    }
}
