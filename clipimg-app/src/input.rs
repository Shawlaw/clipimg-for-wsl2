/// Win32 SendInput 实现 Unicode 文字输入
///
/// 仅在 Windows 上编译，其他平台为空实现

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE,
};

#[cfg(target_os = "windows")]
const KEYEVENTF_KEYUP: u32 = 0x0002;

#[cfg(target_os = "windows")]
pub fn send_text(text: &str) -> Result<(), String> {
    use std::mem::size_of;

    let inputs: Vec<INPUT> = text
        .chars()
        .flat_map(|c| {
            let mut key_inputs = vec![make_keyboard_input(c as u16, KEYEVENTF_UNICODE)];
            key_inputs.push(make_keyboard_input(c as u16, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
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
        #[cfg(not(target_os = "windows"))]
        {
            let result = send_text("test");
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("仅在 Windows"));
        }
    }

    #[test]
    fn test_send_text_empty() {
        #[cfg(target_os = "windows")]
        {
            let result = send_text("");
            assert!(result.is_ok());
        }
    }
}
