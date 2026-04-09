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
}

impl ClipboardWatcher {
    pub fn new(config: AppConfig, exe_dir: &Path) -> Self {
        let save_dir = config.resolved_save_dir(exe_dir);
        Self { config, save_dir }
    }

    /// 确保保存目录存在
    pub fn ensure_dir(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.save_dir)?;
        Ok(())
    }

    /// 用 RGBA 像素数据轮询并保存（平台无关的核心逻辑）
    /// 返回 true 表示有新图片保存
    pub fn poll_with_data(&self, width: usize, height: usize, rgba: &[u8]) -> bool {
        let tmp_path = self.save_dir.join("_tmp_clip.png");

        if let Err(e) = self.save_rgba_to_png(width, height, rgba, &tmp_path) {
            log::warn!("保存临时文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return false;
        }

        let latest_path = self.save_dir.join("latest.png");

        if self.is_duplicate(&tmp_path, &latest_path) {
            let _ = fs::remove_file(&tmp_path);
            return false;
        }

        let timestamp = filename_timestamp();
        let history_path = self.unique_history_path(&timestamp);

        if let Err(e) = fs::rename(&tmp_path, &history_path) {
            log::warn!("重命名历史文件失败: {}", e);
            let _ = fs::remove_file(&tmp_path);
            return false;
        }

        if let Err(e) = fs::copy(&history_path, &latest_path) {
            log::warn!("更新 latest.png 失败: {}", e);
        }

        log::info!("新图片已保存: {}", history_path.display());
        true
    }

    /// 生成唯一的历史文件路径，避免同一秒内的文件名冲突
    fn unique_history_path(&self, timestamp: &str) -> PathBuf {
        let base = self.save_dir.join(format!("clip_{}.png", timestamp));
        if !base.exists() {
            return base;
        }
        // 同一秒内追加序号
        for i in 1..100 {
            let path = self.save_dir.join(format!("clip_{}_{}.png", timestamp, i));
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

    /// 两级去重：文件大小 → MD5 哈希
    pub fn is_duplicate(&self, new_path: &Path, latest_path: &Path) -> bool {
        if !latest_path.exists() {
            return false;
        }

        let new_size = match fs::metadata(new_path) {
            Ok(m) => m.len(),
            Err(_) => return false,
        };
        let latest_size = match fs::metadata(latest_path) {
            Ok(m) => m.len(),
            Err(_) => return false,
        };

        if new_size != latest_size {
            return false;
        }

        let new_hash = file_md5(new_path);
        let latest_hash = file_md5(latest_path);

        new_hash == latest_hash
    }

    /// 清理超过 max_history_hours 的历史图片
    pub fn clean_old_files(&self) -> usize {
        let max_hours = self.config.max_history_hours;
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

            if !name.starts_with("clip_") || !name.ends_with(".png") {
                continue;
            }

            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        if fs::remove_file(&path).is_ok() {
                            log::info!("已删除过期图片: {}", name);
                            deleted += 1;
                        }
                    }
                }
            }
        }

        if deleted > 0 {
            self.update_latest_to_newest();
        }

        deleted
    }

    /// 将 latest.png 更新为最新的历史图片
    fn update_latest_to_newest(&self) {
        let entries = match fs::read_dir(&self.save_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;

        for entry in entries.flatten() {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.starts_with("clip_") || !name.ends_with(".png") {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if newest.as_ref().map_or(true, |(t, _)| modified > *t) {
                        newest = Some((modified, path));
                    }
                }
            }
        }

        if let Some((_, path)) = newest {
            let latest = self.save_dir.join("latest.png");
            if let Err(e) = fs::copy(&path, &latest) {
                log::warn!("更新 latest.png 失败: {}", e);
            }
        }
    }
}

/// 计算文件 MD5（独立函数，方便测试）
pub fn file_md5(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;
    let mut hasher = Md5::new();
    hasher.update(&data);
    Some(format!("{:x}", hasher.finalize()))
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

        fn create_test_png(&self, name: &str, color: [u8; 4]) -> PathBuf {
            let path = self.dir.path().join(".clip").join(name);
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
    fn test_is_duplicate_same_file() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let path = env.create_test_png("test.png", [255, 0, 0, 255]);
        assert!(watcher.is_duplicate(&path, &path));
    }

    #[test]
    fn test_is_duplicate_different_files() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let a = env.create_test_png("a.png", [255, 0, 0, 255]);
        let b = env.create_test_png("b.png", [0, 255, 0, 255]);
        assert!(!watcher.is_duplicate(&a, &b));
    }

    #[test]
    fn test_is_duplicate_no_latest() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let new = env.create_test_png("new.png", [255, 0, 0, 255]);
        let missing = env.dir.path().join(".clip").join("nonexistent.png");
        assert!(!watcher.is_duplicate(&new, &missing));
    }

    #[test]
    fn test_is_duplicate_same_size_different_content() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let red = env.create_test_png("red.png", [255, 0, 0, 255]);
        let blue = env.create_test_png("blue.png", [0, 0, 255, 255]);
        assert!(!watcher.is_duplicate(&red, &blue));
    }

    #[test]
    fn test_poll_with_data_new_image() {
        let env = TestEnv::new();
        let watcher = env.watcher();

        // 创建一张 2x2 红色图片
        let rgba: Vec<u8> = vec![255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255];
        let result = watcher.poll_with_data(2, 2, &rgba);

        assert!(result, "首次应该保存为新图片");

        // 验证 latest.png 存在
        assert!(env.dir.path().join(".clip/latest.png").exists());

        // 验证有一个 clip_*.png 文件
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

        let rgba: Vec<u8> = [255u8, 0, 0, 255].repeat(4); // 2x2 红色
        watcher.poll_with_data(2, 2, &rgba);

        // 再次发送相同数据 → 应判定为重复
        let result = watcher.poll_with_data(2, 2, &rgba);
        assert!(!result, "相同图片不应重复保存");

        // 应该仍然只有一个 clip_ 文件
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
        watcher.poll_with_data(2, 2, &rgba); // 重复

        // _tmp_clip.png 应该被清理
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

        env.create_test_png("latest.png", [255, 0, 0, 255]);
        env.create_test_png("_tmp_clip.png", [0, 255, 0, 255]);
        env.create_test_png("clip_20260407_120000.png", [0, 0, 255, 255]);

        watcher.clean_old_files();

        assert!(clip_dir.join("latest.png").exists());
        assert!(clip_dir.join("_tmp_clip.png").exists());
    }

    #[test]
    fn test_file_md5_same_file() {
        let env = TestEnv::new();
        let watcher = env.watcher();
        let path = env.create_test_png("hash.png", [42, 42, 42, 255]);

        let h1 = file_md5(&path);
        let h2 = file_md5(&path);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_file_md5_different_files() {
        let env = TestEnv::new();
        env.watcher(); // 确保 .clip 目录存在
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

        // 将修改时间设为 2 小时前
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let _ = std::fs::File::open(&old_path).and_then(|f| f.set_modified(two_hours_ago));

        // 创建一个新文件（不会过期）
        let new_path = clip_dir.join("clip_20260408_120000.png");
        let img2 = RgbaImage::from_pixel(10, 10, Rgba([0, 255, 0, 255]));
        img2.save_with_format(&new_path, ImageFormat::Png).unwrap();

        // latest.png 应该保留
        let latest = clip_dir.join("latest.png");
        let img3 = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 255, 255]));
        img3.save_with_format(&latest, ImageFormat::Png).unwrap();

        let watcher = env.watcher();
        let deleted = watcher.clean_old_files();

        assert_eq!(deleted, 1, "应该只删除 1 个过期文件");
        assert!(!old_path.exists(), "过期文件应被删除");
        assert!(new_path.exists(), "未过期文件应保留");
        assert!(latest.exists(), "latest.png 应始终保留");
    }
}
