use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_max_history_hours() -> u32 { 1 }
fn default_max_log_size_mb() -> u32 { 1 }

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 全局热键，如 "Alt+Insert"、"Ctrl+Shift+V"
    pub hotkey: String,
    /// 热键触发时输入到终端的路径（容器侧路径）
    pub output_path: String,
    /// 图片在 Windows 侧的保存目录（相对或绝对路径）
    pub save_dir: String,
    /// 剪贴板轮询间隔（毫秒）— 已废弃，保留用于配置文件兼容
    #[serde(default)]
    pub poll_interval_ms: u64,
    /// 历史图片最大保留小时数
    #[serde(default = "default_max_history_hours")]
    pub max_history_hours: u32,
    /// 日志文件最大大小（MB），超过后轮转
    #[serde(default = "default_max_log_size_mb")]
    pub max_log_size_mb: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "".to_string(),
            output_path: "/workspace/.clip/latest.png".to_string(),
            save_dir: ".clip".to_string(),
            poll_interval_ms: 800,
            max_history_hours: 1,
            max_log_size_mb: 1,
        }
    }
}

impl AppConfig {
    /// 从文件加载配置，文件不存在则创建默认配置
    pub fn load(config_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !config_path.exists() {
            let config = Self::default();
            config.save(config_path)?;
            log::info!("已创建默认配置文件: {}", config_path.display());
            return Ok(config);
        }

        let content = std::fs::read_to_string(config_path)?;
        let config: Self = serde_json::from_str(&content)?;
        config.validate()?;

        // 检测废弃字段
        if content.contains("poll_interval_ms") {
            log::warn!("poll_interval_ms 字段已废弃，剪贴板已改为事件驱动监听，建议从配置文件中删除该字段");
        }

        // 如果旧配置文件缺少字段，回写补全（如 max_history_hours）
        let normalized = serde_json::to_string_pretty(&config)?;
        if normalized != content {
            std::fs::write(config_path, &normalized)?;
            log::info!("配置文件已自动补全缺失字段: {}", config_path.display());
        }

        log::info!("已加载配置文件: {}", config_path.display());
        Ok(config)
    }

    /// 保存配置到文件
    pub fn save(&self, config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }

    /// 校验配置合法性
    pub fn validate(&self) -> Result<(), String> {
        // hotkey 为空表示使用多格式剪贴板模式（方案 C），是合法的
        if self.output_path.trim().is_empty() {
            return Err("output_path 不能为空".to_string());
        }
        if self.save_dir.trim().is_empty() {
            return Err("save_dir 不能为空".to_string());
        }
        if self.poll_interval_ms == 0 {
            return Err("poll_interval_ms 不能为 0".to_string());
        }
        Ok(())
    }

    /// 是否使用热键模式（方案 A）
    pub fn is_hotkey_mode(&self) -> bool {
        !self.hotkey.trim().is_empty()
    }

    /// 解析 save_dir 为绝对路径
    /// 相对路径基于 EXE 所在目录
    pub fn resolved_save_dir(&self, exe_dir: &Path) -> PathBuf {
        let save = Path::new(&self.save_dir);
        if save.is_absolute() || is_windows_absolute(&self.save_dir) {
            save.to_path_buf()
        } else {
            exe_dir.join(&self.save_dir)
        }
    }

    /// 获取 latest.png 的 Windows 侧完整路径
    pub fn latest_png_path(&self, exe_dir: &Path) -> PathBuf {
        self.resolved_save_dir(exe_dir).join("latest.png")
    }
}

/// 检测 Windows 风格绝对路径（如 C:\、E:\）
fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.hotkey, "");
        assert_eq!(config.output_path, "/workspace/.clip/latest.png");
        assert_eq!(config.save_dir, ".clip");
        assert_eq!(config.poll_interval_ms, 800);
        assert_eq!(config.max_history_hours, 1);
    }

    #[test]
    fn test_save_and_load() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");

        let original = AppConfig::default();
        original.save(&config_path).unwrap();

        let loaded = AppConfig::load(&config_path).unwrap();
        assert_eq!(original.hotkey, loaded.hotkey);
        assert_eq!(original.output_path, loaded.output_path);
        assert_eq!(original.save_dir, loaded.save_dir);
    }

    #[test]
    fn test_load_creates_default_when_missing() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");

        let config = AppConfig::load(&config_path).unwrap();
        assert!(config_path.exists());
        assert_eq!(config.hotkey, "");
    }

    #[test]
    fn test_load_invalid_json() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        fs::write(&config_path, "not valid json{{{").unwrap();

        let result = AppConfig::load(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_empty_hotkey_is_ok() {
        let mut config = AppConfig::default();
        config.hotkey = "".to_string();
        assert!(config.validate().is_ok()); // 空 hotkey 表示方案 C
        assert!(!config.is_hotkey_mode());
    }

    #[test]
    fn test_is_hotkey_mode() {
        let config = AppConfig::default();
        assert!(!config.is_hotkey_mode()); // 默认是剪贴板模式

        let mut config_with_hotkey = AppConfig::default();
        config_with_hotkey.hotkey = "Alt+Insert".to_string();
        assert!(config_with_hotkey.is_hotkey_mode());
    }

    #[test]
    fn test_validate_empty_output_path() {
        let mut config = AppConfig::default();
        config.output_path = "  ".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_zero_poll_interval() {
        let mut config = AppConfig::default();
        config.poll_interval_ms = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_resolved_save_dir_relative() {
        let config = AppConfig {
            save_dir: ".clip".to_string(),
            ..Default::default()
        };
        // 相对路径基于 EXE 所在目录
        let exe_dir = Path::new("/some/path/clipImg");
        let resolved = config.resolved_save_dir(exe_dir);
        assert_eq!(resolved, PathBuf::from("/some/path/clipImg/.clip"));
    }

    #[test]
    fn test_resolved_save_dir_absolute() {
        let config = AppConfig {
            save_dir: "E:\\WorkingProjects\\workspace\\.clip".to_string(),
            ..Default::default()
        };
        let exe_dir = Path::new("/some/path/clipImg/clipimg-app");
        let resolved = config.resolved_save_dir(exe_dir);
        assert_eq!(
            resolved,
            PathBuf::from("E:\\WorkingProjects\\workspace\\.clip")
        );
    }

    #[test]
    fn test_latest_png_path() {
        let config = AppConfig {
            save_dir: ".clip".to_string(),
            ..Default::default()
        };
        let exe_dir = Path::new("/workspace/clipImg");
        let latest = config.latest_png_path(exe_dir);
        assert_eq!(latest, PathBuf::from("/workspace/clipImg/.clip/latest.png"));
    }

    #[test]
    fn test_load_old_config_missing_max_history_hours() {
        // 模拟旧配置文件（有 max_history_days 但没有 max_history_hours）
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let old_json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip/latest.png",
            "save_dir": ".clip",
            "poll_interval_ms": 800,
            "max_history_days": 7
        }"#;
        fs::write(&config_path, old_json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.max_history_hours, 1, "缺失 max_history_hours 应使用默认值 1");
    }

    #[test]
    fn test_load_config_with_max_history_hours() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip/latest.png",
            "save_dir": ".clip",
            "poll_interval_ms": 800,
            "max_history_hours": 4
        }"#;
        fs::write(&config_path, json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.max_history_hours, 4);
    }
}
