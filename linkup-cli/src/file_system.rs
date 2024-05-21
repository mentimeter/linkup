use crate::CliError;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub trait FileLike: Read + Write {}
impl FileLike for std::fs::File {}

#[cfg_attr(test, mockall::automock)]
pub trait FileSystem {
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError>;
    fn write_file(&self, file: &mut Box<dyn FileLike>, content: &str) -> Result<(), CliError>;
    fn file_exists(&self, file_path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<(), CliError>;
}

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError> {
        let file = File::create(path).map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(Box::new(file))
    }

    fn write_file(&self, file: &mut Box<dyn FileLike>, content: &str) -> Result<(), CliError> {
        file.write_all(content.as_bytes())
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(())
    }

    fn file_exists(&self, file_path: &Path) -> bool {
        file_path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), CliError> {
        fs::create_dir_all(path).map_err(|err| CliError::StatusErr(err.to_string()))
    }
}
