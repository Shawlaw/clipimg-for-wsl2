use crate::config::AppConfig;
use crate::logger::filename_timestamp;
use image::ImageFormat;
use md5::{Digest, Md5};
use std::fs;
use std::path::{Path, PathBuf};

/// 剪贴板图片监控器
///
/// 核心文件操作逻辑（去重、保存、清理）是平台无关的。
/// 实际的剪贴板轮询通过 `poll_with_data` 方法接收 RGBA 数据。
pub struct ClipboardWatcher {
    pub config: AppConfig,
    pub save_dir: PathBuf,
    /// 上一次保存图片的 MD5，用于在内存中去重，避免盲写磁盘
    last_md5: std::cell::RefCell<Option<String>>,
    /// 存储目录是否可用（UNC 路径下 WSL 可能未启动）
    dir_available: std::cell::RefCell<bool>,
    /// 用户是否选择了"不再提醒"存储目录不可用
    suppress_unavailable_notify: std::cell::RefCell<bool>,
    /// 是否已通知过本次不可用（用于本次不可用期间的首次通知）
    dir_unavailable_notified: std::cell::RefCell<bool>,
}

impl ClipboardWatcher {
    pub fn new(config: AppConfig, exe_dir: &Path) -> Self {
        let save_dir = config.resolved_save_dir(exe_dir);
        Self {
            config,
            save_dir,
            last_md5: std::cell::RefCell::new(None),
            dir_available: std::cell::RefCell::new(true),
            suppress_unavailable_notify: std::cell::RefCell::new(false),
            dir_unavailable_notified: std::cell::RefCell::new(false),
        }
    }

    /// 确保保存目录存在
    pub fn ensure_dir(&self) -> Result<(), std::io::Error> {
        if let Err(e) = fs::create_dir_all(&self.save_dir) {
            log::warn!("创建保存目录失败（可能是 UNC 路径且 WSL 未启动）: {}", e);
            *self.dir_available.borrow_mut() = false;
        }
        Ok(())
    }

    /// 检查存储目录是否可访问，返回 true 表示可用
    /// 自动检测恢复并通知用户
    pub fn check_dir_available(&self) -> bool {
        let available = fs::metadata(&self.save_dir)
            .map(|m| m.is_dir())
            .unwrap_or(false);

        let was_available = *self.dir_available.borrow();
        if available && !was_available {
            // 恢复可用
            *self.dir_available.borrow_mut() = true;
            *self.suppress_unavailable_notify.borrow_mut() = false;
            *self.dir_unavailable_notified.borrow_mut() = false;
            log::info!("存储目录已恢复可用");
            self.notify_dir_recovered();
        } else if !available && was_available {
            *self.dir_available.borrow_mut() = false;
        }
        available
    }

    /// 弹出恢复可用的标准 MessageBox
    #[cfg(target_os = "windows")]
    fn notify_dir_recovered(&self) {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        let msg: Vec<u16> = OsStr::new("存储目录已恢复")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let title: Vec<u16> = OsStr::new("clipImg")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        std::thread::spawn(move || unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                msg.as_ptr(),
                title.as_ptr(),
                0x40, // MB_ICONINFORMATION
            );
        });
    }

    #[cfg(not(target_os = "windows"))]
    fn notify_dir_recovered(&self) {}

    /// 弹出不可用对话框（双按钮：确定 + 不再提醒）
    /// operation_type: "截图" 或 "复制文件"
    #[cfg(target_os = "windows")]
    pub fn notify_dir_unavailable(&self, operation_type: &str) {
        if *self.suppress_unavailable_notify.borrow() {
            return;
        }
        if *self.dir_unavailable_notified.borrow() {
            return;
        }
        *self.dir_unavailable_notified.borrow_mut() = true;

        let msg = format!(
            "存储目录暂不可用，请在WSL2启动后再尝试重新{}",
            operation_type
        );

        let result = show_unavailable_dialog(&msg);
        if result == 2 {
            // 用户点击了"不再提醒"
            *self.suppress_unavailable_notify.borrow_mut() = true;
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn notify_dir_unavailable(&self, _operation_type: &str) {}

    /// 迁移旧版 latest_file.* / latest.png 为 clip_<timestamp>.<ext> 格式
    pub fn migrate_legacy_files(&self) {
        let entries = match fs::read_dir(&self.save_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let should_migrate = name == "latest.png"
                || name == "latest_file"
                || name.starts_with("latest_file.");

            if !should_migrate {
                continue;
            }

            let path = entry.path();
            let extension = if name == "latest.png" || name.contains(".png") {
                "png"
            } else if let Some(dot_pos) = name.rfind('.') {
                &name[dot_pos + 1..]
            } else {
                ""
            };

            // 用 mtime 生成文件名
            let timestamp = match fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
            {
                Some(mtime) => {
                    let dur = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    format_timestamp_from_secs_millis(
                        dur.as_secs(),
                        dur.subsec_millis(),
                    )
                }
                None => filename_timestamp(),
            };

            let new_path = self.unique_history_path(&timestamp, extension);
            match fs::rename(&path, &new_path) {
                Ok(()) => log::info!(
                    "迁移旧版文件: {} → {}",
                    name,
                    new_path.file_name().unwrap_or_default().to_string_lossy()
                ),
                Err(e) => log::warn!("迁移旧版文件 {} 失败: {}", name, e),
            }
        }
    }

    /// 用 RGBA 像素数据轮询并保存（平台无关的核心逻辑）
    /// 返回 Some(saved_path) 表示有新图片保存，返回 None 表示无新内容
    pub fn poll_with_data(&self, width: usize, height: usize, rgba: &[u8]) -> Option<String> {
        if !self.check_dir_available() {
            log::warn!("存储目录不可用，跳过截图保存");
            return None;
        }

        // 先在内存中计算 MD5，与上次保存的比对
        let current_md5 = {
            let mut hasher = Md5::new();
            hasher.update(rgba);
            format!("{:x}", hasher.finalize())
        };

        {
            let last = self.last_md5.borrow();
            if last.as_ref() == Some(&current_md5) {
                return None; // 内容没变，不写磁盘
            }
        }

        // 内容有变化，执行保存
        let tmp_path = self.save_dir.join("_tmp_clip.png");

        if let Err(e) = self.save_rgba_to_png(width, height, rgba, &tmp_path) {
            log::warn!("保存临时文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return None;
        }

        let timestamp = filename_timestamp();
        let history_path = self.unique_history_path(&timestamp, "png");

        if let Err(e) = fs::rename(&tmp_path, &history_path) {
            log::warn!("重命名历史文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return None;
        }

        // 更新缓存的 MD5
        *self.last_md5.borrow_mut() = Some(current_md5);

        let saved_name = history_path.file_name()?.to_str()?.to_string();
        log::info!("新图片已保存: {}", saved_name);
        self.clean_old_files();
        Some(saved_name)
    }

    /// 从 CF_HDROP 文件复制保存（单个文件）
    /// 返回 Some(saved_filename) 表示成功保存
    /// 返回 None 表示跳过
    pub fn copy_file(&self, src_path: &Path) -> Option<String> {
        if !self.check_dir_available() {
            log::warn!("存储目录不可用，跳过文件复制");
            return None;
        }

        let file_name = src_path.file_name()?.to_str()?;
        let extension = src_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        // 跳过目录
        if fs::metadata(src_path).map(|m| m.is_dir()).unwrap_or(false) {
            log::debug!("跳过目录: {}", file_name);
            return None;
        }

        // 检查文件大小
        let metadata = fs::metadata(src_path).ok()?;
        let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
        if size_mb > self.config.max_copy_size_mb as f64 {
            log::warn!(
                "文件过大，跳过: {} ({:.1}MB > {}MB)",
                file_name, size_mb, self.config.max_copy_size_mb
            );
            return None;
        }

        // 生成历史文件名（保留原始后缀）
        let timestamp = filename_timestamp();
        let history_path = self.unique_history_path(&timestamp, &extension);

        if let Err(e) = fs::copy(src_path, &history_path) {
            log::warn!("复制文件失败: {}", e);
            return None;
        }

        // 将 mtime 设为当前时间，防止源文件的旧 mtime 被保留导致被清理
        if let Ok(f) = fs::File::open(&history_path) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }

        let saved_name = history_path.file_name()?.to_str()?.to_string();
        log::info!("新文件已保存: {}", saved_name);
        self.clean_old_files();
        Some(saved_name)
    }

    /// 从 CF_HDROP 批量复制多个文件
    /// 返回成功保存的文件名列表（save_dir 下的相对文件名）
    pub fn copy_files(&self, src_paths: &[PathBuf]) -> Vec<String> {
        if !self.check_dir_available() {
            log::warn!("存储目录不可用，跳过批量文件复制");
            return Vec::new();
        }

        let max = self.config.max_copy_files as usize;
        let mut results = Vec::new();

        for (i, src_path) in src_paths.iter().enumerate() {
            if i >= max {
                let skipped = src_paths.len() - max;
                log::warn!(
                    "文件数超出上限 ({}/{})，已跳过 {} 个文件",
                    max, src_paths.len(), skipped
                );
                break;
            }
            if let Some(name) = self.copy_file(src_path) {
                results.push(name);
            }
        }

        results
    }

    /// 查找 save_dir 中最新的 clip_* 文件（按 mtime 排序）
    /// 返回 (磁盘完整路径, 文件名)
    pub fn find_latest_clip(&self) -> Option<(PathBuf, String)> {
        let entries = fs::read_dir(&self.save_dir).ok()?;
        let mut latest: Option<(PathBuf, String, std::time::SystemTime)> = None;

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("clip_") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if latest.as_ref().map_or(true, |(_, _, t)| mtime > *t) {
                        latest = Some((entry.path(), name, mtime));
                    }
                }
            }
        }

        latest.map(|(path, name, _)| (path, name))
    }

    /// 生成唯一的历史文件路径，避免同一时间戳内的文件名冲突
    fn unique_history_path(&self, timestamp: &str, extension: &str) -> PathBuf {
        let base_name = if extension.is_empty() {
            format!("clip_{}", timestamp)
        } else {
            format!("clip_{}.{}", timestamp, extension)
        };
        let base = self.save_dir.join(&base_name);
        if !base.exists() {
            return base;
        }
        // 同一时间戳内追加序号
        for i in 1..100 {
            let name = if extension.is_empty() {
                format!("clip_{}_{}", timestamp, i)
            } else {
                format!("clip_{}_{}.{}", timestamp, i, extension)
            };
            let path = self.save_dir.join(name);
            if !path.exists() {
                return path;
            }
        }
        base // 极端情况直接覆盖
    }

    /// 从系统剪贴板轮询（仅 Windows）
    #[cfg(target_os = "windows")]
    pub fn poll(&self, clipboard: &mut arboard::Clipboard) -> Option<String> {
        let image_data = match clipboard.get_image() {
            Ok(img) => img,
            Err(_) => return None,
        };
        self.poll_with_data(image_data.width, image_data.height, &image_data.bytes)
    }

    /// 将 RGBA 数据保存为 PNG
    fn save_rgba_to_png(
        &self,
        width: usize,
        height: usize,
        rgba: &[u8],
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let img = image::RgbaImage::from_raw(
            width as u32,
            height as u32,
            rgba.to_vec(),
        )
        .ok_or("无法创建图片缓冲区")?;

        img.save_with_format(path, ImageFormat::Png)?;
        Ok(())
    }

    /// 清理超过 max_history_hours 的历史图片
    pub fn clean_old_files(&self) -> usize {
        let max_hours = self.config.max_history_hours;
        if max_hours == 0 {
            return 0; // 0 表示不清理
        }
        let mut deleted = 0;

        let entries = match fs::read_dir(&self.save_dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };

        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(max_hours as u64 * 3600);

        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            // 匹配所有 clip_* 文件（不限定后缀）
            if !name.starts_with("clip_") {
                continue;
            }

            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        if fs::remove_file(&path).is_ok() {
                            log::info!("已删除过期文件: {}", name);
                            deleted += 1;
                        }
                    }
                }
            }
        }

        deleted
    }

    /// 判断文件是否为 PNG（读文件头 magic bytes）
    pub fn is_png_file(path: &Path) -> bool {
        use std::io::Read;
        let mut file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let mut buf = [0u8; 8];
        file.read_exact(&mut buf).is_ok() && buf.starts_with(b"\x89PNG")
    }
}

/// 计算文件 MD5（独立函数，方便测试）
pub fn file_md5(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    let mut hasher = Md5::new();
    hasher.update(&data);
    Some(format!("{:x}", hasher.finalize()))
}

/// 将 Unix 时间戳（秒 + 毫秒）格式化为 YYYYMMDD_HHmmSSmmm
fn format_timestamp_from_secs_millis(secs: u64, millis: u32) -> String {
    let local_secs = secs + 8 * 3600; // UTC+8
    let days = local_secs / 86400;
    let tod = local_secs % 86400;
    let (y, mo, d) = days_to_ymd(days as i64);
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}{:03}",
        y, mo, d,
        (tod / 3600) as u32,
        ((tod % 3600) / 60) as u32,
        (tod % 60) as u32,
        millis
    )
}

/// 从 Unix epoch 天数计算年月日
fn days_to_ymd(mut days: i64) -> (u32, u32, u32) {
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

/// 从剪贴板读取 CF_HDROP 文件路径列表（仅 Windows）
#[cfg(target_os = "windows")]
pub fn read_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_HDROP: u32 = 15;

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }

        let handle = GetClipboardData(CF_HDROP);
        if handle.is_null() {
            CloseClipboard();
            return None;
        }

        let drop_ptr = GlobalLock(handle);
        if drop_ptr.is_null() {
            CloseClipboard();
            return None;
        }

        // DROPFILES 结构体：pFiles 偏移量在第一个 DWORD (offset 0)
        let file_offset = *(drop_ptr as *const u32) as usize;
        let file_start = drop_ptr.add(file_offset) as *const u16;

        // 解析双 null 终止的 UTF-16 字符串列表
        let mut files = Vec::new();
        let mut pos = 0usize;
        loop {
            // 收集一个 null 终止的字符串
            let mut chars = Vec::new();
            loop {
                let ch = *file_start.add(pos);
                pos += 1;
                if ch == 0 {
                    break;
                }
                chars.push(ch);
            }
            if chars.is_empty() {
                break; // 双 null：列表结束
            }
            let path_str = String::from_utf16_lossy(&chars);
            files.push(std::path::PathBuf::from(path_str));
        }

        GlobalUnlock(handle);
        CloseClipboard();

        if files.is_empty() { None } else { Some(files) }
    }
}

/// 弹出不可用对话框（双按钮：确定 + 不再提醒）
/// 返回 1 = 确定，2 = 不再提醒
#[cfg(target_os = "windows")]
fn show_unavailable_dialog(msg: &str) -> isize {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::iter::once;

    const WS_POPUP: u32 = 0x80000000;
    const WS_VISIBLE: u32 = 0x10000000;
    const WS_CAPTION: u32 = 0x00C00000;
    const WS_SYSMENU: u32 = 0x00080000;
    const DS_MODALFRAME: u32 = 0x80;
    const DS_CENTER: u32 = 0x800;
    const DS_SETFONT: u32 = 0x40;
    const BS_DEFPUSHBUTTON: u32 = 0x01;
    const WS_CHILD: u32 = 0x40000000;
    const WS_TABSTOP: u32 = 0x00010000;
    const SS_ICON: u32 = 0x00000003;
    const IDOK: u16 = 1;
    const IDSUPPRESS: u16 = 101;

    trait BytesExt {
        fn push_u32(&mut self, v: u32);
        fn push_u16(&mut self, v: u16);
        fn push_i16(&mut self, v: i16);
        fn push_str16(&mut self, s: &str);
        fn align4(&mut self);
    }

    impl BytesExt for Vec<u8> {
        fn push_u32(&mut self, v: u32) { self.extend_from_slice(&v.to_ne_bytes()); }
        fn push_u16(&mut self, v: u16) { self.extend_from_slice(&v.to_ne_bytes()); }
        fn push_i16(&mut self, v: i16) { self.extend_from_slice(&v.to_ne_bytes()); }
        fn push_str16(&mut self, s: &str) {
            let w: Vec<u16> = OsStr::new(s).encode_wide().chain(once(0u16)).collect();
            for &c in &w { self.push_u16(c); }
        }
        fn align4(&mut self) { while self.len() % 4 != 0 { self.push(0); } }
    }

    let mut b = Vec::new();

    // DLGTEMPLATE
    b.push_u32(WS_POPUP | WS_VISIBLE | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER | DS_SETFONT);
    b.push_u32(0);
    b.push_u16(4); // cdit = 4 controls
    b.push_i16(0); b.push_i16(0);
    b.push_i16(300); b.push_i16(110);
    b.push_u16(0); // menu
    b.push_u16(0); // class
    b.push_str16("clipImg");
    b.push_u16(9); // font size
    b.push_str16("Segoe UI");

    // Control 1: Warning icon
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | SS_ICON);
    b.push_u32(0);
    b.push_i16(10); b.push_i16(12); b.push_i16(32); b.push_i16(32);
    b.push_u16(0);
    b.push_u16(0xFFFF); b.push_u16(0x0082); // STATIC
    b.push_u16(0xFFFF); b.push_u16(103); // IDI_WARNING
    b.push_u16(0);

    // Control 2: Message text
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(50); b.push_i16(10); b.push_i16(240); b.push_i16(55);
    b.push_u16(0);
    b.push_u16(0xFFFF); b.push_u16(0x0082); // STATIC
    b.push_str16(msg);
    b.push_u16(0);

    // Control 3: OK button (default)
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON);
    b.push_u32(0);
    b.push_i16(145); b.push_i16(80); b.push_i16(60); b.push_i16(20);
    b.push_u16(IDOK);
    b.push_u16(0xFFFF); b.push_u16(0x0080); // BUTTON
    b.push_str16("确定");
    b.push_u16(0);

    // Control 4: "不再提醒" button
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP);
    b.push_u32(0);
    b.push_i16(210); b.push_i16(80); b.push_i16(75); b.push_i16(20);
    b.push_u16(IDSUPPRESS);
    b.push_u16(0xFFFF); b.push_u16(0x0080); // BUTTON
    b.push_str16("不再提醒");
    b.push_u16(0);

    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::DialogBoxIndirectParamW(
            std::ptr::null_mut(),
            b.as_ptr() as *const _,
            std::ptr::null_mut(),
            Some(unavailable_dialog_proc),
            0,
        )
    }
}

/// 不可用对话框过程
#[cfg(target_os = "windows")]
unsafe extern "system" fn unavailable_dialog_proc(
    hwnd: *mut std::ffi::c_void,
    msg: u32,
    wparam: usize,
    _lparam: isize,
) -> isize {
    const WM_COMMAND: u32 = 0x0111;
    const WM_CLOSE: u32 = 0x0010;
    const IDOK: u16 = 1;
    const IDSUPPRESS: u16 = 101;

    match msg {
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            match id {
                IDOK => {
                    windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 1);
                    1
                }
                IDSUPPRESS => {
                    windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 2);
                    1
                }
                _ => 0,
            }
        }
        WM_CLOSE => {
            windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 1);
            1
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::fs;

    struct TestEnv {
        dir: tempfile::TempDir,
        config: AppConfig,
    }

    impl TestEnv {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let save_path = dir.path().join(".clip");
            let config = AppConfig {
                save_dir: save_path.to_str().unwrap().to_string(),
                max_history_hours: 1,
                ..Default::default()
            };
            Self { dir, config }
        }

        fn watcher(&self) -> ClipboardWatcher {
            let watcher = ClipboardWatcher::new(self.config.clone(), self.dir.path());
            watcher.ensure_dir().unwrap();
            watcher
        }

        fn create_test_file(&self, name: &str, content: &[u8]) -> PathBuf {
            let clip_dir = self.dir.path().join(".clip");
            let _ = fs::create_dir_all(&clip_dir);
            let path = clip_dir.join(name);
            fs::write(&path, content).unwrap();
            path
        }

        fn create_test_png(&self, name: &str, color: [u8; 4]) -> PathBuf {
            let clip_dir = self.dir.path().join(".clip");
            let _ = fs::create_dir_all(&clip_dir);
            let path = clip_dir.join(name);
            let img = RgbaImage::from_pixel(10, 10, Rgba(color));
            img.save_with_format(&path, ImageFormat::Png).unwrap();
            path
        }
    }

    #[test]
    fn test_ensure_dir_creates_directory() {
        let env = TestEnv::new();
        let save = env.dir.path().join(".clip");
        assert!(!save.exists());
        env.watcher();
        assert!(save.exists());
    }

    #[test]
    fn test_poll_with_data_new_image() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let rgba: Vec<u8> = vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        let result = watcher.poll_with_data(2, 2, &rgba);

        assert!(result.is_some(), "首次应该保存为新图片");

        let clip_dir = env.dir.path().join(".clip");
        let clip_count = fs::read_dir(&clip_dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_str().map(|n| n.starts_with("clip_")).unwrap_or(false))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(clip_count, 1);
    }

    #[test]
    fn test_poll_with_data_duplicate() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let rgba: Vec<u8> = [255u8, 0, 0, 255].repeat(4);
        watcher.poll_with_data(2, 2, &rgba);

        let result = watcher.poll_with_data(2, 2, &rgba);
        assert!(result.is_none(), "相同图片不应重复保存");

        let clip_dir = env.dir.path().join(".clip");
        let clip_count = fs::read_dir(&clip_dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_str().map(|n| n.starts_with("clip_")).unwrap_or(false))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(clip_count, 1);
    }

    #[test]
    fn test_poll_with_data_different_images() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let red: Vec<u8> = [255u8, 0, 0, 255].repeat(4);
        let blue: Vec<u8> = [0u8, 0, 255, 255].repeat(4);

        assert!(watcher.poll_with_data(2, 2, &red).is_some());
        assert!(watcher.poll_with_data(2, 2, &blue).is_some());

        let clip_dir = env.dir.path().join(".clip");
        let clip_count = fs::read_dir(&clip_dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_str().map(|n| n.starts_with("clip_")).unwrap_or(false))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(clip_count, 2);
    }

    #[test]
    fn test_tmp_file_cleaned_after_duplicate() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let rgba: Vec<u8> = [128u8, 64, 32, 255].repeat(4);
        watcher.poll_with_data(2, 2, &rgba);
        watcher.poll_with_data(2, 2, &rgba);

        assert!(!env.dir.path().join(".clip/_tmp_clip.png").exists());
    }

    #[test]
    fn test_timestamp_naming_format() {
        let timestamp = filename_timestamp();
        let name = format!("clip_{}.png", timestamp);
        assert!(name.starts_with("clip_"));
        assert!(name.ends_with(".png"));
        // clip_YYYYMMDD_HHmmSSmmm.png = 4 + 1 + 8 + 1 + 6 + 3 + 4 = 27
        assert_eq!(name.len(), 27);
    }

    #[test]
    fn test_clean_does_not_touch_non_clip_files() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let clip_dir = env.dir.path().join(".clip");

        env.create_test_png("_tmp_clip.png", [0, 255, 0, 255]);
        env.create_test_png("clip_20260407_120000123.png", [0, 0, 255, 255]);

        watcher.clean_old_files();

        assert!(clip_dir.join("_tmp_clip.png").exists());
    }

    #[test]
    fn test_file_md5_same_file() {
        let env = TestEnv::new();
        let _watcher = env.watcher();
        let path = env.create_test_png("hash.png", [42, 42, 42, 255]);

        let h1 = file_md5(&path);
        let h2 = file_md5(&path);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_file_md5_different_files() {
        let env = TestEnv::new();
        let _watcher = env.watcher();
        let a = env.create_test_png("a.png", [1, 2, 3, 255]);
        let b = env.create_test_png("b.png", [4, 5, 6, 255]);

        assert_ne!(file_md5(&a), file_md5(&b));
    }

    #[test]
    fn test_file_md5_missing() {
        assert!(file_md5(Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn test_clean_old_files_by_hours() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 创建一个过期的文件（修改时间设为 2 小时前）
        let old_path = clip_dir.join("clip_20260407_100000123.png");
        let img = RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255]));
        img.save_with_format(&old_path, ImageFormat::Png).unwrap();

        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_path).and_then(|f| f.set_modified(two_hours_ago));

        // 创建一个新文件（不会过期）
        let new_path = clip_dir.join("clip_20260408_120000456.png");
        let img2 = RgbaImage::from_pixel(10, 10, Rgba([0, 255, 0, 255]));
        img2.save_with_format(&new_path, ImageFormat::Png).unwrap();

        let watcher = env.watcher();
        let deleted = watcher.clean_old_files();

        assert_eq!(deleted, 1, "应该只删除 1 个过期文件");
        assert!(!old_path.exists(), "过期文件应被删除");
        assert!(new_path.exists(), "未过期文件应保留");
    }

    #[test]
    fn test_copy_file_basic() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let src = env.dir.path().join("test_doc.txt");
        fs::write(&src, b"hello world").unwrap();

        let result = watcher.copy_file(&src);
        assert!(result.is_some());
        let name = result.unwrap();
        assert!(name.starts_with("clip_"));
        assert!(name.ends_with(".txt"));
        assert!(env.dir.path().join(".clip").join(&name).exists());
    }

    #[test]
    fn test_copy_file_no_extension() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let src = env.dir.path().join("Makefile");
        fs::write(&src, b"all: build").unwrap();

        let ext = watcher.copy_file(&src);
        assert!(ext.is_some());
        let name = ext.unwrap();
        assert!(name.starts_with("clip_"));
        assert!(!name.contains('.'));
    }

    #[test]
    fn test_copy_file_too_large() {
        let env = TestEnv::new();
        let mut config = env.config.clone();
        config.max_copy_size_mb = 0;
        let watcher = ClipboardWatcher::new(config, env.dir.path());
        watcher.ensure_dir().unwrap();

        let src = env.dir.path().join("big_file.bin");
        fs::write(&src, vec![0u8; 1024]).unwrap();

        let ext = watcher.copy_file(&src);
        assert!(ext.is_none(), "超大文件应被跳过");
    }

    #[test]
    fn test_is_png_file() {
        let env = TestEnv::new();
        let png_path = env.create_test_png("test.png", [255, 0, 0, 255]);
        assert!(ClipboardWatcher::is_png_file(&png_path));

        let txt_path = env.dir.path().join("test.txt");
        fs::write(&txt_path, b"not a png").unwrap();
        assert!(!ClipboardWatcher::is_png_file(&txt_path));
    }

    #[test]
    fn test_migrate_legacy_files() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 创建旧版 latest_file.png
        fs::write(clip_dir.join("latest_file.png"), b"old latest").unwrap();
        // 创建旧版 latest.png
        fs::write(clip_dir.join("latest.png"), b"very old").unwrap();

        let watcher = env.watcher();
        watcher.migrate_legacy_files();

        // 旧文件应该不存在了
        assert!(!clip_dir.join("latest_file.png").exists());
        assert!(!clip_dir.join("latest.png").exists());

        // 应该有对应的 clip_* 文件
        let clip_count = fs::read_dir(&clip_dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name().to_str().map(|n| n.starts_with("clip_")).unwrap_or(false))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(clip_count, 2);
    }

    #[test]
    fn test_find_latest_clip() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        let watcher = env.watcher();

        // 没有 clip 文件
        assert!(watcher.find_latest_clip().is_none());

        // 创建两个 clip 文件，第二个更新
        let path1 = clip_dir.join("clip_20260416_100000123.png");
        let img1 = RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255]));
        img1.save_with_format(&path1, ImageFormat::Png).unwrap();
        let one_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let _ = std::fs::File::open(&path1).and_then(|f| f.set_modified(one_hour_ago));

        let path2 = clip_dir.join("clip_20260416_110000456.png");
        let img2 = RgbaImage::from_pixel(10, 10, Rgba([0, 255, 0, 255]));
        img2.save_with_format(&path2, ImageFormat::Png).unwrap();

        let result = watcher.find_latest_clip();
        assert!(result.is_some());
        let (_, name) = result.unwrap();
        assert_eq!(name, "clip_20260416_110000456.png");
    }

    #[test]
    fn test_clean_handles_multiple_extensions() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        let old_png = clip_dir.join("clip_20260407_100000123.png");
        fs::write(&old_png, b"png data").unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_png).and_then(|f| f.set_modified(two_hours_ago));

        let old_pdf = clip_dir.join("clip_20260407_100001456.pdf");
        fs::write(&old_pdf, b"pdf data").unwrap();
        let _ = std::fs::File::open(&old_pdf).and_then(|f| f.set_modified(two_hours_ago));

        let watcher = env.watcher();
        let deleted = watcher.clean_old_files();

        assert_eq!(deleted, 2, "应删除 png 和 pdf 两个过期文件");
    }

    #[test]
    fn test_unique_history_path_conflict_format() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        fs::write(clip_dir.join("clip_20260415_120000123.pdf"), b"first").unwrap();

        let watcher = env.watcher();
        let path = watcher.unique_history_path("20260415_120000123", "pdf");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "clip_20260415_120000123_1.pdf"
        );
    }

    #[test]
    fn test_clean_old_files_zero_hours_no_cleanup() {
        let env = TestEnv::new();
        let mut config = env.config.clone();
        config.max_history_hours = 0;
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        let old_path = clip_dir.join("clip_20260407_100000123.png");
        let img = RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255]));
        img.save_with_format(&old_path, ImageFormat::Png).unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_path).and_then(|f| f.set_modified(two_hours_ago));

        let watcher = ClipboardWatcher::new(config, env.dir.path());
        watcher.ensure_dir().unwrap();
        let deleted = watcher.clean_old_files();

        assert_eq!(deleted, 0, "max_history_hours=0 不应清理任何文件");
        assert!(old_path.exists(), "过期文件应保留");
    }

    #[test]
    fn test_copy_files_multiple() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let src1 = env.dir.path().join("doc1.txt");
        let src2 = env.dir.path().join("doc2.pdf");
        fs::write(&src1, b"hello").unwrap();
        fs::write(&src2, b"world").unwrap();

        let results = watcher.copy_files(&[src1, src2]);
        assert_eq!(results.len(), 2);
        assert!(results[0].ends_with(".txt"));
        assert!(results[1].ends_with(".pdf"));
    }

    #[test]
    fn test_copy_files_respects_max() {
        let env = TestEnv::new();
        let mut config = env.config.clone();
        config.max_copy_files = 1;
        let watcher = ClipboardWatcher::new(config, env.dir.path());
        watcher.ensure_dir().unwrap();

        let src1 = env.dir.path().join("doc1.txt");
        let src2 = env.dir.path().join("doc2.pdf");
        let src3 = env.dir.path().join("doc3.md");
        fs::write(&src1, b"hello").unwrap();
        fs::write(&src2, b"world").unwrap();
        fs::write(&src3, b"foo").unwrap();

        let results = watcher.copy_files(&[src1, src2, src3]);
        assert_eq!(results.len(), 1, "max_copy_files=1 应只保存 1 个文件");
    }

    #[test]
    fn test_copy_file_mtime_not_preserved() {
        // 模拟源文件有旧 mtime，复制后不应被 clean_old_files 删除
        let env = TestEnv::new();
        let watcher = env.watcher();

        // 创建一个 mtime 为 2 天前的源文件
        let src = env.dir.path().join("old_file.txt");
        fs::write(&src, b"old content").unwrap();
        let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(172800);
        let _ = std::fs::File::open(&src).and_then(|f| f.set_modified(two_days_ago));

        let result = watcher.copy_file(&src);
        assert!(result.is_some(), "文件应成功复制");

        let saved_name = result.unwrap();
        let saved_path = env.dir.path().join(".clip").join(&saved_name);

        // 复制后文件应存在
        assert!(saved_path.exists(), "复制后的文件应存在");

        // 运行清理，文件不应被删除（mtime 已被刷新为当前时间）
        let deleted = watcher.clean_old_files();
        assert_eq!(deleted, 0, "刚复制的文件不应被清理");
        assert!(saved_path.exists(), "刚复制的文件在清理后应仍然存在");
    }

    #[test]
    fn test_copy_files_batch_not_cleaned() {
        // 批量复制多个旧 mtime 文件，全部不应被 clean_old_files 删除
        let env = TestEnv::new();
        let watcher = env.watcher();
        let two_days_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(172800);

        let mut srcs = Vec::new();
        for i in 0..3 {
            let src = env.dir.path().join(format!("old_{}.txt", i));
            fs::write(&src, format!("content {}", i).as_bytes()).unwrap();
            let _ = std::fs::File::open(&src).and_then(|f| f.set_modified(two_days_ago));
            srcs.push(src);
        }

        let results = watcher.copy_files(&srcs);
        assert_eq!(results.len(), 3, "应保存全部 3 个文件");

        // 所有文件在清理后应仍然存在
        let deleted = watcher.clean_old_files();
        assert_eq!(deleted, 0, "批量复制的文件不应被清理");

        let clip_dir = env.dir.path().join(".clip");
        for name in &results {
            assert!(clip_dir.join(name).exists(), "文件 {} 清理后应存在", name);
        }
    }
}
