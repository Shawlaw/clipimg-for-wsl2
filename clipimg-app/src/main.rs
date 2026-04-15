// Windows 子系统：默认 WINDOWS（无控制台），编译时启用 console feature 保留控制台
// 注意：必须在 non-windows 平台不设置，否则编译失败
#![cfg_attr(all(target_os = "windows", not(feature = "console")), windows_subsystem = "windows")]

fn main() {
    run_app();
}

mod clipboard;
#[cfg(target_os = "windows")]
mod clipboard_listener;
mod config;
#[cfg(target_os = "windows")]
mod input;
mod logger;

#[cfg(target_os = "windows")]
mod first_run;

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
    use clipboard_listener::ClipboardListener;
    use config::AppConfig;
    use global_hotkey::hotkey::HotKey;
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
    use std::cell::RefCell;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::rc::Rc;
    use tray_icon::TrayIconBuilder;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, PostQuitMessage, RegisterWindowMessageW, TranslateMessage,
        MSG,
    };

    // --console 模式：附加控制台用于看日志输出
    let console_mode = is_console_mode();

    // 多实例防护：创建命名互斥体，已存在则退出
    {
        let mutex_name: Vec<u16> = OsStr::new("Global\\clipimg")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let handle = windows_sys::Win32::System::Threading::CreateMutexW(
                std::ptr::null_mut(),
                0,
                mutex_name.as_ptr(),
            );
            let err = windows_sys::Win32::Foundation::GetLastError();
            if err == 183 {
                // ERROR_ALREADY_EXISTS
                fatal_error("clipImg 已在运行中，不能重复启动。");
            }
            // handle 不需要关闭，进程退出时自动释放
            let _ = handle;
        }
    }

    // 先确定路径
    let exe_dir = get_exe_dir();
    let config_path = exe_dir.join("config.json");

    // 加载配置（首次运行弹出双路径确认对话框）
    let config = if !config_path.exists() {
        let default_cfg = AppConfig::default();
        let resolved = default_cfg.resolved_save_dir(&exe_dir);
        let resolved_str = resolved.to_str().unwrap_or("").to_string();

        match first_run::confirm_paths(&resolved_str, &default_cfg.output_path) {
            Some((user_win_dir, user_container_dir)) => {
                let mut cfg = default_cfg;
                cfg.save_dir = user_win_dir;
                cfg.output_path = user_container_dir;
                if let Err(e) = cfg.save(&config_path) {
                    fatal_error(&format!("保存配置失败: {}", e));
                }
                cfg
            }
            None => {
                // 用户取消，退出
                std::process::exit(0);
            }
        }
    } else {
        match AppConfig::load(&config_path) {
            Ok(c) => c,
            Err(e) => fatal_error(&format!("加载配置失败: {}", e)),
        }
    };

    // 先确保保存目录存在（日志文件要写到这里）
    let save_dir = config.resolved_save_dir(&exe_dir);
    if let Err(e) = std::fs::create_dir_all(&save_dir) {
        fatal_error(&format!("创建保存目录失败: {}\n目录: {}", e, save_dir.display()));
    }

    // 初始化日志和 panic handler
    let log_path = save_dir.join(".clipimg.log");
    logger::init(&log_path, console_mode, config.max_log_size_mb);
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

    let config = Rc::new(RefCell::new(config));
    let watcher = ClipboardWatcher::new(config.borrow().clone(), &exe_dir);
    if let Err(e) = watcher.ensure_dir() {
        fatal_error(&format!("创建保存目录失败: {}", e));
    }
    let watcher = Rc::new(RefCell::new(watcher));

    let deleted = watcher.borrow().clean_old_files();
    if deleted > 0 {
        log::info!("启动清理: 已删除 {} 个过期文件", deleted);
    }

    // 迁移旧版 latest.png → latest_file.png
    watcher.borrow().migrate_old_latest();

    // 仅在热键模式下注册全局热键
    let hotkey_manager: Rc<RefCell<Option<GlobalHotKeyManager>>> = if config.borrow().is_hotkey_mode() {
        let mgr = match GlobalHotKeyManager::new() {
            Ok(m) => {
                log::info!("热键管理器创建成功");
                m
            }
            Err(e) => fatal_error(&format!("创建热键管理器失败: {:?}", e)),
        };

        let hotkey: HotKey = match HotKey::try_from(config.borrow().hotkey.clone()) {
            Ok(h) => {
                log::info!("热键 '{}' 解析成功, id={:?}", config.borrow().hotkey, h.id());
                h
            }
            Err(e) => fatal_error(&format!(
                "解析热键 '{}' 失败: {:?}\n支持格式: Alt+Insert, Ctrl+Shift+V, Super+V",
                config.borrow().hotkey, e
            )),
        };

        match mgr.register(hotkey) {
            Ok(()) => log::info!("热键已注册成功: {}", config.borrow().hotkey),
            Err(e) => fatal_error(&format!(
                "注册热键失败: {:?}（可能被其他程序占用）",
                e
            )),
        }

        Rc::new(RefCell::new(Some(mgr)))
    } else {
        log::info!("热键未配置，使用多格式剪贴板模式");
        Rc::new(RefCell::new(None))
    };

    // 注册预览热键（独立于输入热键）
    let preview_hotkey: Rc<RefCell<Option<HotKey>>> = Rc::new(RefCell::new(None));
    {
        let phk = config.borrow().preview_hotkey.trim().to_string();
        if !phk.is_empty() {
            match HotKey::try_from(phk.clone()) {
                Ok(key) => {
                    // 确保热键管理器存在
                    if hotkey_manager.borrow().is_none() {
                        match GlobalHotKeyManager::new() {
                            Ok(mgr) => *hotkey_manager.borrow_mut() = Some(mgr),
                            Err(e) => {
                                log::error!("创建热键管理器失败: {:?}", e);
                            }
                        }
                    }
                    if let Some(ref mgr) = *hotkey_manager.borrow() {
                        match mgr.register(key) {
                            Ok(()) => {
                                log::info!("预览热键已注册: {}", phk);
                                *preview_hotkey.borrow_mut() = Some(key);
                            }
                            Err(e) => log::error!("注册预览热键失败: {:?}", e),
                        }
                    }
                }
                Err(e) => log::error!("解析预览热键 '{}' 失败: {:?}", phk, e),
            }
        } else {
            log::info!("预览热键未配置");
        }
    }

    // 开机自启状态
    let autostart_enabled = is_autostart_enabled();
    log::info!("开机自启: {}", if autostart_enabled { "已启用" } else { "未启用" });

    // 系统托盘菜单
    let mode_label = if config.borrow().is_hotkey_mode() {
        format!("clipImg v{} [{}]", version, config.borrow().hotkey)
    } else {
        format!("clipImg v{} [剪贴板模式]", version)
    };

    let tray_menu = Menu::new();
    let status_item = MenuItem::with_id("status", &mode_label, false, None);
    let preview_label = {
        let phk = config.borrow().preview_hotkey.trim().to_string();
        if phk.is_empty() {
            "预览功能已关闭".to_string()
        } else {
            format!("预览快捷键: {}", phk)
        }
    };
    let preview_item = MenuItem::with_id("preview_hotkey", &preview_label, false, None);
    let open_log = MenuItem::with_id("open_log", "打开日志文件", true, None);
    let open_config = MenuItem::with_id("open_config", "打开配置文件", true, None);
    let reload_config = MenuItem::with_id("reload_config", "重新加载配置", true, None);
    let open_dir = MenuItem::with_id("open_dir", "打开图片目录", true, None);
    let homepage = MenuItem::with_id("homepage", "项目主页", true, None);
    let autostart_item = CheckMenuItem::with_id("autostart", "开机自启", true, autostart_enabled, None);
    let quit_item = MenuItem::with_id("quit", "退出", true, None);

    tray_menu
        .append_items(&[
            &status_item,
            &preview_item,
            &PredefinedMenuItem::separator(),
            &open_log,
            &open_config,
            &reload_config,
            &open_dir,
            &homepage,
            &PredefinedMenuItem::separator(),
            &autostart_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .unwrap();

    // 加载托盘图标
    let tray_icon = {
        let icon_data = include_bytes!("../icons/icon_32.png");
        let img = image::load_from_memory(icon_data).expect("无法加载图标");
        let rgba = img.to_rgba8();
        tray_icon::Icon::from_rgba(rgba.to_vec(), rgba.width(), rgba.height())
            .expect("无法创建图标对象")
    };

    let _tray = TrayIconBuilder::new()
        .with_tooltip(&format!("clipImg v{}", version))
        .with_icon(tray_icon)
        .with_menu(Box::new(tray_menu))
        .build()
        .expect("无法创建托盘图标");

    // 启动提示弹窗
    {
        let mode_label = if config.borrow().is_hotkey_mode() {
            format!("热键模式 {}", config.borrow().hotkey)
        } else {
            "剪贴板模式".to_string()
        };
        let tip = format!("clipImg v{} 已启动 [{}]", version, mode_label);
        let _ = _tray.set_tooltip(Some(&tip));
        if config.borrow().show_startup_notification {
            let msg = format!(
                "{}\n\n如不需要此提示，请在配置文件中将 show_startup_notification 设为 false。",
                tip
            );
            show_notification(&format!("clipImg v{}", version), &msg, &save_dir);
        }
    }

    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => {
            log::info!("剪贴板访问初始化成功");
            c
        }
        Err(e) => fatal_error(&format!("无法访问剪贴板: {:?}", e)),
    };

    // 注册自定义消息，用于剪贴板监听线程通知主线程
    let wm_clip_changed: u32 = unsafe {
        let name: Vec<u16> = OsStr::new("clipImgClipboardChanged")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        RegisterWindowMessageW(name.as_ptr())
    };
    if wm_clip_changed == 0 {
        fatal_error("RegisterWindowMessageW 失败");
    }

    let main_thread_id = unsafe { GetCurrentThreadId() };

    // 注册配置文件变化通知消息
    let wm_config_changed: u32 = unsafe {
        let name: Vec<u16> = OsStr::new("clipImgConfigChanged")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        RegisterWindowMessageW(name.as_ptr())
    };

    // 启动剪贴板监听线程（替代轮询）
    let _clip_listener = match ClipboardListener::start(main_thread_id, wm_clip_changed) {
        Ok(listener) => {
            log::info!("剪贴板监听已启动");
            listener
        }
        Err(e) => fatal_error(&format!("启动剪贴板监听失败: {}", e)),
    };

    // 启动配置文件监控线程
    let _config_watcher = start_config_watcher(
        main_thread_id,
        wm_config_changed,
        config_path.clone(),
    );

    log::info!("消息循环启动，等待剪贴板变化通知");
    if config.borrow().is_hotkey_mode() {
        log::info!("按 {} 输入图片路径", config.borrow().hotkey);
    } else {
        log::info!("截图后自动设置多格式剪贴板，在终端 Ctrl+V 粘贴即得到路径");
    }

    // ============================================================
    // Win32 消息循环（替代 tao 事件循环，消除 DeviceEvent 噪音）
    // ============================================================
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // 防止剪贴板反馈环：我们设置剪贴板后，监听器会再次触发，
    // 用此标志跳过自身触发的通知
    let mut clipboard_self_triggered = false;
    loop {
        // GetMessageW 阻塞等待，无消息时线程休眠 → 零 CPU
        let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if ret == 0 {
            // WM_QUIT
            break;
        }

        // 配置文件变化通知
        if msg.message == wm_config_changed {
            do_reload_config(
                &config,
                &config_path,
                &hotkey_manager,
                &status_item,
                &preview_item,
                &exe_dir,
                version,
            );
        }

        // 剪贴板变化通知
        if msg.message == wm_clip_changed {
            // 跳过自身触发的剪贴板变化（防止反馈环）
            if clipboard_self_triggered {
                clipboard_self_triggered = false;
            } else {
                // 先检查 CF_HDROP（文件复制）
                let hdrop_handled = if let Some(files) = clipboard::read_clipboard_files() {
                    if let Some(first_file) = files.into_iter().next() {
                        let is_png = clipboard::ClipboardWatcher::is_png_file(&first_file);
                        if watcher.borrow().copy_file(&first_file).is_some() {
                            if !config.borrow().is_hotkey_mode() {
                                let container_path = watcher.borrow().latest_container_path.borrow().clone();
                                if is_png {
                                    let win_path = config.borrow().latest_file_path(&exe_dir);
                                    log::info!("设置多格式剪贴板 (PNG)...");
                                    match input::set_multi_format_clipboard(&container_path, &win_path) {
                                        Ok(()) => { clipboard_self_triggered = true; log::info!("多格式剪贴板设置成功"); }
                                        Err(e) => log::error!("多格式剪贴板设置失败: {}", e),
                                    }
                                } else {
                                    log::info!("设置文本+文件剪贴板: {}", container_path);
                                    match input::set_text_and_file_clipboard(&container_path, &first_file) {
                                        Ok(()) => { clipboard_self_triggered = true; log::info!("文本+文件剪贴板设置成功"); }
                                        Err(e) => log::error!("文本+文件剪贴板设置失败: {}", e),
                                    }
                                }
                            }
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                // CF_HDROP 未处理时走 DIB 流程
                if !hdrop_handled {
                    let has_new = watcher.borrow_mut().poll(&mut clipboard);
                    if has_new && !config.borrow().is_hotkey_mode() {
                        let win_path = config.borrow().latest_file_path(&exe_dir);
                        if win_path.exists() {
                            let container_path = watcher.borrow().latest_container_path.borrow().clone();
                            log::info!("设置多格式剪贴板...");
                            match input::set_multi_format_clipboard(&container_path, &win_path) {
                                Ok(()) => { clipboard_self_triggered = true; log::info!("多格式剪贴板设置成功"); }
                                Err(e) => log::error!("多格式剪贴板设置失败: {}", e),
                            }
                        }
                    }
                }
            }
        }

        // 热键事件
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            log::debug!("收到热键事件: state={:?}", event.state);
            if event.state == HotKeyState::Pressed {
                let is_preview = preview_hotkey.borrow().as_ref().map_or(false, |k| k.id() == event.id());
                let is_input = config.borrow().is_hotkey_mode();

                if is_preview {
                    // 预览热键：用系统默认程序打开 latest_file
                    log::info!("预览热键触发");
                    let save_dir = config.borrow().resolved_save_dir(&exe_dir);
                    if let Some(latest) = find_latest_file(&save_dir) {
                        if is_executable_file(&latest, &config.borrow().blocked_preview_ext) {
                            log::warn!("预览已拦截：可执行文件不允许通过预览打开 ({})", latest.display());
                        } else {
                            let _ = std::process::Command::new("cmd")
                                .args(["/c", "start", "", &latest.to_string_lossy()])
                                .spawn();
                            log::info!("已打开: {}", latest.display());
                        }
                    } else {
                        log::warn!("没有最新文件，请先复制文件或截图");
                    }
                } else if is_input {
                    // 输入热键：发送路径
                    log::info!("热键触发: {}", config.borrow().hotkey);
                    let container_path = watcher.borrow().latest_container_path.borrow().clone();
                    let save_dir = config.borrow().resolved_save_dir(&exe_dir);
                    if find_latest_file(&save_dir).is_some() {
                        log::info!("发送路径: {}", container_path);
                        match input::send_text_with_ime(&container_path) {
                            Ok(()) => log::info!("路径已发送"),
                            Err(e) => log::error!("发送文本失败: {}", e),
                        }
                    } else {
                        log::warn!("没有最新文件，请先复制文件或截图");
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
                "reload_config" => {
                    do_reload_config(
                        &config,
                        &config_path,
                        &hotkey_manager,
                        &status_item,
                        &preview_item,
                        &exe_dir,
                        version,
                    );
                }
                "open_dir" => {
                    let dir = config.borrow().resolved_save_dir(&exe_dir);
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
                "homepage" => {
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "https://github.com/Shawlaw/clipimg-for-wsl2"])
                        .spawn();
                }
                "autostart" => {
                    toggle_autostart();
                    let now_enabled = is_autostart_enabled();
                    autostart_item.set_checked(now_enabled);
                    log::info!("开机自启: {}", if now_enabled { "已启用" } else { "已禁用" });
                }
                "quit" => {
                    log::info!("用户选择退出");
                    unsafe { PostQuitMessage(0); }
                }
                _ => {}
            }
        }

        // 分发消息给各组件的内部窗口过程（tray-icon、global-hotkey 等）
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    log::info!("clipImg 已退出");
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

/// 在 save_dir 中查找 latest_file.* 文件
#[cfg(target_os = "windows")]
fn find_latest_file(save_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(save_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "latest_file" || name.starts_with("latest_file.") {
            return Some(entry.path());
        }
    }
    None
}

/// 显示启动提示弹窗（MessageBoxW，零额外依赖）
#[cfg(target_os = "windows")]
fn show_notification(title: &str, message: &str, _save_dir: &std::path::Path) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let title = title.to_string();
    let message = message.to_string();
    std::thread::spawn(move || {
        let wide_msg: Vec<u16> = OsStr::new(&message)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_title: Vec<u16> = OsStr::new(&title)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                wide_msg.as_ptr(),
                wide_title.as_ptr(),
                0x40, // MB_ICONINFORMATION
            );
        }
    });
}

// ============================================================
// 配置重载逻辑
// ============================================================

#[cfg(target_os = "windows")]
fn do_reload_config(
    config: &std::rc::Rc<std::cell::RefCell<config::AppConfig>>,
    config_path: &std::path::Path,
    hotkey_manager: &std::rc::Rc<std::cell::RefCell<Option<global_hotkey::GlobalHotKeyManager>>>,
    status_item: &muda::MenuItem,
    preview_item: &muda::MenuItem,
    exe_dir: &std::path::Path,
    version: &str,
) {
    use config::AppConfig;
    use global_hotkey::hotkey::HotKey;

    log::info!("重新加载配置文件: {}", config_path.display());

    // 文件不存在时：回写内存中的当前配置，不创建默认配置
    if !config_path.exists() {
        log::warn!("配置文件不存在，回写当前内存配置到磁盘");
        if let Err(e) = config.borrow().save(config_path) {
            log::error!("回写配置文件失败: {}", e);
        }
        return;
    }

    let new_config = match AppConfig::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            log::error!("重新加载配置失败: {}", e);
            return;
        }
    };

    let old_hotkey_mode = config.borrow().is_hotkey_mode();
    let old_hotkey = config.borrow().hotkey.clone();
    let hotkey_changed = old_hotkey_mode != new_config.is_hotkey_mode()
        || old_hotkey != new_config.hotkey;

    // 更新配置
    *config.borrow_mut() = new_config.clone();

    // 热键变化时重新注册
    if hotkey_changed {
        // 先反注册旧热键
        if old_hotkey_mode {
            if let Some(ref mgr) = *hotkey_manager.borrow() {
                let old_key: HotKey = match HotKey::try_from(old_hotkey.clone()) {
                    Ok(k) => k,
                    Err(_) => {
                        log::warn!("旧热键 '{}' 无法解析，跳过反注册", old_hotkey);
                        return;
                    }
                };
                if let Err(e) = mgr.unregister(old_key) {
                    log::warn!("反注册旧热键失败: {:?}", e);
                } else {
                    log::info!("已反注册旧热键: {}", old_hotkey);
                }
            }
        }

        // 注册新热键
        if new_config.is_hotkey_mode() {
            if hotkey_manager.borrow().is_none() {
                match global_hotkey::GlobalHotKeyManager::new() {
                    Ok(mgr) => *hotkey_manager.borrow_mut() = Some(mgr),
                    Err(e) => {
                        log::error!("创建热键管理器失败: {:?}", e);
                        return;
                    }
                }
            }

            let new_key: HotKey = match HotKey::try_from(new_config.hotkey.clone()) {
                Ok(k) => k,
                Err(e) => {
                    log::error!("解析新热键 '{}' 失败: {:?}", new_config.hotkey, e);
                    return;
                }
            };

            if let Some(ref mgr) = *hotkey_manager.borrow() {
                match mgr.register(new_key) {
                    Ok(()) => log::info!("新热键已注册: {}", new_config.hotkey),
                    Err(e) => log::error!("注册新热键失败: {:?}", e),
                }
            }
        } else {
            // 切换到剪贴板模式，释放热键管理器
            *hotkey_manager.borrow_mut() = None;
            log::info!("已切换到剪贴板模式，热键管理器已释放");
        }
    }

    // 更新 watcher 配置
    // （watcher 在 main 中也用 Rc<RefCell> 包裹，但这里无法直接访问。
    //  watcher.config 会在下次 poll 时读到新的 config... 不对，watcher 有自己的 config 副本。
    //  实际上 watcher 的 config 是独立副本，需要在 main 的 loop 中也更新它。
    //  由于 do_reload_config 被 main loop 调用后，watcher 的 config 需要同步。
    //  这个在 main loop 外面处理比较麻烦，先不更新 watcher.config，
    //  因为 watcher 的核心逻辑（保存目录、MD5去重）在启动时已确定。
    //  真正影响行为的是 config.borrow() 读取的值，这已经更新了。

    // 更新状态栏文字
    let mode_label = if new_config.is_hotkey_mode() {
        format!("clipImg v{} [{}]", version, new_config.hotkey)
    } else {
        format!("clipImg v{} [剪贴板模式]", version)
    };
    status_item.set_text(&mode_label);

    // 更新预览快捷键菜单项
    let preview_label = {
        let phk = new_config.preview_hotkey.trim();
        if phk.is_empty() {
            "预览功能已关闭".to_string()
        } else {
            format!("预览快捷键: {}", phk)
        }
    };
    preview_item.set_text(&preview_label);

    log::info!("配置已重新加载完成 (save_dir: {})", new_config.resolved_save_dir(exe_dir).display());
}

// ============================================================
// 配置文件监控线程
// ============================================================

#[cfg(target_os = "windows")]
struct ConfigWatcher {
    thread: Option<std::thread::JoinHandle<()>>,
    thread_id: u32,
}

#[cfg(target_os = "windows")]
impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;
        use windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT;
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0); }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
fn start_config_watcher(
    main_thread_id: u32,
    notify_msg: u32,
    config_path: std::path::PathBuf,
) -> ConfigWatcher {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("config-watcher".into())
        .spawn(move || {
            let watch_dir = match config_path.parent() {
                Some(dir) => dir.to_path_buf(),
                None => return,
            };
            let config_file_name = config_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("config.json")
                .to_string();

            // 用 CreateFileW 打开目录，获取目录句柄
            let watch_dir_wide: Vec<u16> = OsStr::new(watch_dir.to_str().unwrap_or(""))
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let dir_handle: HANDLE = unsafe {
                CreateFileW(
                    watch_dir_wide.as_ptr(),
                    FILE_LIST_DIRECTORY,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    std::ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                    std::ptr::null_mut(),
                )
            };
            if dir_handle == INVALID_HANDLE_VALUE {
                log::error!("CreateFileW 打开目录失败: {}", watch_dir.display());
                return;
            }

            // 创建事件对象用于 ReadDirectoryChangesW
            let event: HANDLE = unsafe {
                CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null())
            };
            if event.is_null() {
                log::error!("CreateEventW 失败");
                unsafe { CloseHandle(dir_handle); }
                return;
            }

            let mut buffer = [0u8; 4096];
            let mut bytes_returned: u32 = 0;

            let thread_id = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
            let _ = ready_tx.send(thread_id);

            loop {
                // 重置事件
                unsafe {
                    windows_sys::Win32::System::Threading::ResetEvent(event);
                }

                // 开始监听目录变化
                let mut overlapped = windows_sys::Win32::System::IO::OVERLAPPED {
                    Internal: 0,
                    InternalHigh: 0,
                    Anonymous: windows_sys::Win32::System::IO::OVERLAPPED_0 {
                        Anonymous: windows_sys::Win32::System::IO::OVERLAPPED_0_0 {
                            Offset: 0,
                            OffsetHigh: 0,
                        },
                    },
                    hEvent: event,
                };

                let result = unsafe {
                    windows_sys::Win32::Storage::FileSystem::ReadDirectoryChangesW(
                        dir_handle,
                        buffer.as_mut_ptr() as *mut _,
                        buffer.len() as u32,
                        0, // watch subtree = false
                        0x01 | 0x02 | 0x04 | 0x08 | 0x10, // FILE_NOTIFY_CHANGE_FILE_NAME | ATTRIBUTES | SIZE | LAST_WRITE | SECURITY
                        &mut bytes_returned,
                        &mut overlapped,
                        None,
                    )
                };

                // overlapped I/O：返回 0 + ERROR_IO_PENDING(997) 是正常的 pending 状态
                if result == 0 {
                    let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
                    if err != 997 { // ERROR_IO_PENDING
                        log::error!("ReadDirectoryChangesW 失败: 错误码 {}", err);
                        break;
                    }
                }

                // 等待事件触发或 WM_QUIT（通过 PostThreadMessageW）
                let wait_result = unsafe {
                    WaitForSingleObject(event, 1000) // 1秒超时，定期检查 WM_QUIT
                };

                if wait_result == WAIT_OBJECT_0 {
                    // 通过 GetOverlappedResult 获取实际传输字节数
                    let mut bytes_transferred: u32 = 0;
                    let ok = unsafe {
                        windows_sys::Win32::System::IO::GetOverlappedResult(
                            dir_handle,
                            &overlapped,
                            &mut bytes_transferred,
                            0,
                        )
                    };

                    // 检查是否是 config.json 变化
                    if ok != 0 && bytes_transferred > 0 {
                        let mut offset = 0;
                        loop {
                            let notify = unsafe {
                                &*(buffer.as_ptr().add(offset) as *const windows_sys::Win32::Storage::FileSystem::FILE_NOTIFY_INFORMATION)
                            };
                            let filename_len = notify.FileNameLength as usize / 2;
                            let filename: String = String::from_utf16_lossy(
                                unsafe { std::slice::from_raw_parts(notify.FileName.as_ptr(), filename_len) }
                            );

                            if filename == config_file_name {
                                // 防抖：等 100ms 确认稳定
                                std::thread::sleep(std::time::Duration::from_millis(100));
                                log::info!("检测到配置文件变化: {}", filename);
                                unsafe {
                                    PostThreadMessageW(main_thread_id, notify_msg, 0, 0);
                                }
                                break;
                            }

                            if notify.NextEntryOffset == 0 {
                                break;
                            }
                            offset += notify.NextEntryOffset as usize;
                        }
                    }
                }

                // 检查是否有 WM_QUIT 消息（非阻塞）
                let mut msg: windows_sys::Win32::UI::WindowsAndMessaging::MSG = unsafe { std::mem::zeroed() };
                unsafe {
                    if windows_sys::Win32::UI::WindowsAndMessaging::PeekMessageW(
                        &mut msg,
                        std::ptr::null_mut(),
                        0x0012, // WM_QUIT
                        0x0012,
                        0x0001, // PM_REMOVE
                    ) != 0 {
                        break;
                    }
                }
            }

            unsafe { CloseHandle(event); }
            unsafe { CloseHandle(dir_handle); }
            log::info!("配置文件监控线程已退出");
        })
        .expect("创建配置监控线程失败");

    let thread_id = ready_rx.recv().expect("配置监控线程初始化失败");

    log::info!("配置文件监控已启动");
    ConfigWatcher {
        thread: Some(thread),
        thread_id,
    }
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

/// 判断文件是否为可执行文件（预览时拦截，防止误运行）
/// 内置黑名单与用户自定义黑名单取并集
#[cfg(target_os = "windows")]
fn is_executable_file(path: &std::path::Path, user_blocked: &[String]) -> bool {
    const BLOCKED_EXTENSIONS: &[&str] = &[
        "exe", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh",
        "msi", "scr", "com", "pif", "cpl",
        "sh", "bash", "py", "pyw", "rb", "pl", "php",
    ];
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };
    if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
        return true;
    }
    user_blocked.iter().any(|b| b.eq_ignore_ascii_case(&ext))
}
