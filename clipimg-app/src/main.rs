// Windows 子系统：默认 WINDOWS（无控制台），编译时启用 console feature 保留控制台
// 注意：必须在 non-windows 平台不设置，否则编译失败
#![cfg_attr(all(target_os = "windows", not(feature = "console")), windows_subsystem = "windows")]

fn main() {
    run_app();
}

mod clipboard;
mod config;
#[cfg(target_os = "windows")]
mod input;
mod logger;

#[cfg(target_os = "windows")]
fn fatal_error(msg: &str) -> ! {
    eprintln!("clipImg 致命错误: {}", msg);
    // 弹窗显示错误，防止闪退看不到
    unsafe {
        let wide: Vec<u16> = format!("clipImg 启动失败:\n\n{}", msg)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let title: Vec<u16> = "clipImg 错误\0".encode_utf16().collect();
        windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            title.as_ptr(),
            0x10, // MB_ICONERROR
        );
    }
    std::process::exit(1);
}

#[cfg(target_os = "windows")]
fn is_console_mode() -> bool {
    cfg!(feature = "console") || std::env::args().any(|a| a == "--console")
}

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

    // --console 模式：附加控制台用于看日志输出
    let console_mode = is_console_mode();

    // 先确定路径
    let exe_dir = get_exe_dir();
    let config_path = exe_dir.join("config.json");

    // 加载配置
    let config = match AppConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => fatal_error(&format!("加载配置失败: {}", e)),
    };

    // 先确保保存目录存在（日志文件要写到这里）
    let save_dir = config.resolved_save_dir(&exe_dir);
    if let Err(e) = std::fs::create_dir_all(&save_dir) {
        fatal_error(&format!("创建保存目录失败: {}\n目录: {}", e, save_dir.display()));
    }

    // 初始化日志和 panic handler
    let log_path = save_dir.join(".clipimg.log");
    logger::init(&log_path, console_mode);
    logger::set_panic_hook(&log_path);

    let mode_name = if config.is_hotkey_mode() {
        "热键模式 (方案 A)"
    } else {
        "多格式剪贴板模式 (方案 C)"
    };

    let version = env!("CARGO_PKG_VERSION");
    log::info!("========== clipImg v{} 启动 ==========", version);
    log::info!("运行模式: {}", mode_name);
    if console_mode {
        log::info!("控制台模式: 已启用 (--console)");
    }
    log::info!("配置文件: {}", config_path.display());
    log::info!("保存目录: {}", save_dir.display());
    log::info!("日志文件: {}", log_path.display());
    if config.is_hotkey_mode() {
        log::info!("热键: {}", config.hotkey);
    }
    log::info!("输出路径: {}", config.output_path);

    let watcher = ClipboardWatcher::new(config.clone(), &exe_dir);
    if let Err(e) = watcher.ensure_dir() {
        fatal_error(&format!("创建保存目录失败: {}", e));
    }

    let deleted = watcher.clean_old_files();
    if deleted > 0 {
        log::info!("启动清理: 已删除 {} 个过期图片", deleted);
    }

    let event_loop = EventLoop::new();

    // 仅在热键模式下注册全局热键
    let hotkey_manager = if config.is_hotkey_mode() {
        let mgr = match GlobalHotKeyManager::new() {
            Ok(m) => {
                log::info!("热键管理器创建成功");
                m
            }
            Err(e) => fatal_error(&format!("创建热键管理器失败: {:?}", e)),
        };

        let hotkey: HotKey = match HotKey::try_from(config.hotkey.clone()) {
            Ok(h) => {
                log::info!("热键 '{}' 解析成功, id={:?}", config.hotkey, h.id());
                h
            }
            Err(e) => fatal_error(&format!(
                "解析热键 '{}' 失败: {:?}\n支持格式: Alt+Insert, Ctrl+Shift+V, Super+V",
                config.hotkey, e
            )),
        };

        match mgr.register(hotkey) {
            Ok(()) => log::info!("热键已注册成功: {}", config.hotkey),
            Err(e) => fatal_error(&format!(
                "注册热键失败: {:?}（可能被其他程序占用）",
                e
            )),
        }

        Some(mgr)
    } else {
        log::info!("热键未配置，使用多格式剪贴板模式");
        None
    };

    // 开机自启状态
    let autostart_enabled = is_autostart_enabled();
    log::info!("开机自启: {}", if autostart_enabled { "已启用" } else { "未启用" });

    // 系统托盘菜单
    let mode_label = if config.is_hotkey_mode() {
        format!("clipImg v{} [{}]", version, config.hotkey)
    } else {
        format!("clipImg v{} [剪贴板模式]", version)
    };

    let tray_menu = Menu::new();
    let status_item = MenuItem::with_id("status", &mode_label, false, None);
    let open_log = MenuItem::with_id("open_log", "打开日志文件", true, None);
    let open_config = MenuItem::with_id("open_config", "打开配置文件", true, None);
    let open_dir = MenuItem::with_id("open_dir", "打开图片目录", true, None);
    let autostart_item = MenuItem::with_id("autostart", "开机自启", autostart_enabled, None);
    let quit_item = MenuItem::with_id("quit", "退出", true, None);

    tray_menu
        .append_items(&[
            &status_item,
            &PredefinedMenuItem::separator(),
            &open_log,
            &open_config,
            &open_dir,
            &PredefinedMenuItem::separator(),
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .unwrap();

    let _tray = TrayIconBuilder::new()
        .with_tooltip(&format!("clipImg v{}", version))
        .with_menu(Box::new(tray_menu))
        .build()
        .expect("无法创建托盘图标");

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => {
            log::info!("剪贴板访问初始化成功");
            c
        }
        Err(e) => fatal_error(&format!("无法访问剪贴板: {:?}", e)),
    };

    let poll_interval = Duration::from_millis(config.poll_interval_ms);
    let mut last_poll = Instant::now();

    let config_clone = config.clone();
    let exe_dir_clone = exe_dir.clone();

    log::info!("事件循环启动，开始监听剪贴板和热键");
    if config.is_hotkey_mode() {
        log::info!("按 {} 输入图片路径", config.hotkey);
    } else {
        log::info!("截图后自动设置多格式剪贴板，在终端 Ctrl+V 粘贴即得到路径");
    }

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        // 热键事件（仅模式 A）
        if let Some(ref _mgr) = hotkey_manager {
            if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                log::debug!("收到热键事件: state={:?}", event.state);
                if event.state == HotKeyState::Pressed {
                    log::info!("热键触发: {}", config.hotkey);
                    let latest = config.latest_png_path(&exe_dir);
                    if latest.exists() {
                        log::info!("发送路径: {}", config.output_path);
                        match input::send_text_with_ime(&config.output_path) {
                            Ok(()) => log::info!("路径已发送"),
                            Err(e) => log::error!("发送文本失败: {}", e),
                        }
                    } else {
                        log::warn!("latest.png 不存在，请先在 Windows 中复制图片");
                    }
                }
            }
        }

        // 托盘菜单事件
        if let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            log::debug!("菜单事件: id={}", event.id().as_ref());
            match event.id().as_ref() {
                "open_log" => {
                    let _ = std::process::Command::new("notepad").arg(&log_path).spawn();
                }
                "open_config" => {
                    let _ = std::process::Command::new("explorer").arg(&config_path).spawn();
                }
                "open_dir" => {
                    let dir = config_clone.resolved_save_dir(&exe_dir_clone);
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                "autostart" => {
                    toggle_autostart();
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
            let has_new = watcher.poll(&mut clipboard);
            if has_new && !config.is_hotkey_mode() {
                // 模式 C：新图片保存后，自动设置多格式剪贴板
                let latest = config.latest_png_path(&exe_dir);
                if latest.exists() {
                    log::info!("设置多格式剪贴板...");
                    match input::set_multi_format_clipboard(&config.output_path, &latest) {
                        Ok(()) => log::info!("多格式剪贴板设置成功"),
                        Err(e) => log::error!("多格式剪贴板设置失败: {}", e),
                    }
                }
            }
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

// ============================================================
// 开机自启：读写注册表 HKCU\Software\Microsoft\Windows\CurrentVersion\Run
// ============================================================

#[cfg(target_os = "windows")]
fn is_autostart_enabled() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let key = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let value_name: Vec<u16> = OsStr::new("clipImg").encode_wide().chain(std::iter::once(0)).collect();

    let mut buf = [0u16; 512];
    let mut buf_len = (buf.len() * 2) as u32;

    let result = unsafe {
        windows_sys::Win32::System::Registry::RegGetValueW(
            0x80000001 as *mut std::ffi::c_void, // HKCU
            OsStr::new(key).encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>().as_ptr(),
            value_name.as_ptr(),
            0x00020002, // RRF_RT_REG_SZ
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut buf_len,
        )
    };

    if result != 0 {
        return false;
    }

    let stored: String = buf[..(buf_len as usize / 2 - 1)].iter().map(|&c| c as u8 as char).collect();
    let exe_str = exe_path.to_str().unwrap_or("");
    stored.contains(exe_str)
}

#[cfg(target_os = "windows")]
fn toggle_autostart() {
    let currently_enabled = is_autostart_enabled();

    if currently_enabled {
        // 禁用：删除注册表值
        remove_autostart();
    } else {
        // 启用：写入注册表值
        set_autostart();
    }
}

#[cfg(target_os = "windows")]
fn set_autostart() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => { log::error!("获取 EXE 路径失败: {}", e); return; }
    };

    let value: String = format!("\"{}\"", exe_path.display());
    let value_wide: Vec<u16> = OsStr::new(&value).encode_wide().chain(std::iter::once(0)).collect();
    let key_wide: Vec<u16> = OsStr::new(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .encode_wide().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = OsStr::new("clipImg").encode_wide().chain(std::iter::once(0)).collect();

    let result = unsafe {
        windows_sys::Win32::System::Registry::RegSetKeyValueW(
            0x80000001 as *mut std::ffi::c_void, // HKCU
            key_wide.as_ptr(),
            name_wide.as_ptr(),
            1, // REG_SZ
            value_wide.as_ptr() as *const _,
            (value_wide.len() * 2) as u32,
        )
    };

    if result == 0 {
        log::info!("开机自启已启用");
    } else {
        log::error!("设置开机自启失败: 错误码 {}", result);
    }
}

#[cfg(target_os = "windows")]
fn remove_autostart() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let key_wide: Vec<u16> = OsStr::new(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .encode_wide().chain(std::iter::once(0)).collect();
    let name_wide: Vec<u16> = OsStr::new("clipImg").encode_wide().chain(std::iter::once(0)).collect();

    let result = unsafe {
        windows_sys::Win32::System::Registry::RegDeleteKeyValueW(
            0x80000001 as *mut std::ffi::c_void, // HKCU
            key_wide.as_ptr(),
            name_wide.as_ptr(),
        )
    };

    if result == 0 {
        log::info!("开机自启已禁用");
    } else {
        log::error!("移除开机自启失败: 错误码 {}", result);
    }
}
