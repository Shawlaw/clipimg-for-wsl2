/// 首次运行路径确认对话框
/// 使用 Win32 DialogBoxIndirectParamW 创建内存中的模态对话框

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::iter::once;

// Win32 常量
const WS_POPUP: u32 = 0x80000000;
const WS_VISIBLE: u32 = 0x10000000;
const WS_CAPTION: u32 = 0x00C00000;
const WS_SYSMENU: u32 = 0x00080000;
const WS_CHILD: u32 = 0x40000000;
const WS_BORDER: u32 = 0x00800000;
const WS_TABSTOP: u32 = 0x00010000;
const DS_MODALFRAME: u32 = 0x80;
const DS_CENTER: u32 = 0x800;
const DS_SETFONT: u32 = 0x40;
const BS_DEFPUSHBUTTON: u32 = 0x01;
const ES_AUTOHSCROLL: u32 = 0x80;

const WM_INITDIALOG: u32 = 0x0110;
const WM_COMMAND: u32 = 0x0111;
const WM_CLOSE: u32 = 0x0010;
const IDOK: u16 = 1;
const IDCANCEL: u16 = 2;
const EDIT_ID: u16 = 101;
const GWLP_USERDATA: i32 = -21;

/// 对话框上下文，在 WM_INITDIALOG 时存入 GWLP_USERDATA
struct Ctx {
    path: String,
}

/// 弹出路径确认对话框
/// 返回 Some(path) 表示用户确认，None 表示取消
pub fn confirm_save_dir(proposed_path: &str) -> Option<String> {
    let ctx = Box::new(Ctx {
        path: proposed_path.to_string(),
    });
    let ctx_ptr = Box::into_raw(ctx);

    let template = build_template();
    let ret = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::DialogBoxIndirectParamW(
            std::ptr::null_mut(), // hInstance: null 使用当前模块
            template.as_ptr() as *const _,
            std::ptr::null_mut(), // hWndParent
            Some(dialog_proc),
            ctx_ptr as isize,
        )
    };

    if ret == 1 {
        let ctx = unsafe { Box::from_raw(ctx_ptr) };
        Some(ctx.path)
    } else {
        unsafe { let _ = Box::from_raw(ctx_ptr); }
        None
    }
}

trait BytesExt {
    fn push_u32(&mut self, v: u32);
    fn push_u16(&mut self, v: u16);
    fn push_i16(&mut self, v: i16);
    fn push_str16(&mut self, s: &str);
    fn align4(&mut self);
}

impl BytesExt for Vec<u8> {
    fn push_u32(&mut self, v: u32) { self.extend_from_slice(&v.to_ne_bytes()); }
    fn push_u16(&mut self, v: u16) { self.extend_from_slice(&v.to_ne_bytes()); }
    fn push_i16(&mut self, v: i16) { self.extend_from_slice(&v.to_ne_bytes()); }
    fn push_str16(&mut self, s: &str) {
        let w: Vec<u16> = OsStr::new(s).encode_wide().chain(once(0u16)).collect();
        for &c in &w { self.push_u16(c); }
    }
    fn align4(&mut self) { while self.len() % 4 != 0 { self.push(0); } }
}

/// 构建内存中的对话框模板
fn build_template() -> Vec<u8> {
    let mut b = Vec::new();

    // === DLGTEMPLATE ===
    b.push_u32(WS_POPUP | WS_VISIBLE | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER | DS_SETFONT);
    b.push_u32(0); // dwExtendedStyle
    b.push_u16(4); // cdit = 4 controls
    b.push_i16(0); b.push_i16(0); // x, y
    b.push_i16(260); b.push_i16(80); // cx, cy (dialog units)
    b.push_u16(0); // menu = none
    b.push_u16(0); // class = default
    b.push_str16("clipImg - 确认保存路径");
    b.push_u16(9); // font size
    b.push_str16("Segoe UI"); // font face

    // === Control 1: Static label ===
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(7); b.push_i16(7); b.push_i16(240); b.push_i16(10);
    b.push_u16(0); // id (static, not needed)
    b.push_u16(0xFFFF); b.push_u16(0x0082); // STATIC class
    b.push_str16("图片保存路径（可修改后点击确定）：");
    b.push_u16(0); // no creation data

    // === Control 2: Edit ===
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL);
    b.push_u32(0);
    b.push_i16(7); b.push_i16(22); b.push_i16(240); b.push_i16(14);
    b.push_u16(EDIT_ID);
    b.push_u16(0xFFFF); b.push_u16(0x0081); // EDIT class
    b.push_u16(0); // empty initial text
    b.push_u16(0); // no creation data

    // === Control 3: OK button ===
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON);
    b.push_u32(0);
    b.push_i16(150); b.push_i16(45); b.push_i16(50); b.push_i16(14);
    b.push_u16(IDOK);
    b.push_u16(0xFFFF); b.push_u16(0x0080); // BUTTON class
    b.push_str16("确定");
    b.push_u16(0);

    // === Control 4: Cancel button ===
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP);
    b.push_u32(0);
    b.push_i16(205); b.push_i16(45); b.push_i16(50); b.push_i16(14);
    b.push_u16(IDCANCEL);
    b.push_u16(0xFFFF); b.push_u16(0x0080); // BUTTON class
    b.push_str16("取消");
    b.push_u16(0);

    b
}

/// 对话框过程
unsafe extern "system" fn dialog_proc(
    hwnd: *mut std::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            // 保存上下文指针到窗口用户数据
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd, GWLP_USERDATA, lparam,
            );

            // 设置编辑框初始文本
            let ctx = &*(lparam as *const Ctx);
            let wide: Vec<u16> = OsStr::new(&ctx.path).encode_wide().chain(once(0u16)).collect();
            windows_sys::Win32::UI::WindowsAndMessaging::SetDlgItemTextW(
                hwnd, EDIT_ID as i32, wide.as_ptr(),
            );

            1 // TRUE = handled
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            match id {
                IDOK => {
                    // 从编辑框获取文本
                    let ptr = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                        hwnd, GWLP_USERDATA,
                    );
                    let ctx = &mut *(ptr as *mut Ctx);

                    let mut buf = [0u16; 512];
                    let len = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItemTextW(
                        hwnd, EDIT_ID as i32, buf.as_mut_ptr(), 512,
                    );
                    ctx.path = String::from_utf16_lossy(&buf[..len as usize]);

                    windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 1);
                    1
                }
                IDCANCEL => {
                    windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 0);
                    1
                }
                _ => 0,
            }
        }
        WM_CLOSE => {
            windows_sys::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}
