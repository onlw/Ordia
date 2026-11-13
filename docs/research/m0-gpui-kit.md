# M0-02 技术验证报告：GPUI Kit 主窗口与组件选型

## 1. 版本与依赖锁定情况

| 项目 | 版本 / 配置 | 说明 |
|---|---|---|
| `gpui-kit` | `=0.9.1` | 唯一 UI 伞包依赖，统一导出 `gpui`、`gpui_kit::component`、`gpui_kit::base`、`gpui_kit::assets` |
| 底层 GPUI 快照 | `gpui-pre =0.3.12` | 由 `gpui-kit` 内部精确锁定，避免上游快照 API 漂移 |
| Rust 工具链 | `1.100.0`（edition 2024） | `gpui-pre =0.3.12` 使用了 `std::cfg_select!` 与 `std::hint::cold_path()`，需 Rust 1.96+ 稳定版，固定为 `1.100.0` |
| 图标资源 | `gpui_kit::assets::AllAssets` | 嵌入完整 Lucide 矢量图标集，支持按 `rem` 字号和主题色动态着色 |
| UI 集成测试 | `gpui-kit` `test-support` feature | 使用 `#[gpui_kit::test]` + `TestWindowExt` 在无界面窗口中驱动真实指针与键盘事件 |

## 2. Ordia 可直接复用的 GPUI Kit 组件清单

| 办公套件场景 | GPUI Kit 组件 | 复用方式与评估 |
|---|---|---|
| 自定义标题栏与窗口装饰 | `TitleBar` (`TitleBar::window_options()`) | 原生支持 macOS 红绿灯留白与 Windows 自绘最小化/最大化/关闭按钮，拖拽区域处理完善 |
| 工作簿文档标签栏 | `TabBar` (`TabVariant::Folder`) + `Tab` | 浏览器式圆角文件夹标签（带溢出下拉菜单 `menu(true)` 与滑块动画），契合多工作簿窗口 |
| 底部工作表切换栏 | `TabBar` (`TabVariant::Underline` / `Pill`) | 支持横向滚动追踪 `track_scroll`、前缀/后缀按钮（新建 Sheet `+`） |
| 左侧导航与可折叠面板 | `Sidebar`、`SidebarCollapsible::Icon`、`SidebarToggleButton` | 支持展开与仅图标折叠两种模式，折叠时自动移出隐藏项焦点 |
| 右侧上下文属性面板 / 分栏 | `resizable::{h_resizable, v_resizable, resizable_panel}`、`Sheet` | 支持拖拽调宽、最小/最大尺寸约束与状态保持 |
| 单元格编辑框与公式栏 | `input::{Input, InputState, InputEvent}` | 基于 GPUI `EntityInputHandler`，原生接入 macOS / Windows 系统 IME 候选框定位与选区操作 |
| 命令面板（`Cmd/Ctrl+K`） | `command::{Command, CommandState, CommandGroup}` 或 `Dialog` + `Input` + `List` | 内置键盘上下导航、分组展示与快捷键提示 `Kbd` |
| 下拉框（字体、字号、数字格式、数据验证） | `select::{Select, SelectState}`、`combobox::{Combobox, ComboboxState}` | 字体选择器可用带搜索的 `Combobox`；固定枚举用 `Select` |
| 颜色选择器（字体色、填充色、标签色） | `color_picker::{ColorPicker, ColorPickerState}` | 支持预设色板与自定义取色，可直接封装进工具栏按钮 |
| 菜单与右键上下文菜单 | `menu::{PopupMenu, DropdownMenu}`、`DropdownButton` | 完整支持多级子菜单、快捷键展示、禁用态、焦点恢复与 Esc 关闭 |
| 对话框与破坏性确认 | `dialog::Dialog`、`AlertDialog` (`WindowExt`) | 由窗口顶层 `Root` 统一托管遮罩层与焦点陷阱（Focus Trap） |
| 异步状态通知 | `notification::Notification` (`window.push_notification`) | 保存提示、恢复通知、导出完成提示 |
| 虚拟列表（筛选值列表、历史版本、查找结果） | `v_virtual_list`、`list::{List, ListState}` | 十万级筛选唯一值列表仍保持流畅滚动 |
| 设置界面 | `setting::Settings`、`form::{Form, Field}`、`Switch`、`RadioGroup` | 快速构建结构化偏好设置页 |
| 基础图表参考 | `chart::{BarChart, LineChart, PieChart, AreaChart}` | 可用于应用统计页；**但表格文档内嵌图表需走 `ordia-chart -> DisplayList`**，以保证屏幕显示与 PDF 导出完全一致 |
| 数据表格（非电子表格区域） | `table::{DataTable, TableState}` | 适合 CSV 导入向导预览、名称管理器列表、快捷键表；**不用于主电子表格网格**（不支持跨格合并、四象限冻结窗格、公式引用彩色选区） |

## 3. 架构与工程注意事项（实测记录）

1. **必须通过 `gpui_kit::open_window` 打开窗口**：它会自动挂载 `Root` 视图，统一管理 `Dialog`、`Sheet`、`Notification`、`Tooltip`、`PopupMenu` 图层以及 `window.set_rem_size(cx.theme().font_size)`。业务视图严禁再手动挂载遮罩层。
2. **主题更新必须走 `Theme::change` 或 `Theme::update`**：`Theme` 同时维护纯色 `colors`、可渐变背景 `tokens` 以及 `gpui-base` 的滚动条/分割线投影；直接改字段会导致局部颜色不同步。实测 `Theme::change(next_mode, Some(window), cx)` 可零闪烁即时刷新整窗。
3. **快捷键必须在 `cx.set_menus` 之前绑定**：`set_menus` 在调用瞬间读取当前 keymap 生成原生菜单快捷键标记，顺序颠倒会导致菜单栏不显示快捷键。
4. **`h_flex()` 交叉轴默认居中**：包含侧边栏和主工作区的横向容器必须显式加 `.items_stretch()`，且可伸缩子容器必须加 `.min_w_0()` / `.min_h_0()`，否则长标题或大画布会撑破 Flex 最小尺寸或裁切顶部内容。
5. **构建耗时与磁盘空间控制**：在根 `Cargo.toml` 设置 `[profile.dev] debug = 0` 与 `[profile.test] debug = 0` 后，极大缩短了 macOS Metal 与 GPUI 依赖树的链接耗时，并将 `target/` 体积控制在数 GB 以内。

## 4. 结论

**通过（Go）**。GPUI Kit `0.9.1` 提供了成熟的桌面级窗口外壳、表单、菜单、浮层、输入与无界面 UI 集成测试能力，可省去数月基础设施工作；电子表格主网格与文档内嵌图表按既定方案通过自定义 `Element` + `DisplayList` 实现。
