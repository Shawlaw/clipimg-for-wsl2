/// 简单的双写日志：同时写入文件和控制台
/// 替代 env_logger（需要 RUST_LOG 环境变量才能工作）

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// 从 Unix epoch 天数计算年月日
pub fn days_to_ymd(mut days: i64) -> (u32, u32, u32) {
    let mut y = 1970i64;
    loop {
        let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if days < dy { break; }
        days -= dy;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md: &[u32] = if leap { &[31,29,31,30,31,30,31,31,30,31,30,31] } else { &[31,28,31,30,31,30,31,31,30,31,30,31] };
    let mut m = 0u32;
    for (i, &d) in md.iter().enumerate() {
        if days < d as i64 { m = i as u32 + 1; break; }
        days -= d as i64;
    }
    if m == 0 { m = 12; }
    (y as u32, m, days as u32 + 1)
}

/// 当前本地时间格式化为 YYYY-MM-DD HH:MM:SS（UTC+8）
fn now_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let local_secs = secs + 8 * 3600;
    let days = local_secs / 86400;
    let tod = local_secs % 86400;
    let (y, mo, d) = days_to_ymd(days as i64);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, tod / 3600, (tod % 3600) / 60, tod % 60)
}

/// 当前本地时间格式化为 YYYYMMDD_HHmmSS（用于文件名）
pub fn filename_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let local_secs = secs + 8 * 3600;
    let days = local_secs / 86400;
    let tod = local_secs % 86400;
    let (y, mo, d) = days_to_ymd(days as i64);
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", y, mo, d, tod / 3600, (tod % 3600) / 60, tod % 60)
}

struct FileAndConsoleLogger {
    file: Mutex<File>,
    console_mode: bool,
}

impl log::Log for FileAndConsoleLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = now_timestamp();
        let msg = format!("[{} {}] {}", timestamp, record.level(), record.args());

        // 写文件
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{}", msg);
            let _ = file.flush();
        }

        // 写控制台（Error/Warn → stderr，其余 → stdout）
        if self.console_mode {
            if record.level() <= log::Level::Warn {
                eprintln!("{}", msg);
            } else {
                println!("{}", msg);
            }
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
/// console_mode: 是否同时输出到控制台
pub fn init(log_path: &Path, console_mode: bool) {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .expect("无法创建日志文件");

    let logger = Box::new(FileAndConsoleLogger {
        file: Mutex::new(file),
        console_mode,
    });

    log::set_boxed_logger(logger).expect("无法设置日志");
    log::set_max_level(log::LevelFilter::Debug);
}

/// 注册 panic handler，将崩溃信息写入日志文件
pub fn set_panic_hook(log_path: &Path) {
    let log_path = log_path.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        let timestamp = now_timestamp();
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
