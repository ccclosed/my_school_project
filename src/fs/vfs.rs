use alloc::string::String;
use alloc::vec::Vec;

pub trait FileSystem {
    fn read(&self, path: &str) -> Result<Vec<u8>, FsError>;
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError>;
    fn create(&mut self, path: &str) -> Result<(), FsError>;
    fn delete(&mut self, path: &str) -> Result<(), FsError>;
    fn list(&self) -> Vec<(String, usize)>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotFound,
    Exists,
    InvalidName,
    #[allow(dead_code)]
    Io,
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FsError::NotFound => write!(f, "not found"),
            FsError::Exists => write!(f, "already exists"),
            FsError::InvalidName => write!(f, "invalid name"),
            FsError::Io => write!(f, "I/O error"),
        }
    }
}
