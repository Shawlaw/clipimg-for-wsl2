# assets/ — UI 资源源文件

此目录存放设计源文件，不会被打包进程序。用于后续调整和替换。

## 文件说明

| 文件 | 用途 |
|------|------|
| `icon_source.png` | 应用图标原始设计稿（1024x1024），所有尺寸从此图生成 |
| `icon_raw.png` | 图标设计过程中的草稿/备用版本 |

## 如何更新图标

1. 将新的 1024x1024 图标替换 `icon_source.png`
2. 在 `clipimg-app/` 目录下运行生成工具：

```bash
# 生成 ICO + 各尺寸 PNG（需要 Pillow）
python3 -c "
from PIL import Image
img = Image.open('assets/icon_source.png').convert('RGBA')
sizes = [(16,16),(32,32),(48,48),(64,64),(128,128),(256,256)]
ico = img.resize((256,256), Image.LANCZOS)
ico.save('icons/icon.ico', format='ICO', sizes=sizes)
for s,_ in sizes:
    img.resize((s,s), Image.LANCZOS).save(f'icons/icon_{s}.png')
print('done')
"

# 或使用 Rust 版工具
cargo run --example convert_icon
```

3. 重新编译 EXE：

```bash
cargo xwin build --target x86_64-pc-windows-msvc --release
```

## 图标在程序中的使用

| 用途 | 文件 | 说明 |
|------|------|------|
| EXE 图标 + 属性面板 | `icons/icon.ico` | 通过 `build.rs` 嵌入 EXE 资源 |
| 系统托盘图标 | `icons/icon_32.png` | 通过 `include_bytes!` 编译时嵌入 |
| README 展示 | `icons/icon_256.png` | Markdown 中引用 |
