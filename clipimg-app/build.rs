fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    println!("cargo:rerun-if-changed=resource.rc");
    println!("cargo:rerun-if-changed=icons/icon.ico");

    // 查找资源编译器
    let rc = std::env::var("RC").ok()
        .or_else(|| which("llvm-rc"))
        .or_else(|| which("llvm-rc-20"));

    let rc = match rc {
        Some(rc) => rc,
        None => {
            println!("cargo:warning=未找到资源编译器 (llvm-rc)，跳过图标嵌入");
            return;
        }
    };

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let res_path = format!("{}/resource.res", out_dir);

    let status = std::process::Command::new(&rc)
        .arg("-no-preprocess")
        .arg("resource.rc")
        .arg("/FO")
        .arg(&res_path)
        .status()
        .expect("无法运行资源编译器");

    if status.success() {
        println!("cargo:rustc-link-arg={}", res_path);
    } else {
        println!("cargo:warning=资源编译失败，EXE 将不包含图标");
    }
}

fn which(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !path.is_empty() { Some(path) } else { None }
            } else {
                None
            }
        })
}
