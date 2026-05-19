fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let version_parts: Vec<u32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let v_major = version_parts.get(0).copied().unwrap_or(0);
    let v_minor = version_parts.get(1).copied().unwrap_or(0);
    let v_patch = version_parts.get(2).copied().unwrap_or(0);

    // 文件版本用编译时间：年.月.日.小时
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    // 简单计算年月日时分（UTC+8）
    let total_days = secs / 86400;
    let build_hour = ((secs % 86400) / 3600) as u32;
    // 从 1970-01-01 算起
    let (build_year, build_month, build_day) = days_to_ymd(total_days as i64);

    // 动态生成包含版本信息的 .rc 文件
    let rc_content = format!(
        r#"// EXE 图标
1 ICON "icons/icon.ico"

// 版本信息（Windows 属性面板 → 详细信息）
1 VERSIONINFO
FILEVERSION {build_year},{build_month},{build_day},{build_hour}
PRODUCTVERSION {v_major},{v_minor},{v_patch}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "080404b0"
        BEGIN
            VALUE "CompanyName", "clipImg"
            VALUE "FileDescription", "clipImg - WSL2/Docker clipboard image tool"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "clipimg"
            VALUE "LegalCopyright", "MIT License"
            VALUE "OriginalFilename", "clipimg.exe"
            VALUE "ProductName", "ClipImg"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0804, 1200
    END
END
"#
    );

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let rc_path = format!("{}/resource.rc", out_dir);
    std::fs::write(&rc_path, &rc_content).expect("无法写入 .rc 文件");

    // 不指定 rerun-if-changed，让 cargo 在包内文件变化时自动重新运行
    // 这样每次编译都能拿到最新的编译时间戳

    // 查找资源编译器
    let rc = std::env::var("RC")
        .ok()
        .or_else(|| which("llvm-rc"))
        .or_else(|| which("llvm-rc-20"))
        .or_else(|| find_windows_sdk_rc());

    let rc = match rc {
        Some(rc) => rc,
        None => {
            println!("cargo:warning=未找到资源编译器 (llvm-rc / rc.exe)，跳过图标嵌入");
            return;
        }
    };

    let res_path = format!("{}/resource.res", out_dir);

    // 判断是否为 Windows SDK 的 rc.exe（llvm-rc 用 -no-preprocess，rc.exe 用 /Fo）
    let is_msvc_rc = rc.to_lowercase().ends_with("rc.exe")
        && !rc.to_lowercase().contains("llvm");

    let status = if is_msvc_rc {
        // MSVC rc.exe 语法: rc /fo output.res input.rc
        // 用绝对路径重写 .rc 文件中的图标引用（去掉 \\?\ 前缀）
        let icon_abs = std::path::Path::new("icons/icon.ico")
            .canonicalize()
            .unwrap_or_else(|_| std::path::PathBuf::from("icons/icon.ico"));
        let icon_str = icon_abs.to_string_lossy();
        // 去掉 \\?\ UNC 前缀，rc.exe 不认识
        let icon_clean = icon_str
            .trim_start_matches(r"\\?\")
            .to_string();
        let rc_content_abs = rc_content.replace("\"icons/icon.ico\"", &format!("\"{}\"", icon_clean));
        std::fs::write(&rc_path, &rc_content_abs).expect("无法写入 .rc 文件");

        let result = std::process::Command::new(&rc)
            .arg("/fo")
            .arg(&res_path)
            .arg(&rc_path)
            .output()
            .expect("无法运行资源编译器");
        if !result.status.success() {
            println!("cargo:warning=rc.exe 失败: {}", String::from_utf8_lossy(&result.stderr));
            println!("cargo:warning=rc.exe stdout: {}", String::from_utf8_lossy(&result.stdout));
        }
        result.status
    } else {
        // llvm-rc 语法
        std::process::Command::new(&rc)
            .arg("-no-preprocess")
            .arg(&rc_path)
            .arg("/FO")
            .arg(&res_path)
            .status()
            .expect("无法运行资源编译器")
    };

    if status.success() {
        println!("cargo:rustc-link-arg={}", res_path);
    } else {
        println!("cargo:warning=资源编译失败，EXE 将不包含图标和版本信息");
    }
}

fn days_to_ymd(mut days: i64) -> (u32, u32, u32) {
    let mut y = 1970i64;
    loop {
        let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
        if days < dy {
            break;
        }
        days -= dy;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md: &[u32] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 0u32;
    for (i, &d) in md.iter().enumerate() {
        if days < d as i64 {
            m = i as u32 + 1;
            break;
        }
        days -= d as i64;
    }
    if m == 0 {
        m = 12;
    }
    (y as u32, m, days as u32 + 1)
}

fn which(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !path.is_empty() {
                    Some(path)
                } else {
                    None
                }
            } else {
                None
            }
        })
}

/// 在 Windows SDK 目录中查找 rc.exe（Windows 原生编译时使用）
fn find_windows_sdk_rc() -> Option<String> {
    let sdk_base = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if !sdk_base.exists() {
        return None;
    }

    let mut versions: Vec<String> = std::fs::read_dir(&sdk_base)
        .ok()?
        .filter_map(|e| {
            let name = e.ok()?.file_name();
            let name = name.to_string_lossy().to_string();
            if name.starts_with("10.") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    versions.sort();
    versions.reverse(); // 最新版本优先

    // 优先使用 x64，回退到 x86
    let arches = ["x64", "x86"];

    for ver in &versions {
        for arch in &arches {
            let rc_path = sdk_base.join(ver).join(arch).join("rc.exe");
            if rc_path.exists() {
                let path = rc_path.to_string_lossy().to_string();
                println!("cargo:warning=找到 Windows SDK 资源编译器: {}", path);
                return Some(path);
            }
        }
    }

    None
}
