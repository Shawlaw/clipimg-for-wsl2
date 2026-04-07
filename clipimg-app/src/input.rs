/// Win32 SendInput 实现 Unicode 文字输入
///
/// 仅在 Windows 上编译，其他平台为空实现

#[cfg(target_os = "windows")]
pub fn send_text(text: &str) -> Result<(), String> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE, VK_SPACE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    let inputs: Vec<INPUT> = text
        .chars()
        .flat_map(|c| {
            let mut key_inputs = vec![make_keyboard_input(c as u16, KEYEVENTF_UNICODE)];
            // 释放事件（keyup）
            key_inputs.push(make_keyboard_input(
                c as u16,
                KEYEVENTF_UNICODE | 0x0002, // KEYEVENTF_KEYUP
            ));
            key_inputs
        })
        .collect();

    if inputs.is_empty() {
        return Ok(());
    }

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };

    if sent == 0 {
        Err(format!("SendInput 失败，错误码: {}", std::io::Error::last_os_error()))
    } else {
        log::debug!("SendInput 已发送 {} 个字符事件", sent / 2);
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn make_keyboard_input(w_scan: u16, dw_flags: u32) -> INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_KEYBOARD, KEYBDINPUT};
    use std::mem::zeroed;

    let mut input: INPUT = unsafe { zeroed() };
    input.r#type = INPUT_KEYBOARD;
    unsafe {
        input.Anonymous.ki = KEYBDINPUT {
            wVk: 0,
            wScan: w_scan,
            dwFlags: dw_flags,
            time: 0,
            dwExtraInfo: 0,
        };
    }
    input
}

#[cfg(not(target_os = "windows"))]
pub fn send_text(_text: &str) -> Result<(), String> {
    Err("SendInput 仅在 Windows 上可用".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_text_non_windows() {
        // 在非 Windows 平台上应返回错误
        #[cfg(not(target_os = "windows"))]
        {
            let result = send_text("test");
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("仅在 Windows"));
        }
    }

    #[test]
    fn test_send_text_empty() {
        // 空字符串在 Windows 上应该直接返回 Ok
        #[cfg(target_os = "windows")]
        {
            let result = send_text("");
            assert!(result.is_ok());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_make_keyboard_input_flags() {
        let input = make_keyboard_input(0x41, KEYEVENTF_UNICODE);
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        unsafe {
            assert_eq!(input.Anonymous.ki.wScan, 0x41);
            assert_eq!(input.Anonymous.ki.dwFlags, KEYEVENTF_UNICODE);
        }
    }
}
