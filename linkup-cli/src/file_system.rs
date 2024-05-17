use crate::CliError;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub trait FileLike: Read + Write {}
impl FileLike for std::fs::File {}

#[cfg_attr(test, mockall::automock)]
pub trait FileSystem {
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError>;
    fn write_file(&self, file: &mut Box<dyn FileLike>, content: &str) -> Result<(), CliError>;
    fn file_exists(&self, file_path: &str) -> bool {
        Path::new(file_path).exists()
    }
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
}
