/// 剪贴板变化监听器
///
/// 使用 Win32 `AddClipboardFormatListener` 注册 Message-Only Window
/// 接收 `WM_CLIPBOARDUPDATE` 消息，通过 `PostThreadMessageW` 通知主线程。
/// 替代轮询方案，实现零空闲 CPU。

#[cfg(target_os = "windows")]
use std::cell::RefCell;

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostThreadMessageW,
    RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WM_CLIPBOARDUPDATE, WM_DESTROY, WM_QUIT,
    WNDCLASSW,
};

/// thread_local 存储主线程 ID 和通知消息 ID，供 wnd_proc 回调使用
#[cfg(target_os = "windows")]
thread_local! {
    static NOTIFY_INFO: RefCell<Option<(u32, u32)>> = const { RefCell::new(None) }; // (main_thread_id, notify_msg)
}

/// 窗口过程 — 收到 WM_CLIPBOARDUPDATE 时 PostThreadMessageW 通知主线程
#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            NOTIFY_INFO.with(|cell| {
                if let Some((main_tid, notify_msg)) = *cell.borrow() {
                    PostThreadMessageW(main_tid, notify_msg, 0, 0);
                }
            });
            0
        }
        WM_DESTROY => {
            RemoveClipboardFormatListener(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// 剪贴板监听器
///
/// 析构时自动通知监听线程退出。
#[cfg(target_os = "windows")]
pub struct ClipboardListener {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl ClipboardListener {
    /// 启动剪贴板监听线程
    ///
    /// `main_thread_id`: 主线程 ID，用于 PostThreadMessageW 通知
    /// `notify_msg`: 自定义消息 ID（由 RegisterWindowMessageW 生成）
    pub fn start(main_thread_id: u32, notify_msg: u32) -> Result<Self, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        let thread = std::thread::Builder::new()
            .name("clipboard-listener".into())
            .spawn(move || {
                // 设置 thread_local，供 wnd_proc 回调使用
                NOTIFY_INFO.with(|cell| {
                    *cell.borrow_mut() = Some((main_thread_id, notify_msg));
                });

                listener_main(ready_tx);
            })
            .map_err(|e| format!("创建监听线程失败: {}", e))?;

        let thread_id = ready_rx
            .recv()
            .map_err(|e| format!("监听线程初始化失败: {}", e))?;

        Ok(Self {
            thread_id,
            thread: Some(thread),
        })
    }
}

#[cfg(target_os = "windows")]
impl Drop for ClipboardListener {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 监听线程主逻辑
#[cfg(target_os = "windows")]
fn listener_main(ready_tx: std::sync::mpsc::Sender<u32>) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let class_name: Vec<u16> = OsStr::new("clipImgClipboardListener")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // 注册窗口类
        let wnd_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };

        if RegisterClassW(&wnd_class) == 0 {
            log::error!("RegisterClassW 失败: {}", std::io::Error::last_os_error());
            return;
        }

        // 创建 Message-Only Window（不可见、不接收广播消息）
        let hwnd = CreateWindowExW(
            0, // dwExStyle
            class_name.as_ptr(),
            std::ptr::null(),
            0, // dwStyle
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );

        if hwnd.is_null() {
            log::error!("CreateWindowExW 失败: {}", std::io::Error::last_os_error());
            return;
        }

        // 注册剪贴板变化监听
        if AddClipboardFormatListener(hwnd) == 0 {
            log::error!(
                "AddClipboardFormatListener 失败: {}",
                std::io::Error::last_os_error()
            );
            return;
        }

        let thread_id = GetCurrentThreadId();
        log::info!(
            "剪贴板监听线程已启动 (thread_id={}, hwnd={:?})",
            thread_id,
            hwnd
        );

        // 通知主线程：初始化完成
        if ready_tx.send(thread_id).is_err() {
            RemoveClipboardFormatListener(hwnd);
            return;
        }

        // 消息循环（阻塞等待，零 CPU）
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // 退出时清理
        RemoveClipboardFormatListener(hwnd);
        log::info!("剪贴板监听线程已退出");
    }
}
