/// 简单的双写日志：同时写入文件和控制台
/// 替代 env_logger（需要 RUST_LOG 环境变量才能工作）

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

struct FileAndConsoleLogger {
    file: Mutex<File>,
}

impl log::Log for FileAndConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let msg = format!("[{} {}] {}", timestamp, record.level(), record.args());

        // 写文件
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{}", msg);
            let _ = file.flush();
        }

        // 写控制台（Error/Warn → stderr，其余 → stdout）
        if record.level() <= log::Level::Warn {
            eprintln!("{}", msg);
        } else {
            println!("{}", msg);
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// 初始化日志系统
/// log_path: 日志文件路径（如 save_dir/.clipimg.log）
pub fn init(log_path: &Path) {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("无法创建日志文件");

    let logger = Box::new(FileAndConsoleLogger {
        file: Mutex::new(file),
    });

    log::set_boxed_logger(logger).expect("无法设置日志");
    log::set_max_level(log::LevelFilter::Debug);
}

/// 注册 panic handler，将崩溃信息写入日志文件
pub fn set_panic_hook(log_path: &Path) {
    let log_path = log_path.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let msg = format!("[{} ERROR] PANIC: {}", timestamp, info);

        // 尝试写到日志文件
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(file, "{}", msg);
            // 也写 backtrace
            let _ = writeln!(file, "[{} ERROR] Backtrace:\n{:?}", timestamp, std::backtrace::Backtrace::capture());
        }

        eprintln!("{}", msg);
        eprintln!("日志文件: {}", log_path.display());
    }));
}
