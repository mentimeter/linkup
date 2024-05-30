use crate::CliError;
use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub trait FileLike: Read + Write {}
impl FileLike for std::fs::File {}

#[cfg_attr(test, mockall::automock)]
pub trait System {
    fn get_env(&self, key: &str) -> Result<String, CliError>;
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError>;
    fn write_file(&self, file: &mut Box<dyn FileLike>, content: &str) -> Result<(), CliError>;
    fn file_exists(&self, file_path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<(), CliError>;
}

pub struct RealSystem;

impl System for RealSystem {
    fn get_env(&self, key: &str) -> Result<String, CliError> {
        Ok(env::var(key).unwrap_or_else(|_| panic!("{} is not set", key)))
    }

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
