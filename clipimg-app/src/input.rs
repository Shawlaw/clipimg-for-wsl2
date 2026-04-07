/// 通过剪贴板 + Ctrl+V 模拟文字输入
///
/// SendInput + KEYEVENTF_UNICODE 在终端等应用下不生效，
/// 改用剪贴板方式：设置剪贴板文本 → 模拟 Ctrl+V → 恢复剪贴板。
/// 这在所有 Windows 应用中都可靠工作。

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

/// 通过剪贴板粘贴方式发送文本
#[cfg(target_os = "windows")]
pub fn send_text(clipboard: &mut arboard::Clipboard, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    // 保存当前剪贴板图片（如果有）
    let saved_image = clipboard.get_image().ok();

    // 设置剪贴板为文本
    clipboard.set_text(text).map_err(|e| format!("设置剪贴板失败: {}", e))?;

    // 短暂延迟确保剪贴板已更新
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 模拟 Ctrl+V
    send_ctrl_v()?;

    // 短暂延迟确保粘贴完成
    std::thread::sleep(std::time::Duration::from_millis(50));

    // 恢复剪贴板图片
    if let Some(img) = saved_image {
        if let Err(e) = clipboard.set_image(img) {
            log::warn!("恢复剪贴板图片失败: {}", e);
        }
    }

    Ok(())
}

/// 发送 Ctrl+V 按键序列
#[cfg(target_os = "windows")]
fn send_ctrl_v() -> Result<(), String> {
    use std::mem::{size_of, zeroed};

    let mut inputs: [INPUT; 4] = unsafe { [zeroed(), zeroed(), zeroed(), zeroed()] };

    // Ctrl down
    inputs[0].r#type = INPUT_KEYBOARD;
    unsafe { inputs[0].Anonymous.ki = KEYBDINPUT { wVk: VK_CONTROL, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 } };

    // V down
    inputs[1].r#type = INPUT_KEYBOARD;
    unsafe { inputs[1].Anonymous.ki = KEYBDINPUT { wVk: VK_V, wScan: 0, dwFlags: 0, time: 0, dwExtraInfo: 0 } };

    // V up
    inputs[2].r#type = INPUT_KEYBOARD;
    unsafe { inputs[2].Anonymous.ki = KEYBDINPUT { wVk: VK_V, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } };

    // Ctrl up
    inputs[3].r#type = INPUT_KEYBOARD;
    unsafe { inputs[3].Anonymous.ki = KEYBDINPUT { wVk: VK_CONTROL, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } };

    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };

    if sent == 0 {
        Err(format!("SendInput Ctrl+V 失败: {}", std::io::Error::last_os_error()))
    } else {
        log::debug!("SendInput Ctrl+V 成功 ({} events)", sent);
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub fn send_text(_clipboard: &mut (), _text: &str) -> Result<(), String> {
    Err("剪贴板粘贴仅在 Windows 上可用".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_text_non_windows() {
        #[cfg(not(target_os = "windows"))]
        {
            let result = send_text(&mut (), "test");
            assert!(result.is_err());
        }
    }
}
