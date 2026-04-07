mod clipboard;
mod config;
#[cfg(target_os = "windows")]
mod input;
mod logger;

#[cfg(target_os = "windows")]
fn run_app() {
    use clipboard::ClipboardWatcher;
    use config::AppConfig;
    use global_hotkey::hotkey::HotKey;
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use muda::{Menu, MenuItem, PredefinedMenuItem};
    use std::time::{Duration, Instant};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tray_icon::TrayIconBuilder;

    // 先确定路径
    let exe_dir = get_exe_dir();
    let config_path = exe_dir.join("config.json");

    // 加载配置（需要先加载才能知道 save_dir）
    let config = match AppConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("加载配置失败: {}", e);
            std::process::exit(1);
        }
    };

    // 初始化日志和 panic handler
    let save_dir = config.resolved_save_dir(&exe_dir);
    let log_path = save_dir.join(".clipimg.log");
    logger::init(&log_path);
    logger::set_panic_hook(&log_path);

    log::info!("========== clipImg 启动 ==========");
    log::info!("配置文件: {}", config_path.display());
    log::info!("保存目录: {}", save_dir.display());
    log::info!("日志文件: {}", log_path.display());
    log::info!("热键: {}", config.hotkey);
    log::info!("输出路径: {}", config.output_path);

    let watcher = ClipboardWatcher::new(config.clone(), &exe_dir);
    if let Err(e) = watcher.ensure_dir() {
        log::error!("创建保存目录失败: {}", e);
        std::process::exit(1);
    }

    let deleted = watcher.clean_old_files();
    if deleted > 0 {
        log::info!("启动清理: 已删除 {} 个过期图片", deleted);
    }

    let event_loop = EventLoop::new();

    // 注册全局热键
    let hotkey_manager = match GlobalHotKeyManager::new() {
        Ok(m) => {
            log::info!("热键管理器创建成功");
            m
        }
        Err(e) => {
            log::error!("创建热键管理器失败: {:?}", e);
            std::process::exit(1);
        }
    };

    let hotkey: HotKey = match HotKey::try_from(config.hotkey.clone()) {
        Ok(h) => {
            log::info!("热键 '{}' 解析成功, id={:?}", config.hotkey, h.id());
            h
        }
        Err(e) => {
            log::error!("解析热键 '{}' 失败: {:?}", config.hotkey, e);
            log::error!("支持的格式示例: Alt+Insert, Ctrl+Shift+V, Super+V");
            std::process::exit(1);
        }
    };

    match hotkey_manager.register(hotkey) {
        Ok(()) => log::info!("热键已注册成功: {}", config.hotkey),
        Err(e) => {
            log::error!("注册热键失败: {:?}（可能被其他程序占用）", e);
            std::process::exit(1);
        }
    }

    // 系统托盘菜单
    let tray_menu = Menu::new();
    let status_item = MenuItem::with_id("status", "clipImg 运行中", false, None);
    let open_config = MenuItem::with_id("open_config", "打开配置文件", true, None);
    let open_dir = MenuItem::with_id("open_dir", "打开图片目录", true, None);
    let quit_item = MenuItem::with_id("quit", "退出", true, None);

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
        .build()
        .expect("无法创建托盘图标");

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => {
            log::info!("剪贴板访问初始化成功");
            c
        }
        Err(e) => {
            log::error!("无法访问剪贴板: {:?}", e);
            std::process::exit(1);
        }
    };

    let poll_interval = Duration::from_millis(config.poll_interval_ms);
    let mut last_poll = Instant::now();

    let config_clone = config.clone();
    let exe_dir_clone = exe_dir.clone();

    log::info!("事件循环启动，开始监听剪贴板和热键");
    log::info!("按 {} 输入图片路径", config.hotkey);

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        // 热键事件
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            log::debug!("收到热键事件: state={:?}", event.state);
            if event.state == HotKeyState::Pressed {
                log::info!("热键触发: {}", config.hotkey);
                let latest = config.latest_png_path(&exe_dir);
                if latest.exists() {
                    log::info!("发送路径: {}", config.output_path);
                    match input::send_text(&mut clipboard, &config.output_path) {
                        Ok(()) => log::info!("路径已发送"),
                        Err(e) => log::error!("发送文本失败: {}", e),
                    }
                } else {
                    log::warn!("latest.png 不存在，请先在 Windows 中复制图片");
                }
            }
        }

        // 托盘菜单事件
        if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            log::debug!("菜单事件: id={}", event.id().as_ref());
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
