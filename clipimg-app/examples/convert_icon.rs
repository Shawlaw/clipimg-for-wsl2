/// 从 icon_1024.png 生成各尺寸图标
/// Run: cargo run --example convert_icon
use image::imageops::FilterType;
use std::path::Path;

fn main() {
    let src = "icons/icon_1024.png";
    let img = image::open(src).expect("无法打开 icon_1024.png");

    let out_dir = Path::new("icons");

    // 生成 ICO（包含 128x128，避免过大）
    let img128 = img.resize_exact(128, 128, FilterType::Lanczos3);
    img128.save_with_format(out_dir.join("icon.ico"), image::ImageFormat::Ico)
        .expect("保存 ICO 失败");

    // 生成各尺寸 PNG（不含 256）
    for &size in &[16, 32, 48, 64, 128] {
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);
        resized.save_with_format(out_dir.join(format!("icon_{}.png", size)), image::ImageFormat::Png)
            .expect("保存 PNG 失败");
    }

    println!("图标生成完成");
    for entry in std::fs::read_dir(out_dir).unwrap() {
        let e = entry.unwrap();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("icon") {
            let meta = e.metadata().unwrap();
            println!("  {} ({} bytes)", name, meta.len());
        }
    }
}
