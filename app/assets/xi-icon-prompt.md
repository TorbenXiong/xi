# xi 客户端图标

使用内置 `image_gen` 生成，原图为 `xi-icon-source.png`。最终提示词：

```text
Use case: logo-brand
Asset type: application icon for a desktop AI agent workbench named xi
Primary request: a polished modern app icon featuring a single elegant Greek xi-inspired glyph, abstract and geometric, centered and highly legible at 16px and 32px
Style/medium: premium flat vector-like digital icon with subtle depth, crisp clean edges
Composition/framing: centered glyph, square icon, generous padding, no text, no letters other than the abstract xi glyph
Lighting/mood: calm, intelligent, trustworthy
Color palette: deep navy background with electric periwinkle and cyan highlights, small warm violet accent
Constraints: opaque square background, suitable for Windows .ico and web favicon, strong contrast, no gradients that band, no tiny details
Avoid: words, UI screenshots, people, animals, clutter, watermark, photorealism
```

使用仓库已有 Tauri CLI 转换尺寸和封装格式，无需安装新依赖。PowerShell（仓库根目录）：

```powershell
& .\app\node_modules\.bin\tauri.cmd icon .\app\assets\xi-icon-source.png --output .\target\xi-icon-build
```

桌面端采用 `src-tauri/icons/` 中的 PNG、ICO 和 ICNS；页面采用 `app/public/xi-icon.png`（128 × 128）。转换输出目录位于忽略的 `target/`，只将当前桌面和页面需要的资源复制到源码目录。
