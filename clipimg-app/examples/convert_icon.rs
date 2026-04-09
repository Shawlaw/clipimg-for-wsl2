/// 从 assets/icon_source.png 生成各尺寸图标到 icons/
/// Run: cargo run --example convert_icon
use image::imageops::FilterType;
use std::path::Path;

fn main() {
    let src = "assets/icon_source.png";
    let img = image::open(src).expect("无法打开 assets/icon_source.png");

    let out_dir = Path::new("icons");

    // 生成 ICO（包含 256x256，PNG-based）
    let img256 = img.resize_exact(256, 256, FilterType::Lanczos3);
    img256.save_with_format(out_dir.join("icon.ico"), image::ImageFormat::Ico)
        .expect("保存 ICO 失败");

    // 生成各尺寸 PNG
    for &size in &[16, 32, 48, 64, 128, 256] {
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
