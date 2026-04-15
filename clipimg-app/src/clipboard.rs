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
    /// 最近一次操作的容器侧完整路径（如 /workspace/.clip/latest_file.png）
    pub latest_container_path: std::cell::RefCell<String>,
}

impl ClipboardWatcher {
    pub fn new(config: AppConfig, exe_dir: &Path) -> Self {
        let save_dir = config.resolved_save_dir(exe_dir);
        let latest_container_path = config.resolved_output_path_for("png");
        Self {
            config,
            save_dir,
            last_md5: std::cell::RefCell::new(None),
            latest_container_path: std::cell::RefCell::new(latest_container_path),
        }
    }

    /// 确保保存目录存在
    pub fn ensure_dir(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.save_dir)?;
        Ok(())
    }

    /// 从磁盘上已存在的 latest_file.* 恢复 latest_container_path
    pub fn sync_latest_from_disk(&self) {
        if let Ok(entries) = fs::read_dir(&self.save_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "latest_file" {
                    *self.latest_container_path.borrow_mut() =
                        self.config.resolved_output_path_for("");
                    log::info!("从磁盘恢复 latest_container_path: latest_file");
                    return;
                } else if name.starts_with("latest_file.") {
                    let ext = name.trim_start_matches("latest_file.").to_string();
                    let path = self.config.resolved_output_path_for(&ext);
                    *self.latest_container_path.borrow_mut() = path;
                    log::info!("从磁盘恢复 latest_container_path: {}", name);
                    return;
                }
            }
        }
    }

    /// 用 RGBA 像素数据轮询并保存（平台无关的核心逻辑）
    /// 返回 true 表示有新图片保存
    pub fn poll_with_data(&self, width: usize, height: usize, rgba: &[u8]) -> bool {
        // 先在内存中计算 MD5，与上次保存的比对
        let current_md5 = {
            let mut hasher = Md5::new();
            hasher.update(rgba);
            format!("{:x}", hasher.finalize())
        };

        {
            let last = self.last_md5.borrow();
            if last.as_ref() == Some(&current_md5) {
                return false; // 内容没变，不写磁盘
            }
        }

        // 内容有变化，执行保存
        let tmp_path = self.save_dir.join("_tmp_clip.png");

        if let Err(e) = self.save_rgba_to_png(width, height, rgba, &tmp_path) {
            log::warn!("保存临时文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return false;
        }

        let timestamp = filename_timestamp();
        let history_path = self.unique_history_path(&timestamp, "png");

        if let Err(e) = fs::rename(&tmp_path, &history_path) {
            log::warn!("重命名历史文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return false;
        }

        self.update_latest_from_history(&history_path, "png");

        // 更新缓存的 MD5
        *self.last_md5.borrow_mut() = Some(current_md5);

        log::info!("新图片已保存: {}", history_path.display());
        self.clean_old_files();
        true
    }

    /// 从 CF_HDROP 文件复制保存
    /// 返回 Some(ext) 表示成功保存，ext 是文件扩展名（可能为空）
    /// 返回 None 表示跳过
    pub fn copy_file(&self, src_path: &Path) -> Option<String> {
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

        self.update_latest_from_history(&history_path, &extension);

        log::info!("新文件已保存: {}", history_path.display());
        self.clean_old_files();
        Some(extension)
    }

    /// 将历史文件更新为 latest_file
    fn update_latest_from_history(&self, history_path: &Path, extension: &str) {
        // 删除旧的 latest_file.*
        self.remove_latest_file();

        // 复制历史文件为 latest_file.xxx
        let latest_name = if extension.is_empty() {
            "latest_file".to_string()
        } else {
            format!("latest_file.{}", extension)
        };
        let latest_path = self.save_dir.join(&latest_name);
        if let Err(e) = fs::copy(history_path, &latest_path) {
            log::warn!("更新 {} 失败: {}", latest_name, e);
        }

        // 更新容器侧路径
        let container_path = self.config.resolved_output_path_for(extension);
        *self.latest_container_path.borrow_mut() = container_path;
    }

    /// 删除目录中所有 latest_file.* 文件
    fn remove_latest_file(&self) {
        if let Ok(entries) = fs::read_dir(&self.save_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == "latest_file" || name_str.starts_with("latest_file.") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    /// 迁移旧版 latest.png 为 latest_file.png
    pub fn migrate_old_latest(&self) {
        let old_path = self.save_dir.join("latest.png");
        let new_path = self.save_dir.join("latest_file.png");

        if !old_path.exists() {
            return;
        }

        if new_path.exists() {
            // latest_file.png 已存在，删除旧版残留
            let _ = fs::remove_file(&old_path);
            log::info!("已清理旧版 latest.png 残留");
        } else {
            // 重命名旧版
            if let Err(e) = fs::rename(&old_path, &new_path) {
                log::warn!("迁移 latest.png → latest_file.png 失败: {}", e);
            } else {
                log::info!("已迁移 latest.png → latest_file.png");
            }
        }
    }

    /// 生成唯一的历史文件路径，避免同一秒内的文件名冲突
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
        // 同一秒内追加序号
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
    pub fn poll(&self, clipboard: &mut arboard::Clipboard) -> bool {
        let image_data = match clipboard.get_image() {
            Ok(img) => img,
            Err(_) => return false,
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

        assert!(result, "首次应该保存为新图片");
        assert!(env.dir.path().join(".clip/latest_file.png").exists());

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
        assert!(!result, "相同图片不应重复保存");

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

        assert!(watcher.poll_with_data(2, 2, &red));
        assert!(watcher.poll_with_data(2, 2, &blue));

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
        assert_eq!(name.len(), 24);
    }

    #[test]
    fn test_clean_does_not_touch_non_clip_files() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let clip_dir = env.dir.path().join(".clip");

        env.create_test_png("latest_file.png", [255, 0, 0, 255]);
        env.create_test_png("_tmp_clip.png", [0, 255, 0, 255]);
        env.create_test_png("clip_20260407_120000.png", [0, 0, 255, 255]);

        watcher.clean_old_files();

        assert!(clip_dir.join("latest_file.png").exists());
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
        let old_path = clip_dir.join("clip_20260407_100000.png");
        let img = RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255]));
        img.save_with_format(&old_path, ImageFormat::Png).unwrap();

        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_path).and_then(|f| f.set_modified(two_hours_ago));

        // 创建一个新文件（不会过期）
        let new_path = clip_dir.join("clip_20260408_120000.png");
        let img2 = RgbaImage::from_pixel(10, 10, Rgba([0, 255, 0, 255]));
        img2.save_with_format(&new_path, ImageFormat::Png).unwrap();

        // latest_file.png 应该保留
        let latest = clip_dir.join("latest_file.png");
        let img3 = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 255, 255]));
        img3.save_with_format(&latest, ImageFormat::Png).unwrap();

        let watcher = env.watcher();
        let deleted = watcher.clean_old_files();

        assert_eq!(deleted, 1, "应该只删除 1 个过期文件");
        assert!(!old_path.exists(), "过期文件应被删除");
        assert!(new_path.exists(), "未过期文件应保留");
        assert!(latest.exists(), "latest_file.png 应始终保留");
    }

    #[test]
    fn test_copy_file_basic() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        // 在临时位置创建一个测试文件
        let src = env.dir.path().join("test_doc.txt");
        fs::write(&src, b"hello world").unwrap();

        let ext = watcher.copy_file(&src);
        assert_eq!(ext, Some("txt".to_string()));
        assert!(env.dir.path().join(".clip/latest_file.txt").exists());

        // 检查容器侧路径
        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file.txt"
        );
    }

    #[test]
    fn test_copy_file_no_extension() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        let src = env.dir.path().join("Makefile");
        fs::write(&src, b"all: build").unwrap();

        let ext = watcher.copy_file(&src);
        assert_eq!(ext, Some("".to_string()));
        assert!(env.dir.path().join(".clip/latest_file").exists());
    }

    #[test]
    fn test_copy_file_too_large() {
        let env = TestEnv::new();
        let mut config = env.config.clone();
        config.max_copy_size_mb = 0; // 0 MB limit
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
    fn test_migrate_old_latest() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 创建旧版 latest.png
        let old = clip_dir.join("latest.png");
        fs::write(&old, b"old latest").unwrap();

        let watcher = env.watcher();
        watcher.migrate_old_latest();

        assert!(clip_dir.join("latest_file.png").exists());
        assert!(!clip_dir.join("latest.png").exists());
    }

    #[test]
    fn test_migrate_old_latest_both_exist() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 同时存在 latest.png 和 latest_file.png
        fs::write(clip_dir.join("latest.png"), b"old").unwrap();
        fs::write(clip_dir.join("latest_file.png"), b"new").unwrap();

        let watcher = env.watcher();
        watcher.migrate_old_latest();

        // latest.png 应被删除，latest_file.png 保留
        assert!(!clip_dir.join("latest.png").exists());
        assert_eq!(fs::read_to_string(clip_dir.join("latest_file.png")).unwrap(), "new");
    }

    #[test]
    fn test_latest_container_path_updated() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        // 默认是 png
        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file.png"
        );

        // 复制一个 txt 文件后路径改变
        let src = env.dir.path().join("doc.txt");
        fs::write(&src, b"test").unwrap();
        watcher.copy_file(&src);

        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file.txt"
        );
    }

    #[test]
    fn test_clean_handles_multiple_extensions() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 创建过期的 png 和 pdf 文件
        let old_png = clip_dir.join("clip_20260407_100000.png");
        fs::write(&old_png, b"png data").unwrap();
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_png).and_then(|f| f.set_modified(two_hours_ago));

        let old_pdf = clip_dir.join("clip_20260407_100001.pdf");
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

        // 创建同名文件模拟冲突
        fs::write(clip_dir.join("clip_20260415_120000.pdf"), b"first").unwrap();

        let watcher = env.watcher();
        let path = watcher.unique_history_path("20260415_120000", "pdf");
        // 序号应在扩展名前：clip_20260415_120000_1.pdf
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "clip_20260415_120000_1.pdf"
        );
    }

    #[test]
    fn test_clean_old_files_zero_hours_no_cleanup() {
        let env = TestEnv::new();
        let mut config = env.config.clone();
        config.max_history_hours = 0;
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 创建一个过期文件
        let old_path = clip_dir.join("clip_20260407_100000.png");
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
    fn test_sync_latest_from_disk_pdf() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        // 模拟磁盘上已有 latest_file.pdf
        fs::write(clip_dir.join("latest_file.pdf"), b"pdf data").unwrap();

        let watcher = env.watcher();
        // 默认路径是 png
        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file.png"
        );

        watcher.sync_latest_from_disk();

        // 同步后应为 pdf
        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file.pdf"
        );
    }

    #[test]
    fn test_sync_latest_from_disk_no_extension() {
        let env = TestEnv::new();
        let clip_dir = env.dir.path().join(".clip");
        fs::create_dir_all(&clip_dir).unwrap();

        fs::write(clip_dir.join("latest_file"), b"no ext").unwrap();

        let watcher = env.watcher();
        watcher.sync_latest_from_disk();

        assert_eq!(
            *watcher.latest_container_path.borrow(),
            "/workspace/.clip/latest_file"
        );
    }
}