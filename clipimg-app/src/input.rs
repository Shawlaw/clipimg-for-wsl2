/// 通过 SendInput + VkKeyScanW 模拟键盘输入
///
/// 之前用 KEYEVENTF_UNICODE 在终端不生效，改为用 VkKeyScanW 获取每个字符的
/// 虚拟键码和修饰键状态（Shift 等），直接模拟物理按键。
/// 不修改剪贴板，在所有应用中都可靠工作。

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, VkKeyScanW, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VK_CONTROL, VK_MENU, VK_SHIFT,
};

#[cfg(target_os = "windows")]
pub fn send_text(text: &str) -> Result<(), String> {
    use std::mem::{size_of, zeroed};

    if text.is_empty() {
        return Ok(());
    }

    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 4);

    for c in text.chars() {
        // VkKeyScanW 返回: 低字节 = 虚拟键码, 高字节 = 修饰键状态
        // 修饰键: bit0=Shift, bit1=Ctrl, bit2=Alt
        let vk_result = unsafe { VkKeyScanW(c as u16) };

        if vk_result == -1 {
            // 无法映射的字符，跳过
            log::warn!("VkKeyScanW 无法映射字符: '{}' (0x{:04X}), 跳过", c, c as u16);
            continue;
        }

        let vk_code = (vk_result & 0xFF) as u16;
        let modifiers = (vk_result >> 8) as u8;
        let need_shift = modifiers & 0x01 != 0;
        let need_ctrl = modifiers & 0x02 != 0;
        let need_alt = modifiers & 0x04 != 0;

        // 按下修饰键
        if need_shift {
            inputs.push(make_key_down(VK_SHIFT as u16));
        }
        if need_ctrl {
            inputs.push(make_key_down(VK_CONTROL as u16));
        }
        if need_alt {
            inputs.push(make_key_down(VK_MENU as u16));
        }

        // 按下并释放字符键
        inputs.push(make_key_down(vk_code));
        inputs.push(make_key_up(vk_code));

        // 释放修饰键
        if need_alt {
            inputs.push(make_key_up(VK_MENU as u16));
        }
        if need_ctrl {
            inputs.push(make_key_up(VK_CONTROL as u16));
        }
        if need_shift {
            inputs.push(make_key_up(VK_SHIFT as u16));
        }
    }

    if inputs.is_empty() {
        return Ok(());
    }

    let sent = unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32)
    };

    if sent == 0 {
        Err(format!(
            "SendInput 失败: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        log::debug!("SendInput 成功: {} events ({} chars)", sent, text.len());
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn make_key_down(vk: u16) -> INPUT {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    unsafe {
        input.Anonymous.ki = KEYBDINPUT {
            wVk: vk,
            wScan: 0,
            dwFlags: 0,
            time: 0,
            dwExtraInfo: 0,
        };
    }
    input
}

#[cfg(target_os = "windows")]
fn make_key_up(vk: u16) -> INPUT {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    unsafe {
        input.Anonymous.ki = KEYBDINPUT {
            wVk: vk,
            wScan: 0,
            dwFlags: KEYEVENTF_KEYUP,
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
        }
    }
}
