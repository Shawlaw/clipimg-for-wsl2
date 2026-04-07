mod clipboard;
mod config;
#[cfg(target_os = "windows")]
mod input;

#[cfg(target_os = "windows")]
fn run_app() {
    use clipboard::ClipboardWatcher;
    use config::AppConfig;
    use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager};
    use muda::{Menu, MenuItem, PredefinedMenuItem};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};
    use tao::event_loop::EventLoopBuilder;
    use tray_icon::{TrayIconBuilder, TrayIconEvent};

    env_logger::init();

    let exe_dir = get_exe_dir();
    let config_path = exe_dir.join("config.json");

    let config = match AppConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("加载配置失败: {}", e);
            std::process::exit(1);
        }
    };

    log::info!("配置加载完成: hotkey={}, output_path={}", config.hotkey, config.output_path);

    let watcher = ClipboardWatcher::new(config.clone(), &exe_dir);
    if let Err(e) = watcher.ensure_dir() {
        log::error!("创建保存目录失败: {}", e);
        std::process::exit(1);
    }

    let deleted = watcher.clean_old_files();
    if deleted > 0 {
        log::info!("启动清理: 已删除 {} 个过期图片", deleted);
    }

    let event_loop = EventLoopBuilder::new().build();

    // 注册全局热键
    let hotkey_manager = GlobalHotKeyManager::new().expect("无法创建热键管理器");
    let hotkey: HotKey = config.hotkey.as_str().try_into().unwrap_or_else(|e| {
        log::error!("解析热键 '{}' 失败: {:?}", config.hotkey, e);
        std::process::exit(1);
    });
    hotkey_manager.register(hotkey).expect("无法注册热键");
    log::info!("已注册全局热键: {}", config.hotkey);

    // 系统托盘菜单
    let tray_menu = Menu::new();
    let status_item = MenuItem::with_id("status", "clipImg 运行中", false, true);
    let open_config = MenuItem::with_id("open_config", "打开配置文件", true, false);
    let open_dir = MenuItem::with_id("open_dir", "打开图片目录", true, false);
    let quit_item = MenuItem::with_id("quit", "退出", true, false);

    tray_menu
        .append_items(&[
            &status_item,
            &PredefinedMenuItem::separator(),
            &open_config,
            &open_dir,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .unwrap();

    let _tray = TrayIconBuilder::new()
        .with_tooltip("clipImg - 剪贴板图片工具")
        .with_menu(Box::new(tray_menu))
        .build(&event_loop)
        .expect("无法创建托盘图标");

    let mut clipboard = arboard::Clipboard::new().expect("无法访问剪贴板");
    let poll_interval = Duration::from_millis(config.poll_interval_ms);
    let mut last_poll = Instant::now();

    let config_clone = config.clone();
    let exe_dir_clone = exe_dir.clone();

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        // 热键事件
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::hotkey::HotKeyState::Pressed {
                let latest = config.latest_png_path(&exe_dir);
                if latest.exists() {
                    if let Err(e) = input::send_text(&config.output_path) {
                        log::warn!("发送文本失败: {}", e);
                    }
                } else {
                    log::warn!("latest.png 不存在，请先复制图片");
                }
            }
        }

        // 托盘菜单事件
        if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            match event.id().as_ref() {
                "open_config" => {
                    let _ = std::process::Command::new("explorer").arg(&config_path).spawn();
                }
                "open_dir" => {
                    let dir = config_clone.resolved_save_dir(&exe_dir_clone);
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                "quit" => {
                    log::info!("用户选择退出");
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            }
        }

        // 剪贴板轮询
        if last_poll.elapsed() >= poll_interval {
            watcher.poll(&mut clipboard);
            last_poll = Instant::now();
        }
    });
}

#[cfg(not(target_os = "windows"))]
fn run_app() {
    eprintln!("clipImg 仅支持 Windows。请使用 cargo xwin build --target x86_64-pc-windows-msvc --release 构建。");
    std::process::exit(1);
}

fn get_exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn main() {
    run_app();
}
