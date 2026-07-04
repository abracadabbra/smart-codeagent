# Tauri Icons

Tauri 在 `tauri.conf.json` 的 `bundle.icon` 字段引用以下图标。Phase 1 占位用
最小合法 PNG（1×1 透明像素），后续 Phase 4 替换为正式图标。

> 这些 PNG 是合法的最小占位文件，**不是可显示的品牌图标**。

需要以下文件名（与 `tauri.conf.json` 对应）：
- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- `icon.icns` (macOS) — 需 `iconutil` 由 PNG 生成
- `icon.ico` (Windows) — 需 `convert` (ImageMagick) 由 PNG 生成

Phase 1 临时方案：先用 `32x32.png` / `128x128.png` / `128x128@2x.png` 占位，
`tauri.conf.json` 中只保留这 3 项（不需要 .icns / .ico 来跑 `cargo tauri dev`）。