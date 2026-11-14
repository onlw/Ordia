# Ordia 设计系统规范（`ordia-ui`）

> 对应任务：`M1-01` ｜ 事实来源：[PLAN.md 第 5 节](../PLAN.md#5-设计系统与交互)、[DECISIONS.md D-011](../DECISIONS.md)

## 1. 设计理念与视觉基调

Ordia 的首要用户是个人专业人士（`D-001`），界面风格遵循 **现代、克制、高信噪比** 原则（`D-011`），质感对齐 Linear、Apple Numbers 与 Zed：

1. **内容优先于装饰**：界面外框采用中性灰阶（亮色 Zinc 暖白、暗色深板岩灰），将视觉重心让给表格数据与选区焦点。
2. **品牌强调色节制使用**：采用深青绿（亮色 `#0F766E`、暗色 `#14B8A6`）作为主操作、活动选区边框与焦点环颜色；语义状态色（`success` / `warning` / `danger`）仅用于真实状态表达。
3. **表格画布原色不变性（`PLAN.md` §5.2）**：无论外壳处于亮色还是暗色主题，`sheet_paper_bg` 始终保持纸白 `#FFFFFF`，文档调色板（`spreadsheet_color_swatches()`）保持主题无关的 sRGB 原色，确保用户在 Excel/WPS 中设置的浅色底纹与深色文字在暗色模式下依然 100% 忠实可读（与 Apple Numbers 一致）。
4. **令牌驱动、零裸色值**：业务视图与组件禁止硬编码十六进制颜色或随意像素字号，统一读取 `cx.theme()` 或 `ordia_ui::OrdiaTokens`。

---

## 2. 设计令牌体系（`crates/ordia-ui/src/theme/mod.rs`）

### 2.1 语义颜色令牌（`OrdiaColorTokens`）

| 令牌字段 | 亮色（Light） | 暗色（Dark） | 映射到的 GPUI Kit `SemanticThemeTokens` | 用途 |
|---|---|---|---|---|
| `background` | `#FAFAFB` | `#121316` | `colors.background` | 窗口底色、外壳背景 |
| `surface` | `#FFFFFF` | `#1A1C20` | `colors.surface` | 工具栏、属性面板、弹出层表面 |
| `surface_muted` | `#F4F4F6` | `#22252B` | `colors.secondary` / `colors.muted` | 分段控件底槽、状态栏、公式栏前缀 |
| `foreground` | `#18181B` | `#F4F4F5` | `colors.foreground` | 主界面文字 |
| `muted_foreground` | `#71717A` | `#A1A1AA` | `colors.muted_foreground` | 次要说明、快捷键提示、表头序号 |
| `primary` | `#0F766E` | `#14B8A6` | `colors.primary` | 主按钮、主强调状态 |
| `primary_foreground` | `#FFFFFF` | `#042F2E` | `colors.primary_foreground` | 主按钮文字 |
| `accent_subtle` | `#CCFBF1` | `#14B8A62C` | `colors.accent` | 工具栏激活态底色、悬停高亮 |
| `accent_foreground` | `#115E59` | `#5EEAD4` | `colors.accent_foreground` | 激活态图标与文字 |
| `border` | `#E4E4E7` | `#2C3038` | `colors.border` | 1px 区域分割线 |
| `input_border` | `#D4D4D8` | `#383D47` | `colors.input` | 输入框与下拉触发器描边 |
| `focus_ring` | `#0F766E60` | `#14B8A66E` | `colors.ring` | 键盘导航焦点环 |
| `danger` / `warning` / `success` | `#DC2626` / `#D97706` / `#16A34A` | `#F87171` / `#FBBF24` / `#4ADE80` | `colors.destructive` 等 | 破坏性操作、保留层风险提示、校验通过 |
| `sheet_paper_bg` | `#FFFFFF` | `#FFFFFF` | Ordia 专有令牌 | 表格网格纸面背景（亮暗恒定） |
| `selection_border` | `#0F766E` | `#14B8A6` | Ordia 专有令牌 | 活动单元格选区外框 |

通过 `ordia_ui::apply_ordia_theme(mode, window, cx)` 将上述令牌一次性同步写入 GPUI Kit 全局 `Theme`，自动驱动滚动条、下拉浮层、对话框与焦点环换肤。

### 2.2 圆角、间距、字号、阴影与动效

- **圆角（`RadiusScale`）**：`none = 0px`（单元格网格）、`xs = 4px`（快捷键徽标/色块）、`sm = 6px`（紧凑工具栏控件）、`md = 8px`（下拉菜单/卡片内框）、`lg = 12px`（对话框/命令面板）、`full = 9999px`（胶囊）。嵌套容器遵循同心圆原则 `inner ≈ max(0, outer - gap)`。
- **间距（`SpacingScale`）**：基于 4px 网格：`xxs = 2px`、`xs = 4px`、`sm = 8px`、`md = 12px`、`lg = 16px`、`xl = 24px`。专业工具栏采用紧凑密度（`Size::Small`），同时保留舒适点击热区。
- **字号与字体（`TypographyScale`）**：
  - 界面字体：macOS 使用 `.SystemUIFont`（自动调度 SF Pro + PingFang SC），Windows 使用 `Segoe UI`（回退 Microsoft YaHei UI）。
  - 等宽字体：macOS `SF Mono` / Windows `Consolas`（用于公式栏、单元格地址与性能面板）。
  - 字号阶梯：`caption = 11px`、`sm = 12px`、`base = 13px`（桌面正文默认）、`md = 15px`、`lg = 18px`、`xl = 22px`。
- **层级阴影（`ElevationScale`）**：`sm`（分段滑块/轻卡片）、`md`（下拉菜单/浮层）、`lg`（模态框/命令面板），暗色模式下自动提高阴影不透明度以维持层次边界。
- **动效（`MotionScale`）**：`fast = 120ms`（悬停/选区反馈）、`normal = 160ms`（面板/标签切换）、`gentle = 200ms`（抽屉/弹窗进入），严格控制在 `120–200ms` 区间内，绝不阻塞键盘连续输入。

---

## 3. 语义图标封装（`crates/ordia-ui/src/icons.rs`）

`OrdiaIcon` 枚举对 GPUI Kit 内置 Lucide 矢量图标（`gpui_kit::assets::IconName`）做产品语义层隔离，共收录 27 个核心高频图标（`Undo`、`Redo`、`Save`、`Open`、`Search`、`CommandPalette`、`Bold`、`Italic`、`Underline`、`AlignLeft/Center/Right`、`FillColor`、`TextColor`、`Borders`、`Formula`、`Chart`、`Filter`、`Sort`、`Settings`、`LightMode`、`DarkMode`、`Add`、`Close`、`Inspector`、`Spreadsheet`、`ExportPdf`），并实现 `IconNamed` trait，可直接传给 `Icon::new(OrdiaIcon::Bold)`。

---

## 4. 二次封装组件（`crates/ordia-ui/src/components.rs`）

遵循 GPUI Kit Coding Guides 的 **状态归属清晰（`RenderOnce` vs `Entity<State>`）** 与 **无障碍标签完备** 规范：

1. **`ToolbarButton`**（无状态 `RenderOnce`）：固定 `Size::Small` + `ghost` 基底，支持 `icon`、`label`、`selected(bool)` 激活高亮、`disabled(bool)` 以及 `tooltip(...)`（纯图标按钮自动将 `tooltip` 设为屏幕阅读器 `accessibility_label`）。
2. **`ToolbarSelect` + `new_toolbar_select_state`**（受控 `Entity<ToolbarSelectState>`）：封装紧凑型下拉选择器，支持固定触发器宽度（`with_width`）与独立弹出菜单宽度（`with_menu_width`），避免切换长字体名称时导致工具栏抖动。
3. **`ToolbarColorPicker` + `new_toolbar_color_picker_state`**（受控 `Entity<ColorPickerState>`）：内置 18 色表格标准调色板（中性灰、高饱和强调色、柔和浅底纹色）与 HSLA 滑块，支持语义图标触发器（如油漆桶 `FillColor`、调色盘 `TextColor`）。
4. **`SegmentedControl`**（无状态 `RenderOnce`，基于 `TabBar::segmented().small()`）：用于对齐方式（左/中/右）、亮暗外观、视图模式等互斥单选场景，支持纯文字、纯图标（自动挂载 `aria_label`）及图文混排。

---

## 5. 组件画廊与人工观感验收指南（🧑）

运行以下命令启动交互式设计系统画廊：

```sh
cargo run -p ordia-app -- --gallery
```

人工观感评审清单：
1. **顶栏与交互工具栏区**：点击右上角 `Light / Dark` 分段控件，确认整个窗口外框、卡片、工具栏与图标集即时在亮色与暗色主题间平滑切换；点击 `Bold / Italic / Underline`、字体下拉框、填充/文字颜色选择器及对齐分段控件，检查悬停、激活、禁用与弹出层阴影质感。
2. **亮暗双主题同屏对照卡**：页面中部左右并列展示 `Light Theme Preview` 与 `Dark Theme Preview` 两张实景卡片，无需切换即可同屏对比所有语义色块与模拟工具栏，并确认底部 `B4 · Spreadsheet Paper Canvas` 在亮暗两卡中均保持 `#FFFFFF` 纸白底色。
3. **语义图标矩阵**：页面底部完整渲染 `OrdiaIcon::ALL` 全部 27 枚 Lucide 图标，检查线宽统一性与小尺寸（`Size::Small`）像素清晰度。
