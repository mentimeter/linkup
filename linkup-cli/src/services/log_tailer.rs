use std::{io::SeekFrom, path::PathBuf, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncSeekExt, BufReader},
    task::JoinHandle,
    time::sleep,
};

pub struct LogTailer {
    handle: JoinHandle<()>,
}

impl Drop for LogTailer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub fn tail_logs(file_path: PathBuf, from_start: bool) -> LogTailer {
    let handle = tokio::spawn(async move {
        for _ in 0..200 {
            if file_path.exists() {
                break;
            }

            sleep(Duration::from_millis(10)).await;
        }

        let Ok(mut file) = tokio::fs::File::open(&file_path).await else {
            return;
        };

        if !from_start {
            let _ = file.seek(SeekFrom::End(0)).await;
        }

        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line).await {
                Ok(0) => sleep(Duration::from_millis(10)).await,
                Ok(_) => {
                    eprint!("{line}");
                    line.clear();
                }
                Err(_) => return,
            }
        }
    });

    LogTailer { handle }
}
