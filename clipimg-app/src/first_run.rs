/// 首次运行路径确认对话框（双输入框）
/// 使用 Win32 DialogBoxIndirectParamW 创建内存中的模态对话框
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;

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
const EDIT_WIN_DIR: u16 = 101;
const EDIT_CONTAINER_DIR: u16 = 102;
const GWLP_USERDATA: i32 = -21;

/// 对话框上下文
struct Ctx {
    windows_dir: String,
    container_dir: String,
}

/// 弹出双路径确认对话框
/// 返回 Some((save_dir, output_path)) 或 None（用户取消）
pub fn confirm_paths(windows_dir: &str, container_dir: &str) -> Option<(String, String)> {
    let ctx = Box::new(Ctx {
        windows_dir: windows_dir.to_string(),
        container_dir: container_dir.to_string(),
    });
    let ctx_ptr = Box::into_raw(ctx);

    let template = build_template();
    let ret = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::DialogBoxIndirectParamW(
            std::ptr::null_mut(),
            template.as_ptr() as *const _,
            std::ptr::null_mut(),
            Some(dialog_proc),
            ctx_ptr as isize,
        )
    };

    if ret == 1 {
        let ctx = unsafe { Box::from_raw(ctx_ptr) };
        Some((ctx.windows_dir, ctx.container_dir))
    } else {
        unsafe {
            let _ = Box::from_raw(ctx_ptr);
        }
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
    fn push_u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_ne_bytes());
    }
    fn push_u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_ne_bytes());
    }
    fn push_i16(&mut self, v: i16) {
        self.extend_from_slice(&v.to_ne_bytes());
    }
    fn push_str16(&mut self, s: &str) {
        let w: Vec<u16> = OsStr::new(s).encode_wide().chain(once(0u16)).collect();
        for &c in &w {
            self.push_u16(c);
        }
    }
    fn align4(&mut self) {
        while self.len() % 4 != 0 {
            self.push(0);
        }
    }
}

/// 构建双输入框对话框模板
///
/// 布局：
/// ┌──────────────────────────────────────────┐
/// │  以下两个路径指向同一个物理目录（WSL2 挂载） │
/// │  Windows 侧（程序实际写入）：               │
/// │  [edit_win_dir                          ] │
/// │              ↕ 挂载映射                    │
/// │  容器侧（粘贴到终端的路径）：                │
/// │  [edit_container_dir                    ] │
/// │                    [确定]  [取消]           │
/// └──────────────────────────────────────────┘
fn build_template() -> Vec<u8> {
    let mut b = Vec::new();

    // === DLGTEMPLATE ===
    b.push_u32(
        WS_POPUP | WS_VISIBLE | WS_CAPTION | WS_SYSMENU | DS_MODALFRAME | DS_CENTER | DS_SETFONT,
    );
    b.push_u32(0);
    b.push_u16(8); // cdit = 8 controls
    b.push_i16(0);
    b.push_i16(0);
    b.push_i16(280);
    b.push_i16(120); // cx, cy
    b.push_u16(0); // menu
    b.push_u16(0); // class
    b.push_str16("clipImg - 路径配置");
    b.push_u16(9); // font size
    b.push_str16("Segoe UI");

    // Control 1: 说明文字
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(7);
    b.push_i16(5);
    b.push_i16(260);
    b.push_i16(10);
    b.push_u16(0);
    b.push_u16(0xFFFF);
    b.push_u16(0x0082); // STATIC
    b.push_str16("以下两个路径指向同一个物理目录（WSL2 挂载）");
    b.push_u16(0);

    // Control 2: Windows 侧标签
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(7);
    b.push_i16(20);
    b.push_i16(260);
    b.push_i16(10);
    b.push_u16(0);
    b.push_u16(0xFFFF);
    b.push_u16(0x0082);
    b.push_str16("Windows 侧（程序实际写入）：");
    b.push_u16(0);

    // Control 3: Windows 侧输入框
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL);
    b.push_u32(0);
    b.push_i16(7);
    b.push_i16(33);
    b.push_i16(260);
    b.push_i16(14);
    b.push_u16(EDIT_WIN_DIR);
    b.push_u16(0xFFFF);
    b.push_u16(0x0081); // EDIT
    b.push_u16(0);
    b.push_u16(0);

    // Control 4: 映射标注
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(105);
    b.push_i16(50);
    b.push_i16(60);
    b.push_i16(10);
    b.push_u16(0);
    b.push_u16(0xFFFF);
    b.push_u16(0x0082);
    b.push_str16("↕ 挂载映射");
    b.push_u16(0);

    // Control 5: 容器侧标签
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE);
    b.push_u32(0);
    b.push_i16(7);
    b.push_i16(63);
    b.push_i16(260);
    b.push_i16(10);
    b.push_u16(0);
    b.push_u16(0xFFFF);
    b.push_u16(0x0082);
    b.push_str16("容器侧（粘贴到终端的路径）：");
    b.push_u16(0);

    // Control 6: 容器侧输入框
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL);
    b.push_u32(0);
    b.push_i16(7);
    b.push_i16(76);
    b.push_i16(260);
    b.push_i16(14);
    b.push_u16(EDIT_CONTAINER_DIR);
    b.push_u16(0xFFFF);
    b.push_u16(0x0081); // EDIT
    b.push_u16(0);
    b.push_u16(0);

    // Control 7: OK
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON);
    b.push_u32(0);
    b.push_i16(170);
    b.push_i16(98);
    b.push_i16(50);
    b.push_i16(14);
    b.push_u16(IDOK);
    b.push_u16(0xFFFF);
    b.push_u16(0x0080); // BUTTON
    b.push_str16("确定");
    b.push_u16(0);

    // Control 8: Cancel
    b.align4();
    b.push_u32(WS_CHILD | WS_VISIBLE | WS_TABSTOP);
    b.push_u32(0);
    b.push_i16(225);
    b.push_i16(98);
    b.push_i16(50);
    b.push_i16(14);
    b.push_u16(IDCANCEL);
    b.push_u16(0xFFFF);
    b.push_u16(0x0080);
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
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                lparam,
            );

            let ctx = &*(lparam as *const Ctx);

            // 设置 Windows 侧输入框
            let wide1: Vec<u16> = OsStr::new(&ctx.windows_dir)
                .encode_wide()
                .chain(once(0u16))
                .collect();
            windows_sys::Win32::UI::WindowsAndMessaging::SetDlgItemTextW(
                hwnd,
                EDIT_WIN_DIR as i32,
                wide1.as_ptr(),
            );

            // 设置容器侧输入框
            let wide2: Vec<u16> = OsStr::new(&ctx.container_dir)
                .encode_wide()
                .chain(once(0u16))
                .collect();
            windows_sys::Win32::UI::WindowsAndMessaging::SetDlgItemTextW(
                hwnd,
                EDIT_CONTAINER_DIR as i32,
                wide2.as_ptr(),
            );

            1
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            match id {
                IDOK => {
                    let ptr = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                        hwnd,
                        GWLP_USERDATA,
                    );
                    let ctx = &mut *(ptr as *mut Ctx);

                    let mut buf = [0u16; 512];

                    // 读取 Windows 侧
                    let len1 = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItemTextW(
                        hwnd,
                        EDIT_WIN_DIR as i32,
                        buf.as_mut_ptr(),
                        512,
                    );
                    ctx.windows_dir = String::from_utf16_lossy(&buf[..len1 as usize]);

                    // 读取容器侧
                    let len2 = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItemTextW(
                        hwnd,
                        EDIT_CONTAINER_DIR as i32,
                        buf.as_mut_ptr(),
                        512,
                    );
                    ctx.container_dir = String::from_utf16_lossy(&buf[..len2 as usize]);

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
