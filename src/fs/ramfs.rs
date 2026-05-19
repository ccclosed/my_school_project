use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

use super::vfs::{FileSystem, FsError};

pub(crate) struct RamFs {
    files: BTreeMap<String, Vec<u8>>,
}

impl RamFs {
    fn normalize(path: &str) -> Result<String, FsError> {
        let path = path.trim();
        if path.is_empty() || path.contains('/') || path.contains('\\') {
            return Err(FsError::InvalidName);
        }
        Ok(path.to_string())
    }
}

impl FileSystem for RamFs {
    fn read(&self, path: &str) -> Result<Vec<u8>, FsError> {
        let key = Self::normalize(path)?;
        self.files.get(&key).cloned().ok_or(FsError::NotFound)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        let key = Self::normalize(path)?;
        self.files.insert(key, data.to_vec());
        Ok(())
    }

    fn create(&mut self, path: &str) -> Result<(), FsError> {
        let key = Self::normalize(path)?;
        if self.files.contains_key(&key) {
            return Err(FsError::Exists);
        }
        self.files.insert(key, Vec::new());
        Ok(())
    }

    fn delete(&mut self, path: &str) -> Result<(), FsError> {
        let key = Self::normalize(path)?;
        self.files.remove(&key).ok_or(FsError::NotFound)?;
        Ok(())
    }

    fn list(&self) -> Vec<(String, usize)> {
        self.files
            .iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect()
    }
}

lazy_static! {
    static ref RAMFS: Mutex<RamFs> = Mutex::new(RamFs {
        files: BTreeMap::new(),
    });
}

pub fn read(path: &str) -> Result<Vec<u8>, FsError> {
    RAMFS.lock().read(path)
}

pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    RAMFS.lock().write(path, data)
}

pub fn create(path: &str) -> Result<(), FsError> {
    RAMFS.lock().create(path)
}

pub fn delete(path: &str) -> Result<(), FsError> {
    RAMFS.lock().delete(path)
}

pub fn list() -> Vec<(String, usize)> {
    RAMFS.lock().list()
}
