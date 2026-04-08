/// 路径输入模块 — 双模式
///
/// 模式 A（热键模式）：SendInput + KEYEVENTF_UNICODE + IME 临时切换
/// 模式 C（无热键模式）：多格式剪贴板（CF_UNICODETEXT + CF_DIB + CF_HDROP）

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::{HGLOBAL, HWND};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, LoadKeyboardLayoutW, SendInput, ACTIVATE_KEYBOARD_LAYOUT_FLAGS, HKL,
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, HWND_BROADCAST,
    WM_INPUTLANGCHANGEREQUEST,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

// 剪贴板格式常量
#[cfg(target_os = "windows")]
const CF_UNICODETEXT: u32 = 13;
#[cfg(target_os = "windows")]
const CF_DIB: u32 = 8;
#[cfg(target_os = "windows")]
const CF_HDROP: u32 = 15;

// KLF_ACTIVATE flag
#[cfg(target_os = "windows")]
const KLF_ACTIVATE: ACTIVATE_KEYBOARD_LAYOUT_FLAGS = 0x00000001;

// ============================================================================
// 模式 A：SendInput + KEYEVENTF_UNICODE + IME 临时切换
// ============================================================================

/// 模式 A：通过 SendInput + IME 切换自动输入文本（不碰剪贴板）
#[cfg(target_os = "windows")]
pub fn send_text_with_ime(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    log::debug!("send_text_with_ime 开始: '{}'", text);

    unsafe {
        let hwnd = GetForegroundWindow();
        let mut process_id: u32 = 0;
        let thread_id = GetWindowThreadProcessId(hwnd, &mut process_id);

        // 保存当前键盘布局
        let prev_hkl = GetKeyboardLayout(thread_id);

        // 切换到英文布局
        let eng_hkl = load_keyboard_layout("00000409");
        switch_input_language(eng_hkl);

        // 等待切换生效
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 逐字符发送
        for ch in text.chars() {
            send_unicode_char(ch)?;
        }

        // 等待输入完成
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 恢复原始布局
        switch_input_language(prev_hkl);
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    log::debug!("send_text_with_ime 完成");
    Ok(())
}

/// 加载键盘布局（如 "00000409" = en-US）
#[cfg(target_os = "windows")]
unsafe fn load_keyboard_layout(layout_id: &str) -> HKL {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = OsStr::new(layout_id)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    LoadKeyboardLayoutW(wide.as_ptr(), KLF_ACTIVATE)
}

/// 通过 PostMessage 切换输入语言
#[cfg(target_os = "windows")]
unsafe fn switch_input_language(hkl: HKL) {
    PostMessageW(
        HWND_BROADCAST as HWND,
        WM_INPUTLANGCHANGEREQUEST,
        0,
        hkl as isize,
    );
}

/// 发送单个 Unicode 字符（key-down 和 key-up 分开调用）
#[cfg(target_os = "windows")]
fn send_unicode_char(ch: char) -> Result<(), String> {
    use std::mem::{size_of, zeroed};

    let mut key_down: INPUT = unsafe { zeroed() };
    key_down.r#type = INPUT_KEYBOARD;
    key_down.Anonymous.ki = KEYBDINPUT {
        wVk: 0,
        wScan: ch as u16,
        dwFlags: KEYEVENTF_UNICODE,
        time: 0,
        dwExtraInfo: 0,
    };

    let mut key_up: INPUT = unsafe { zeroed() };
    key_up.r#type = INPUT_KEYBOARD;
    key_up.Anonymous.ki = KEYBDINPUT {
        wVk: 0,
        wScan: ch as u16,
        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
        time: 0,
        dwExtraInfo: 0,
    };

    // 关键：分开调用 SendInput，修复批量发送 bug
    let sent_down =
        unsafe { SendInput(1, &key_down as *const INPUT, size_of::<INPUT>() as i32) };
    if sent_down == 0 {
        return Err(format!(
            "SendInput key-down 失败 (char='{}'): {}",
            ch,
            std::io::Error::last_os_error()
        ));
    }

    let sent_up =
        unsafe { SendInput(1, &key_up as *const INPUT, size_of::<INPUT>() as i32) };
    if sent_up == 0 {
        return Err(format!(
            "SendInput key-up 失败 (char='{}'): {}",
            ch,
            std::io::Error::last_os_error()
        ));
    }

    Ok(())
}

// ============================================================================
// 模式 C：多格式剪贴板（Win32 API 直接操作）
// ============================================================================

/// 模式 C：设置多格式剪贴板
///
/// 同时写入三种格式：
/// - CF_UNICODETEXT: output_path（如 /workspace/.clip/latest.png）
/// - CF_DIB: PNG 图片的 DIB 数据（给图片应用粘贴用）
/// - CF_HDROP: Windows 侧文件路径（给资源管理器粘贴用）
#[cfg(target_os = "windows")]
pub fn set_multi_format_clipboard(
    text_path: &str,
    win_image_path: &std::path::Path,
) -> Result<(), String> {
    log::debug!(
        "set_multi_format_clipboard: text='{}', win_image='{}'",
        text_path,
        win_image_path.display()
    );

    // 读取 PNG 数据
    let png_data = std::fs::read(win_image_path)
        .map_err(|e| format!("读取图片失败: {}", e))?;

    // 将 PNG 转换为 BMP/DIB 格式
    let dib_data = png_to_dib(&png_data)?;

    // Windows 文件路径（用于 CF_HDROP）
    let win_path_str = win_image_path.to_str().unwrap_or("");
    let win_path_wide: Vec<u16> = win_path_str.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return Err(format!("OpenClipboard 失败: {}", std::io::Error::last_os_error()));
        }

        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err(format!(
                "EmptyClipboard 失败: {}",
                std::io::Error::last_os_error()
            ));
        }

        let null_handle: HGLOBAL = std::ptr::null_mut();

        // 1. 设置 CF_UNICODETEXT（路径字符串）
        let text_bytes: Vec<u16> = text_path.encode_utf16().chain(std::iter::once(0)).collect();
        let text_size = text_bytes.len() * 2;
        let text_handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, text_size);
        if text_handle != null_handle {
            let ptr = GlobalLock(text_handle) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(text_bytes.as_ptr(), ptr, text_bytes.len());
                GlobalUnlock(text_handle);
                SetClipboardData(CF_UNICODETEXT, text_handle);
            }
        }

        // 2. 设置 CF_DIB（图片 DIB 数据）
        let dib_size = dib_data.len();
        let dib_handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, dib_size);
        if dib_handle != null_handle {
            let ptr = GlobalLock(dib_handle) as *mut u8;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(dib_data.as_ptr(), ptr, dib_data.len());
                GlobalUnlock(dib_handle);
                SetClipboardData(CF_DIB, dib_handle);
            }
        }

        // 3. 设置 CF_HDROP（文件拖放列表）
        let dropfiles_size = std::mem::size_of::<DROPFILES>() + (win_path_wide.len() + 1) * 2;
        let drop_handle: HGLOBAL = GlobalAlloc(GMEM_MOVEABLE, dropfiles_size);
        if drop_handle != null_handle {
            let ptr = GlobalLock(drop_handle) as *mut u8;
            if !ptr.is_null() {
                let df = DROPFILES {
                    pFiles: std::mem::size_of::<DROPFILES>() as u32,
                    pt: (0, 0),
                    fNC: 0,
                    fWide: 1,
                };
                std::ptr::write(ptr as *mut DROPFILES, df);
                let path_ptr = ptr.add(std::mem::size_of::<DROPFILES>()) as *mut u16;
                std::ptr::copy_nonoverlapping(win_path_wide.as_ptr(), path_ptr, win_path_wide.len());
                *path_ptr.add(win_path_wide.len()) = 0;
                GlobalUnlock(drop_handle);
                SetClipboardData(CF_HDROP, drop_handle);
            }
        }

        CloseClipboard();
    }

    log::info!(
        "多格式剪贴板已设置 (text='{}', dib_size={})",
        text_path,
        dib_data.len()
    );
    Ok(())
}

/// DROPFILES 结构体（用于 CF_HDROP）
#[cfg(target_os = "windows")]
#[repr(C)]
#[allow(non_snake_case)]
struct DROPFILES {
    pFiles: u32,
    pt: (i32, i32),
    fNC: i32,
    fWide: i32,
}

/// 将 PNG 数据转换为 DIB（Device Independent Bitmap）格式
#[cfg(target_os = "windows")]
fn png_to_dib(png_data: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(png_data)
        .map_err(|e| format!("解析 PNG 失败: {}", e))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();

    let header_size: u32 = 40;
    let bpp: u16 = 32;
    let compression: u32 = 0;
    let image_size = (width * height * (bpp as u32) / 8) as u32;

    let mut dib = Vec::with_capacity((header_size + image_size) as usize);

    // BITMAPINFOHEADER
    dib.extend_from_slice(&header_size.to_le_bytes());
    dib.extend_from_slice(&(width as i32).to_le_bytes());
    dib.extend_from_slice(&(height as i32).to_le_bytes());
    dib.extend_from_slice(&[1u8, 0]); // biPlanes
    dib.extend_from_slice(&bpp.to_le_bytes());
    dib.extend_from_slice(&compression.to_le_bytes());
    dib.extend_from_slice(&image_size.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0i32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&0u32.to_le_bytes());

    // 像素数据：DIB 是从下到上、BGRA 格式
    for y in (0..height).rev() {
        for x in 0..width {
            let pixel = rgba.get_pixel(x, y);
            dib.push(pixel[2]); // B
            dib.push(pixel[1]); // G
            dib.push(pixel[0]); // R
            dib.push(pixel[3]); // A
        }
    }

    Ok(dib)
}

// ============================================================================
// 非 Windows 平台 stub
// ============================================================================

#[cfg(not(target_os = "windows"))]
pub fn send_text_with_ime(_text: &str) -> Result<(), String> {
    Err("SendInput 仅在 Windows 上可用".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn set_multi_format_clipboard(
    _text_path: &str,
    _win_image_path: &std::path::Path,
) -> Result<(), String> {
    Err("剪贴板操作仅在 Windows 上可用".to_string())
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_text_non_windows() {
        #[cfg(not(target_os = "windows"))]
        {
            let result = send_text_with_ime("test");
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_send_text_empty() {
        assert!(send_text_with_ime("").is_ok());
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_multi_format_clipboard_non_windows() {
        let result = set_multi_format_clipboard("/test/path.png", std::path::Path::new("C:\\test.png"));
        assert!(result.is_err());
    }
}
