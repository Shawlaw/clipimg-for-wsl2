use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_max_history_hours() -> u32 {
    1
}
fn default_max_log_size_mb() -> u32 {
    1
}
fn default_max_copy_size_mb() -> u32 {
    10
}
fn default_max_copy_files() -> u32 {
    10
}
fn default_preview_hotkey() -> String {
    "Ctrl+Alt+P".to_string()
}
fn default_blocked_preview_ext() -> Vec<String> {
    Vec::new()
}
fn default_show_startup_notification() -> bool {
    true
}
fn default_wsl2_path_conversion() -> bool {
    true
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 全局热键，如 "Alt+Insert"、"Ctrl+Shift+V"
    pub hotkey: String,
    /// 容器侧目录路径（目录级，不含 latest.png）
    /// 粘贴/输入到终端时自动拼接 /latest.png
    pub output_path: String,
    /// 图片在 Windows 侧的保存目录（相对或绝对路径）
    pub save_dir: String,
    /// 历史图片最大保留小时数
    #[serde(default = "default_max_history_hours")]
    pub max_history_hours: u32,
    /// 日志文件最大大小（MB），超过后轮转
    #[serde(default = "default_max_log_size_mb")]
    pub max_log_size_mb: u32,
    /// CF_HDROP 文件最大允许大小（MB），超过则跳过
    #[serde(default = "default_max_copy_size_mb")]
    pub max_copy_size_mb: u32,
    /// 单次 CF_HDROP 最多处理的文件数，超过则跳过
    #[serde(default = "default_max_copy_files")]
    pub max_copy_files: u32,
    /// 预览快捷键，如 "Ctrl+Alt+P"，空字符串表示不启用
    #[serde(default = "default_preview_hotkey")]
    pub preview_hotkey: String,
    /// 用户自定义的预览拦截后缀名列表（与内置列表取并集）
    /// 示例: ["dll", "sys", "reg"]
    #[serde(default = "default_blocked_preview_ext")]
    pub blocked_preview_ext: Vec<String>,
    /// 启动时是否显示提示弹窗
    #[serde(default = "default_show_startup_notification")]
    pub show_startup_notification: bool,
    /// 是否启用 WSL2 路径转化（关闭后剪贴板使用 Windows 原生路径）
    #[serde(default = "default_wsl2_path_conversion")]
    pub wsl2_path_conversion: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "".to_string(),
            output_path: "/workspace/.clip".to_string(),
            save_dir: ".clip".to_string(),
            max_history_hours: 1,
            max_log_size_mb: 1,
            max_copy_size_mb: 10,
            max_copy_files: 10,
            preview_hotkey: "Ctrl+Alt+P".to_string(),
            blocked_preview_ext: Vec::new(),
            show_startup_notification: true,
            wsl2_path_conversion: true,
        }
    }
}

impl AppConfig {
    /// 从文件加载配置，文件不存在则创建默认配置
    pub fn load(config_path: &Path) -> Result<Self, String> {
        if !config_path.exists() {
            let config = Self::default();
            config.save(config_path)?;
            log::info!("已创建默认配置文件: {}", config_path.display());
            return Ok(config);
        }

        let content = desktop_config::read_to_string(config_path)?;

        // 旧配置兼容：处理废弃字段和格式变化
        let migrated = Self::migrate_config(&content);
        let content_to_parse = if migrated != content {
            desktop_config::write_string(config_path, &migrated)?;
            log::info!("配置文件已自动迁移: {}", config_path.display());
            migrated
        } else {
            content
        };

        let mut config: Self = serde_json::from_str(&content_to_parse)
            .map_err(|err| format!("配置文件解析失败: {}", err))?;

        // 旧配置兼容：output_path 从文件级截断为目录级
        if config.output_path.ends_with("/latest.png") {
            let truncated = config
                .output_path
                .trim_end_matches("/latest.png")
                .to_string();
            log::warn!(
                "output_path 已从文件级自动截断为目录级: {} → {}",
                config.output_path,
                truncated
            );
            config.output_path = truncated;
            config.save(config_path)?;
        }

        config.validate()?;

        log::info!("已加载配置文件: {}", config_path.display());
        Ok(config)
    }

    /// 迁移配置文件内容（移除废弃字段、补充新字段）
    fn migrate_config(content: &str) -> String {
        let mut json: serde_json::Value = match serde_json::from_str(content) {
            Ok(v) => v,
            Err(_) => return content.to_string(),
        };

        let mut changed = false;

        if let Some(obj) = json.as_object_mut() {
            // 移除废弃字段
            if obj.remove("poll_interval_ms").is_some() {
                log::warn!("poll_interval_ms 字段已废弃，已从配置文件中自动删除");
                changed = true;
            }

            // 迁移 max_history_days → max_history_hours
            if !obj.contains_key("max_history_hours") {
                if let Some(days) = obj.remove("max_history_days").and_then(|v| v.as_u64()) {
                    let hours = days * 24;
                    log::warn!(
                        "max_history_days ({}) 已迁移为 max_history_hours ({})",
                        days,
                        hours
                    );
                    obj.insert("max_history_hours".to_string(), serde_json::json!(hours));
                    changed = true;
                }
            }

            // 补充 v1.0.6 新字段
            if !obj.contains_key("max_copy_size_mb") {
                obj.insert("max_copy_size_mb".to_string(), serde_json::json!(10));
                changed = true;
            }
            if !obj.contains_key("preview_hotkey") {
                obj.insert(
                    "preview_hotkey".to_string(),
                    serde_json::json!("Ctrl+Alt+P"),
                );
                changed = true;
            }
            if !obj.contains_key("blocked_preview_ext") {
                obj.insert("blocked_preview_ext".to_string(), serde_json::json!([]));
                changed = true;
            }
            if !obj.contains_key("show_startup_notification") {
                obj.insert(
                    "show_startup_notification".to_string(),
                    serde_json::json!(true),
                );
                changed = true;
            }

            // 补充 v1.0.8 新字段
            if !obj.contains_key("max_copy_files") {
                obj.insert("max_copy_files".to_string(), serde_json::json!(10));
                changed = true;
            }

            // 补充 v1.0.13 新字段
            if !obj.contains_key("wsl2_path_conversion") {
                obj.insert("wsl2_path_conversion".to_string(), serde_json::json!(true));
                changed = true;
            }
        }

        if changed {
            log::info!("配置文件已自动迁移（补充新字段/移除废弃字段）");
            serde_json::to_string_pretty(&json).unwrap_or_else(|_| content.to_string())
        } else {
            content.to_string()
        }
    }

    /// 保存配置到文件
    pub fn save(&self, config_path: &Path) -> Result<(), String> {
        desktop_config::save_pretty_json(config_path, self)
    }

    /// 校验配置合法性
    pub fn validate(&self) -> Result<(), String> {
        if self.output_path.trim().is_empty() {
            return Err("output_path 不能为空".to_string());
        }
        if self.save_dir.trim().is_empty() {
            return Err("save_dir 不能为空".to_string());
        }
        Ok(())
    }

    /// 是否使用热键模式（方案 A）
    pub fn is_hotkey_mode(&self) -> bool {
        !self.hotkey.trim().is_empty()
    }

    /// 获取容器侧目录路径（不含文件名）
    pub fn output_dir(&self) -> &str {
        self.output_path.trim_end_matches('/')
    }

    /// 给定 save_dir 下的文件名，返回容器侧完整路径
    /// 即 `output_path/filename`
    pub fn container_path_for(&self, filename: &str) -> String {
        format!("{}/{}", self.output_dir(), filename)
    }

    /// 给定文件名，返回剪贴板文本路径
    /// wsl2_path_conversion 启用时返回容器侧路径，否则返回 Windows 原生路径
    pub fn clipboard_text_path(&self, filename: &str, win_path: &std::path::Path) -> String {
        if self.wsl2_path_conversion {
            self.container_path_for(filename)
        } else {
            win_path.to_string_lossy().to_string()
        }
    }

    /// 解析 save_dir 为绝对路径
    /// 相对路径基于 EXE 所在目录
    pub fn resolved_save_dir(&self, exe_dir: &Path) -> PathBuf {
        let save = Path::new(&self.save_dir);
        if save.is_absolute() || desktop_config::is_windows_absolute(&self.save_dir) {
            save.to_path_buf()
        } else {
            exe_dir.join(&self.save_dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_config::is_windows_absolute;
    use std::fs;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.hotkey, "");
        assert_eq!(config.output_path, "/workspace/.clip");
        assert_eq!(config.save_dir, ".clip");
        assert_eq!(config.max_history_hours, 1);
        assert_eq!(config.max_copy_size_mb, 10);
        assert_eq!(config.preview_hotkey, "Ctrl+Alt+P");
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
        assert_eq!(config.output_path, "/workspace/.clip");
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
        assert!(config.validate().is_ok());
        assert!(!config.is_hotkey_mode());
    }

    #[test]
    fn test_is_hotkey_mode() {
        let config = AppConfig::default();
        assert!(!config.is_hotkey_mode());

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
    fn test_resolved_save_dir_relative() {
        let config = AppConfig {
            save_dir: ".clip".to_string(),
            ..Default::default()
        };
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
    fn test_container_path_for() {
        let config = AppConfig {
            output_path: "/workspace/.clip".to_string(),
            ..Default::default()
        };
        assert_eq!(
            config.container_path_for("clip_20260416_103000123.png"),
            "/workspace/.clip/clip_20260416_103000123.png"
        );
        assert_eq!(
            config.container_path_for("clip_20260416_103500456.pdf"),
            "/workspace/.clip/clip_20260416_103500456.pdf"
        );
    }

    #[test]
    fn test_container_path_for_trailing_slash() {
        let config = AppConfig {
            output_path: "/workspace/.clip/".to_string(),
            ..Default::default()
        };
        assert_eq!(
            config.container_path_for("clip_20260416_103000123.png"),
            "/workspace/.clip/clip_20260416_103000123.png"
        );
    }

    #[test]
    fn test_load_old_config_migrates_poll_interval() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let old_json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip",
            "save_dir": ".clip",
            "poll_interval_ms": 800,
            "max_history_hours": 1
        }"#;
        fs::write(&config_path, old_json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.output_path, "/workspace/.clip");

        // 验证配置文件已回写，不再包含 poll_interval_ms
        let rewritten = fs::read_to_string(&config_path).unwrap();
        assert!(!rewritten.contains("poll_interval_ms"));
    }

    #[test]
    fn test_load_old_config_truncates_output_path() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let old_json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip/latest.png",
            "save_dir": ".clip",
            "max_history_hours": 1
        }"#;
        fs::write(&config_path, old_json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.output_path, "/workspace/.clip");

        // 验证配置文件已回写截断后的值
        let rewritten = fs::read_to_string(&config_path).unwrap();
        assert!(!rewritten.contains("latest.png"));
    }

    #[test]
    fn test_load_old_config_migrates_max_history_days() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let old_json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip",
            "save_dir": ".clip",
            "max_history_days": 7
        }"#;
        fs::write(&config_path, old_json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.max_history_hours, 168);

        // 验证配置文件已回写迁移后的值
        let rewritten = fs::read_to_string(&config_path).unwrap();
        assert!(rewritten.contains("max_history_hours"));
        assert!(!rewritten.contains("max_history_days"));
    }

    #[test]
    fn test_load_config_with_max_history_hours() {
        let dir = temp_dir();
        let config_path = dir.path().join("config.json");
        let json = r#"{
            "hotkey": "",
            "output_path": "/workspace/.clip",
            "save_dir": ".clip",
            "max_history_hours": 4
        }"#;
        fs::write(&config_path, json).unwrap();

        let config = AppConfig::load(&config_path).unwrap();
        assert_eq!(config.max_history_hours, 4);
    }

    #[test]
    fn test_resolved_save_dir_unc_path() {
        let config = AppConfig {
            save_dir: "\\\\wsl$\\debian\\home\\user\\.clip".to_string(),
            ..Default::default()
        };
        let exe_dir = Path::new("/some/path/clipImg");
        let resolved = config.resolved_save_dir(exe_dir);
        assert_eq!(
            resolved,
            PathBuf::from("\\\\wsl$\\debian\\home\\user\\.clip")
        );
    }

    #[test]
    fn test_is_windows_absolute_unc() {
        assert!(is_windows_absolute("\\\\wsl$\\debian\\home\\user\\.clip"));
        assert!(is_windows_absolute(
            "\\\\wsl.localhost\\debian\\home\\user\\.clip"
        ));
        assert!(is_windows_absolute("C:\\Users\\test"));
        assert!(!is_windows_absolute(".clip"));
        assert!(!is_windows_absolute("/home/user/.clip"));
    }
}
