/// 通过剪贴板 + Ctrl+V 输入文本
///
/// 策略：创建独立的剪贴板实例 → 设置文本 → Ctrl+V → 完成
/// 不保存/恢复旧剪贴板内容（守护进程用文件级 MD5 去重，不受剪贴板变化影响）
/// 不与守护进程共享 arboard::Clipboard 实例，避免竞争。

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

#[cfg(target_os = "windows")]
pub fn send_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    log::debug!("send_text 开始: '{}'", text);

    // 创建独立的剪贴板实例，不与守护进程共用
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("打开剪贴板失败: {}", e))?;
    log::debug!("剪贴板实例创建成功");

    // 设置文本到剪贴板
    clipboard.set_text(text)
        .map_err(|e| format!("设置剪贴板文本失败: {}", e))?;
    log::debug!("剪贴板文本已设置 ({} bytes)", text.len());

    // 短暂等待确保剪贴板已更新
    std::thread::sleep(std::time::Duration::from_millis(30));

    // 发送 Ctrl+V
    send_ctrl_v()?;

    // 等待粘贴完成（目标应用处理 Ctrl+V 需要时间）
    std::thread::sleep(std::time::Duration::from_millis(30));

    log::debug!("send_text 完成");
    Ok(())
}

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
pub fn send_text(_text: &str) -> Result<(), String> {
    Err("剪贴板粘贴仅在 Windows 上可用".to_string())
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
