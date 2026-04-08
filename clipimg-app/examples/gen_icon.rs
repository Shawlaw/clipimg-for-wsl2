/// Simple icon generator for clipImg
/// Run: cargo run --example gen_icon
use image::{Rgba, RgbaImage, ImageFormat};
use std::path::Path;

fn main() {
    let size = 256u32;
    let mut img = RgbaImage::new(size, size);

    let bg = Rgba([58, 142, 204, 255]);       // Blue
    let white = Rgba([255, 255, 255, 255]);
    let dark_bg = Rgba([40, 105, 165, 255]);
    let accent = Rgba([255, 200, 60, 255]);   // Sun yellow
    let green = Rgba([80, 180, 80, 255]);     // Mountain green

    // Draw rounded rectangle background
    let margin = 16u32;
    let radius = 48u32;
    for y in margin..(size - margin) {
        for x in margin..(size - margin) {
            if in_rounded_rect(x, y, margin, size - margin, radius) {
                img.put_pixel(x, y, bg);
            }
        }
    }

    // Clipboard body
    let cx = 68u32;
    let cy = 72u32;
    let cw = 120u32;
    let ch = 150u32;
    fill_rect(&mut img, cx, cy, cw, ch, white);

    // Clipboard tab (top clip)
    let tx = 98u32;
    let tw = 60u32;
    let th = 24u32;
    fill_rect(&mut img, tx, cy - th, tw, th, white);
    // Tab hole
    fill_rect(&mut img, tx + 16, cy - th + 6, tw - 32, th - 12, bg);

    // Inside clipboard: simple landscape
    let inner_x = cx + 10;
    let inner_y = cy + 10;
    let inner_w = cw - 20;
    let inner_h = ch - 25;

    // Sky (fill inner area with light blue)
    let sky = Rgba([180, 220, 250, 255]);
    fill_rect(&mut img, inner_x, inner_y, inner_w, inner_h, sky);

    // Sun
    let sun_cx = inner_x + inner_w - 25;
    let sun_cy = inner_y + 25;
    let sun_r = 14u32;
    for y in (sun_cy - sun_r)..=(sun_cy + sun_r) {
        for x in (sun_cx - sun_r)..=(sun_cx + sun_r) {
            let dx = x as i32 - sun_cx as i32;
            let dy = y as i32 - sun_cy as i32;
            if dx * dx + dy * dy <= (sun_r as i32).pow(2) {
                if x >= inner_x && x < inner_x + inner_w && y >= inner_y && y < inner_y + inner_h {
                    img.put_pixel(x, y, accent);
                }
            }
        }
    }

    // Mountains (two triangles)
    let base_y = inner_y + inner_h - 10;
    // Left mountain
    let peak_x1 = inner_x + inner_w / 3;
    let peak_y1 = inner_y + 20i32 as u32;
    draw_triangle(&mut img, peak_x1, peak_y1, inner_x + 10, base_y, inner_x + inner_w / 2 - 5, base_y, dark_bg, inner_x, inner_y, inner_w, inner_h);

    // Right mountain (taller)
    let peak_x2 = inner_x + inner_w * 2 / 3;
    let peak_y2 = inner_y + 10;
    draw_triangle(&mut img, peak_x2, peak_y2, inner_x + inner_w / 3, base_y, inner_x + inner_w - 10, base_y, green, inner_x, inner_y, inner_w, inner_h);

    // Ground
    let ground = Rgba([100, 160, 80, 255]);
    fill_rect(&mut img, inner_x, base_y, inner_w, inner_h - (base_y - inner_y), ground);

    // Generate multiple sizes for ICO: 16, 32, 48, 256
    let out_dir = std::env::current_dir().unwrap().join("icons");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Save as ICO (image crate handles multi-size ICO via PNG frames)
    let ico_path = out_dir.join("icon.ico");
    img.save_with_format(&ico_path, ImageFormat::Ico)
        .expect("Failed to save ICO");

    // Also save PNG for reference
    let png_path = out_dir.join("icon.png");
    img.save_with_format(&png_path, ImageFormat::Png)
        .expect("Failed to save PNG");

    // Save smaller sizes
    for &s in &[16u32, 32, 48, 64, 128] {
        let resized = image::imageops::resize(&img, s, s, image::imageops::FilterType::Lanczos3);
        let path = out_dir.join(format!("icon_{}.png", s));
        resized.save_with_format(&path, ImageFormat::Png).unwrap();
    }

    println!("Icon generated in: {}", out_dir.display());
}

fn in_rounded_rect(x: u32, y: u32, min: u32, max: u32, r: u32) -> bool {
    let dx = if x < min + r { min + r - x } else if x > max - r { x - (max - r) } else { return true; };
    let dy = if y < min + r { min + r - y } else if y > max - r { y - (max - r) } else { return true; };
    dx * dx + dy * dy <= r * r
}

fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    for py in y..(y + h) {
        for px in x..(x + w) {
            if px < img.width() && py < img.height() {
                img.put_pixel(px, py, color);
            }
        }
    }
}

fn draw_triangle(
    img: &mut RgbaImage,
    px: u32, py: u32,      // peak
    lx: u32, ly: u32,      // left base
    rx: u32, ry: u32,      // right base
    color: Rgba<u8>,
    clip_x: u32, clip_y: u32, clip_w: u32, clip_h: u32,
) {
    let min_y = py.min(ly).min(ry);
    let max_y = py.max(ly).max(ry);
    for y in min_y..=max_y {
        let mut min_x = u32::MAX;
        let mut max_x = 0u32;
        for &tx in &[px, lx, rx] {
            for &ty in &[py, ly, ry] {
                // Simple scanline approach
            }
        }
        // Use barycentric-ish approach: find x range at this y
        let x_range = scanline_triangle(y, px, py, lx, ly, rx, ry);
        if let Some((x1, x2)) = x_range {
            for x in x1..=x2 {
                if x >= clip_x && x < clip_x + clip_w && y >= clip_y && y < clip_y + clip_h {
                    img.put_pixel(x, y, color);
                }
            }
        }
    }
}

fn scanline_triangle(y: u32, x0: u32, y0: u32, x1: u32, y1: u32, x2: u32, y2: u32) -> Option<(u32, u32)> {
    let mut xs = Vec::new();
    for &(ax, ay, bx, by) in &[(x0, y0, x1, y1), (x1, y1, x2, y2), (x2, y2, x0, y0)] {
        if (ay <= y && by > y) || (by <= y && ay > y) {
            let t = (y as f32 - ay as f32) / (by as f32 - ay as f32);
            let x = ax as f32 + t * (bx as f32 - ax as f32);
            xs.push(x as u32);
        }
    }
    if xs.len() >= 2 {
        xs.sort();
        Some((xs[0], xs[xs.len() - 1]))
    } else {
        None
    }
}
