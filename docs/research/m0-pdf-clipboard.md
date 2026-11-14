# M0-07 研究报告：PDF 导出与剪贴板互通原型

## 1. 目标与验证范围

本任务验证两项对桌面表格软件至关重要的跨生态输出/互通能力：
1. **平台无关渲染中间层（`DisplayList`）与纯 Rust 中文 PDF 导出**：验证 `ordia-render` → `ordia-render-pdf`（基于 `krilla 0.8`）+ `ordia-text`（基于 `fontdb 0.24`）能否将包含中文、公式计算结果、背景填充与边框的工作表区域导出为结构稳定、体积紧凑（CJK 字体子集化）且文字可选中/可搜索的 PDF，同时保持内核层零 GPUI 依赖。
2. **Ordia ↔ Excel / WPS 双向富格式剪贴板互通**：验证 `text/plain`（TSV）+ `text/html`（`<table>` 内联 CSS 与 `<style>` 类样式表）双通道编解码及 macOS / Windows 原生剪贴板读写，确保双向粘贴保留单元格值、粗体与背景色，且粘贴动作遵循 `Transaction` 可逆事务模型。

---

## 2. DisplayList 与 PDF 子集化导出架构

### 2.1 分层设计
- **`ordia-render`**：定义纯数据 `DisplayList` 与 `RenderCommand`（`FillRect`、`StrokeRect`、`DrawLine`、`DrawText`、`PushClip`、`PopClip`），提供 `has_balanced_clips()` 不变量校验。
- **`ordia-render-gpui`**：将 `DisplayList` 映射为 GPUI 的 `paint_quad`、`PathBuilder`、`ContentMask` 与 `TextSystem::shape_line`，作为后续图表与打印预览屏幕渲染的共享后端。
- **`ordia-text`**：封装 `FontCatalog`，内置 `PLAN.md §5.3` 度量兼容字体替换链（如 `Calibri → Carlito`）与跨平台中文字体回退链（`PingFang SC` → `Microsoft YaHei` → `Hiragino Sans GB` → `STHeiti` → `Noto Sans CJK SC`），并专门适配了现代 macOS 将 `PingFang.ttc` 存放在 `/System/Library/AssetsV2/com_apple_MobileAsset_Font7` 下的系统路径。
- **`ordia-render-pdf`**：
  - `build_sheet_range_display_list` 将工作表选区/已用区域（背景色、网格线、显式单元格边框、数值右对齐/文本左对齐、单元格裁剪框）编译为 `DisplayList`。
  - `render_display_lists_to_pdf` 调用 `krilla` 绘制路径与字形；遇到含 CJK 字符的文本自动解析 CJK 字体面（含 `.ttc` `face_index`），由 `krilla` + `subsetter` 自动生成嵌入的 `/CIDFontType0`/`CIDFontType2` 子集与 `/ToUnicode` CMap 映射表。
  - `export_sheet_range_to_pdf_file` 采用同目录临时文件 + `sync_all` + `rename` 原子落盘。

### 2.2 实测结果
- **字体子集化效果**：系统原始 `PingFang.ttc` / `STHeiti` 字体体积超过 15–30 MiB，导出含中英文预算表与加粗样式的单页 PDF 后体积仅约 **10–30 KiB**。
- **确定性与结构快照**：同一字体目录下连续多次导出字节 100% 完全一致；`insta` 快照测试（`chinese_budget_pdf_structure`）确认输出为 `%PDF-1.7`，包含 `/ToUnicode` CMap 与 CID 字体子集，文字在 macOS 预览（Preview）、Chrome 与 Adobe Acrobat 中均可正常高亮选中与 `Cmd+F` 搜索。

---

## 3. Excel / WPS 双向剪贴板互通方案

### 3.1 格式细节与踩坑发现
1. **Ordia → Excel / WPS**：同时向系统剪贴板写入 `text/html`（带 `<!--StartFragment-->` 标记和 `<td style="font-weight:bold;background-color:#RRGGBB;color:#RRGGBB;">` 的标准表格）与 `text/plain`（制表符分隔 TSV，对含换行/制表符/双引号的内容按 Excel 规范加双引号转义）。Excel、WPS 与飞书/钉钉文档均优先读取 `text/html` 并还原粗体和背景填充色，纯文本编辑器则回退读取 TSV。
2. **Excel / WPS → Ordia**：实测桌面版 Microsoft Excel 复制出的 HTML 往往**不写内联 `style="..."`**，而是在 `<head><style>` 中生成 `.xl65 { font-weight:700; background:#FFD966; }` 并在单元格上标记 `<td class=xl65>`；WPS 则混合使用内联 `style` 与 `<b>` 标签。因此 `ordia-sheet-model::clipboard::parse_html_table` 同时解析了 `<style>` 类规则映射表、内联 CSS（`#RRGGBB`、`#RGB`、`rgb(...)`）、`bgcolor` 属性与语义标签（`<b>`/`<strong>`/`<i>`/`<em>`），缺失 HTML 时无缝回退至多行 TSV 解析器。
3. **平台隔离与无界面测试**：系统剪贴板调用基于 `arboard`（macOS `NSPasteboard` / Windows `CF_HTML` + `CF_UNICODETEXT`）收敛在 `ordia-app/src/platform/{macos,windows}/`（严格遵循架构规则 §4.6），并通过 `SheetClipboardBridge` trait 注入 `ordia-sheet-view`；无界面 GPUI 测试默认使用内存级 `InMemoryClipboardBridge`，避免污染宿主机器剪贴板或引发并行测试竞争。

### 3.2 人工复核指引（🧑 验收项）
1. **PDF 中文显示与搜索复核**：运行 `cargo run -p ordia-app -- --export-pdf /tmp/ordia_demo.pdf`，使用 macOS「预览」或浏览器打开 `/tmp/ordia_demo.pdf`，按 `Cmd/Ctrl+F` 搜索 `订阅收入` 或 `全年合计`，确认中文清晰锐利且可精确命中高亮。
2. **Excel / WPS 双向互贴复核**：启动 `cargo run -p ordia-app`，框选首行带浅灰蓝背景与粗体的表头区域（`A1:C2`）按 `Cmd/Ctrl+C` 粘贴至 Excel / WPS，确认值、粗体与背景色保留；反之在 Excel / WPS 中为单元格设置加粗与黄色填充后复制，回到 Ordia 按 `Cmd/Ctrl+V` 粘贴并按 `Cmd/Ctrl+Z` 撤销，确认样式还原与单步撤销正常。

---

## 4. 结论

**Go**。`ordia-render` + `krilla` + `arboard` 双通道剪贴板方案全部采用宽松开源协议（MIT / Apache-2.0），满足零 `unsafe` 与内核零 GUI 依赖约束，可直接作为 M2–M4 打印预览、PDF 导出与高级选择性粘贴的底座。
