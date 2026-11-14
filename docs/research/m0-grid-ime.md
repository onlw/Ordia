# M0-05 实测报告：百万行网格虚拟化渲染与中文输入法（IME）原型

- **日期**：2026-11-14
- **测试平台**：macOS（Apple Silicon `aarch64-apple-darwin`，Metal GPU 渲染后端）
- **构建配置**：`--release`（`lto = "thin"`, `codegen-units = 1`）
- **复现命令**：
  - 交互运行：`cargo run --release -p ordia-app -- --open corpus/generated/1m.xlsx`
  - 自动滚动基准：`cargo run --release -p ordia-app -- --open corpus/generated/1m.xlsx --bench-scroll 200`
  - 无界面 UI 测试：`cargo test -p ordia-sheet-view`

---

## 1. 架构与核心实现

本次原型在三个分层 crate 中建立了无泄露、可复用的表格视图管线：

1. **`ordia-sheet-layout`（纯内核几何与虚拟化索引，零 GPUI 依赖）**
   - `GridAxisIndex`：为 `1,048,576` 行与 `16,384` 列构建 `f64` 累积偏移前缀和数组（内存约 `8.4 MB`），提供严格单调的 `O(log N)` 二分坐标反查（`offset_to_index` 自动跳过 `height == 0` 的隐藏行列）与 `O(1)` 区间跨度读取。
   - `ViewportGeometry`：将画布划分为四个独立裁剪象限（左上冻结角 `top_left_pane`、顶部冻结行 `top_pane`、左侧冻结列 `left_pane`、主滚动区 `main_pane`），统一处理滚动边界钳制、命中测试（`hit_test`）与 `scroll_into_view`。
2. **`ordia-sheet-model`（文档状态与事务驱动）**
   - `SheetDocument`：封装 `Box<dyn SheetEngine>`、活动工作表/选区状态及基于 `SheetOp` / `Transaction` 的撤销重做栈；针对 `> 8 MiB` 的超大工作簿（如 `1m.xlsx`）采用 `from_xlsx_bytes_unrecalculated` 直接读取缓存值 `<v>` 首屏秒开，避免全量重算阻塞首帧。
3. **`ordia-sheet-view`（自定义 GPUI `Element` + 编辑框 + IME 桥接 + 帧时间 HUD）**
   - `SheetGridElement`：实现 GPUI `Element` 三阶段（`request_layout` → `prepaint` → `paint`），按四象限绘制背景色、选区高亮、行列网格线、双代缓存（`ShapedLineCache`）排版文字、行号列标、冻结分隔线与极简比例滚动条。
   - **内联编辑与公式栏双向同步**：双击、`F2`、`Enter` 或直接输入激活定位于目标单元格精确像素边界的 GPUI Kit `Input`（`cell-inline-editor`），与顶部公式栏（`formula-bar-input`）实时同步；通过 `OrdiaSheetGrid && !Input` 上下文谓词隔离网格导航键与编辑框光标键，并监听传播动作 `input::Enter` / `input::Escape` 实现零延迟提交或取消。
   - **OS 输入法（IME）无缝桥接**：在网格非编辑聚焦态下，通过 `window.handle_input(&focus_handle, ElementInputHandler::new(active_cell_bounds, view_entity), cx)` 将活动单元格的窗口坐标暴露给操作系统的输入法协议（macOS `NSTextInputClient::firstRectForCharacterRange` / Windows TSF `ITfContextView::GetTextExt`）。用户在选中格上敲下第一个拼音字母触发 `replace_and_mark_text_in_range` 时，立即原地切换进 `InputState` 预编辑态，候选词浮窗从首帧起即紧贴目标单元格下方。

---

## 2. 100 万行 × 20 列（`1m.xlsx`）滚动帧时间实测与调优演进

在 `corpus/generated/1m.xlsx`（**2,000 万非空单元格、200 万公式，1,200 × 780 窗口，单屏可见约 360 格**）上执行 200 帧连续 GPU 渲染实测，记录了从初版实现到生产达标的完整调优链路：

| 阶段 | 场景说明 | 采样帧数 | 平均帧耗时 (`Avg`) | 中位数 (`P50`) | `P95` | **`P99`（目标 `< 8 ms`）** | 最大值 (`Max`) | 瓶颈定位与修复手段 |
|---|---|---:|---:|---:|---:|---:|---:|---|
| 初版基线 | 全表拖拽滚动 | 2 | 435.43 ms | 423.94 ms | 423.94 ms | 423.94 ms | 446.93 ms | 发现每帧调用 `engine.sheets()` 触发 `ws.dimension()` 遍历 2,000 万格；改为 `SheetDocument` 缓存 `O(1)` 读取 |
| 第二轮（`O(1)` 元数据） | 全表冷缓存跳滚 | 150 | 8.24 ms | 8.14 ms | 10.69 ms | 11.74 ms | 13.14 ms | 发现两处次生开销：① 每格独立 push/pop `ContentMask` 打断 GPU 图元合批；② 单桶 `ShapedLineCache` 满 4,096 条整表清空造成周期抖动 |
| **最终版（批次合并 + 双代缓存）** | **高速触控板惯性滚动（`FLICK`，50 万行处 `+150 px/帧`）** | **200** | **3.40 ms** | **3.48 ms** | **4.49 ms** | **7.00 ms** ✅ | **8.03 ms** | **约 294 FPS，满足 `120 Hz` ProMotion 预算（`8.33 ms`）** |
| **最终版（批次合并 + 双代缓存）** | **极端全表滚动条拖拽（`SCRUB`，0 → 100 万行，`+4,900 行/帧` 100% 冷整形）** | **200** | **5.61 ms** | **5.57 ms** | **7.02 ms** | **7.96 ms** ✅ | **8.98 ms** | **即使每帧跨越 4,900 行全冷整形，`P99` 仍压进 `< 8.0 ms` 红线内** |

### 关键性能洞察

1. **禁止在 `paint` 路径触发任何与全表规模线性相关的元数据查询**：哪怕是一次看似轻量的 `used_range` 查询，在 2,000 万格规模下也会放大成百毫秒级卡顿；缓存到文档层并按事务增量扩展边界后开销降为 `0 ns`。
2. **GPU 裁剪遮罩（`ContentMask`）按需下发**：95% 以上未超宽单元格由所属象限（`PaneRegion`）的外层遮罩统一裁剪，仅当 `shaped.width > inner_width` 时才下发单元格级遮罩，使整个象限文字合并为单次 Metal 字形批次提交，帧耗时立降 **32%~58%**。
3. **双代老化文本整形缓存（`current` + `previous` 各 4,096 条）**：消除了固定容量清空导致的周期性冷启动尖峰，同时在 `window.rem_size()` 缩放变化时自动失效重建。

---

## 3. 中文输入法（IME）与内联编辑验证

### 3.1 自动化无界面验证（`crates/ordia-sheet-view/tests/ui.rs`）

1. **`inline_formula_editing_escape_cancel_and_undo_redo`**：
   - 验证双击 `A1`/`B1` 输入数值、`C1` 输入 `=A1+B1` 回车后引擎实时算出 `42`；
   - 验证编辑中途按 `Escape` 安全丢弃未提交文本、保留原公式与计算值；
   - 验证 `Cmd/Ctrl+Z` 与 `Cmd/Ctrl+Shift+Z` 走 `Transaction` 完整撤销与重做。
2. **`chinese_ime_composition_tracks_cell_bounds_and_commits_cleanly`**：
   - 验证单击选中 `B3`（未双击进入编辑框）直接发起 `compose_text("ni hao")` 预编辑时，`marked_text_range` 为 `Some(0..6)`，内联编辑框与公式栏同步显示预编辑串；
   - 调用 `EntityInputHandler::bounds_for_range` 校验返回的候选窗锚点矩形严格落在 `B3` 窗口像素边界内；
   - 调用 `commit_text("你好，世界")` 并回车，验证中文准确写入模型且焦点自动下移至 `B4`。
3. **`million_row_virtualized_scrolling_keeps_frozen_panes_pinned`**：
   - 验证滚轮与跳转至第 `1,000,000` 行附近时，性能叠加层正常更新且点击左上角冻结区仍精确命中 `A1`。

### 3.2 跨平台系统输入法手工复核指引（🧑 验收项）

- **macOS 自带简体拼音**：运行 `cargo run --release -p ordia-app`，单击任意单元格直接敲击 `zhongwen`，确认系统拼音候选条紧贴该单元格下沿（而非窗口左上角/左下角），按空格或数字键上屏正常，回车后单元格显示中文。
- **Windows 微软拼音 & 搜狗输入法**：在 Windows 10/11 上运行同一命令，得益于 GPUI `ElementInputHandler` 对 Windows TSF `GetTextExt` 的统一坐标上报与 `InputState` UTF-16 选区换算，验证预编辑下划线与候选框跟随单元格位置且不吞首字母。

---

## 4. Go / No-Go 结论与后续演进建议

- **结论：Go（全面达标）**。百万行惯性滚动 `P99 = 7.00 ms`、极端全表冷跳滚 `P99 = 7.96 ms`，双双优于 `PLAN.md` §1.2 规定的 `P99 < 8 ms` 门槛。
- **M2 进一步优化方向**：
  - 在 `M2-02` 引入 `64 行 × 16 列` 平台无关 `DisplayList` 瓦片缓存，将可见格的 `cell_display_text` 与 `cell_style` 读取也纳入瓦片级缓存，预计可将惯性滚动 `P99` 进一步压低至 **< 1.5 ms**；
  - 在 `M1-06` 将 `open_xlsx` 移入后台异步任务并配进度条，消除打开 194 MB 文件时的窗口等待。
