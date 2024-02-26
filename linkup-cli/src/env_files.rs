use std::io::Write;
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
};

use crate::{CliError, Result};

const LINKUP_ENV_SEPARATOR: &str = "##### Linkup environment - DO NOT EDIT #####";

pub fn write_to_env_file(service: &str, dev_env_path: &PathBuf, env_path: &PathBuf) -> Result<()> {
    if let Ok(env_content) = fs::read_to_string(env_path) {
        if env_content.contains(LINKUP_ENV_SEPARATOR) {
            return Ok(());
        }
    }

    let mut dev_env_content = fs::read_to_string(dev_env_path).map_err(|e| {
        CliError::SetServiceEnv(
            service.to_string(),
            format!("could not read dev env file: {}", e),
        )
    })?;

    if dev_env_content.ends_with('\n') {
        dev_env_content.pop();
    }

    let mut env_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(env_path)
        .map_err(|e| {
            CliError::SetServiceEnv(
                service.to_string(),
                format!("Failed to open .env file: {}", e),
            )
        })?;

    let content = vec![
        format!("\n{}", LINKUP_ENV_SEPARATOR),
        format!("\n{}", dev_env_content),
        format!("\n{}", LINKUP_ENV_SEPARATOR),
    ];

    writeln!(env_file, "{}", content.concat()).map_err(|e| {
        CliError::SetServiceEnv(
            service.to_string(),
            format!("could not write to env file: {}", e),
        )
    })?;

    Ok(())
}

pub fn clear_env_file(service: &str, env_path: &PathBuf) -> Result<()> {
    let mut file_content = fs::read_to_string(env_path).map_err(|e| {
        CliError::RemoveServiceEnv(
            service.to_string(),
            format!("could not read dev env file: {}", e),
        )
    })?;

    if let (Some(mut linkup_block_start), Some(mut linkup_block_end)) = (
        file_content.find(LINKUP_ENV_SEPARATOR),
        file_content.rfind(LINKUP_ENV_SEPARATOR),
    ) {
        if linkup_block_start > 0 && file_content.chars().nth(linkup_block_start - 1) == Some('\n')
        {
            linkup_block_start -= 1;
        }

        linkup_block_end += LINKUP_ENV_SEPARATOR.len() - 1;
        if file_content.chars().nth(linkup_block_end + 1) == Some('\n') {
            linkup_block_end += 1;
        }

        if linkup_block_start < linkup_block_end {
            file_content.drain(linkup_block_start..=linkup_block_end);
        }

        file_content = file_content.trim_end_matches('\n').to_string();
        file_content.push('\n');

        // Write the updated content back to the file
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(env_path)
            .map_err(|e| {
                CliError::RemoveServiceEnv(
                    service.to_string(),
                    format!("Failed to open .env file for writing: {}", e),
                )
            })?;
        file.write_all(file_content.as_bytes()).map_err(|e| {
            CliError::RemoveServiceEnv(
                service.to_string(),
                format!("Failed to write .env file: {}", e),
            )
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;

    use crate::env_files::write_to_env_file;

    use super::clear_env_file;

    #[test]
    fn write_to_env_file_empty_target() {
        let source = TestFile::create("SOURCE_1=VALUE_1");
        let target = TestFile::create("");

        write_to_env_file("service_1", &source.path, &target.path).unwrap();

        let file_content = fs::read_to_string(&target.path).unwrap();
        let expected_content = format!(
            "\n{}\n{}\n{}\n",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );

        assert_eq!(file_content, expected_content);
    }

    #[test]
    fn write_to_env_file_filled_target() {
        let source = TestFile::create("SOURCE_1=VALUE_1");
        let target = TestFile::create("EXISTING_1=VALUE_1\nEXISTING_2=VALUE_2");

        write_to_env_file("service_1", &source.path, &target.path).unwrap();

        let file_content = fs::read_to_string(&target.path).unwrap();
        let expected_content = format!(
            "{}\n{}\n{}\n{}\n{}\n",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );

        assert_eq!(file_content, expected_content);
    }

    #[test]
    fn clear_env_file_only_linkup_env() {
        let content = format!(
            "\n{}\n{}\n{}\n",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );
        let env_file = TestFile::create(&content);

        clear_env_file("service_1", &env_file.path).unwrap();

        let file_content = fs::read_to_string(&env_file.path).unwrap();
        assert_eq!("\n", file_content);
    }

    #[test]
    fn clear_env_file_existing_env_before_linkup() {
        let content = format!(
            "{}\n{}\n{}\n\n{}\n{}\n",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );
        let env_file = TestFile::create(&content);

        clear_env_file("service_1", &env_file.path).unwrap();

        let file_content = fs::read_to_string(&env_file.path).unwrap();
        let expected_content = format!("{}\n{}\n", "EXISTING_1=VALUE_1", "EXISTING_2=VALUE_2",);
        assert_eq!(expected_content, file_content);
    }

    #[test]
    fn clear_env_file_existing_env_before_and_after_linkup() {
        let content = format!(
            "{}\n{}\n{}\n\n{}\n{}\n\n{}\n{}",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
            "EXISTING_3=VALUE_3",
            "EXISTING_4=VALUE_4",
        );
        let env_file = TestFile::create(&content);

        clear_env_file("service_1", &env_file.path).unwrap();

        let file_content = fs::read_to_string(&env_file.path).unwrap();
        let expected_content = format!(
            "{}\n{}\n{}\n{}\n",
            "EXISTING_1=VALUE_1", "EXISTING_2=VALUE_2", "EXISTING_3=VALUE_3", "EXISTING_4=VALUE_4",
        );
        assert_eq!(expected_content, file_content);
    }

    #[test]
    fn clear_env_file_no_new_line_after_linkup() {
        let content = format!(
            "{}\n{}\n{}\n\n{}\n{}",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );
        let env_file = TestFile::create(&content);

        clear_env_file("service_1", &env_file.path).unwrap();

        let file_content = fs::read_to_string(&env_file.path).unwrap();
        let expected_content = format!("{}\n{}\n", "EXISTING_1=VALUE_1", "EXISTING_2=VALUE_2",);
        assert_eq!(expected_content, file_content);
    }

    #[test]
    fn clear_env_file_multiple_new_lines_after_linkup() {
        let content = format!(
            "{}\n{}\n{}\n\n{}\n{}\n\n\n\n",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "##### Linkup environment - DO NOT EDIT #####",
        );
        let env_file = TestFile::create(&content);

        clear_env_file("service_1", &env_file.path).unwrap();

        let file_content = fs::read_to_string(&env_file.path).unwrap();
        let expected_content = format!("{}\n{}\n", "EXISTING_1=VALUE_1", "EXISTING_2=VALUE_2",);
        assert_eq!(expected_content, file_content);
    }

    #[test]
    fn write_and_clear() {
        let source = TestFile::create("SOURCE_1=VALUE_1\nSOURCE_2=VALUE_2");
        let target = TestFile::create("EXISTING_1=VALUE_1\nEXISTING_2=VALUE_2");

        write_to_env_file("service_1", &source.path, &target.path).unwrap();

        // Check post write content
        let file_content = fs::read_to_string(&target.path).unwrap();
        let expected_content = format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            "EXISTING_1=VALUE_1",
            "EXISTING_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
            "SOURCE_1=VALUE_1",
            "SOURCE_2=VALUE_2",
            "##### Linkup environment - DO NOT EDIT #####",
        );
        assert_eq!(file_content, expected_content);

        clear_env_file("service_1", &target.path).unwrap();

        // Check post clear content
        let file_content = fs::read_to_string(&target.path).unwrap();
        let expected_content = format!("{}\n{}\n", "EXISTING_1=VALUE_1", "EXISTING_2=VALUE_2",);
        assert_eq!(file_content, expected_content);
    }

    struct TestFile {
        pub path: PathBuf,
    }

    impl TestFile {
        fn create(content: &str) -> Self {
            let file_name: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(12)
                .map(char::from)
                .collect();

            let mut test_file = std::env::temp_dir();
            test_file.push(file_name);

            let mut env_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&test_file)
                .unwrap();

            write!(&mut env_file, "{}", content).unwrap();

            Self { path: test_file }
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_file(&self.path) {
                println!("failed to remove file {}: {}", &self.path.display(), err);
            }
        }
    }
}
