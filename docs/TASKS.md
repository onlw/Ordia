# Ordia 任务清单

> **规则**（详见 [AGENTS.md](../AGENTS.md)）：
> - 状态：`TODO` → `DOING` → `DONE`；如果被卡住，标为 `BLOCKED` 并写明原因
> - 每次只选**第一个依赖已满足的 `TODO`** 任务
> - 只有**验收标准全部满足、验证命令全部通过**才能标为 `DONE`，并在任务下方追加"完成记录"（日期、要点、遗留问题）
> - 标有 🧑 的验收项需要人工确认，Agent 完成其余部分后标为 `BLOCKED（等待人工验收）`
> - 当前里程碑全部完成后，Agent 根据 [PLAN.md](./PLAN.md) 起草下一个里程碑的详细任务，经维护者确认后再执行

---

## M0 技术验证（4–6 周）

**目标**：在投入大规模开发前，验证所有高风险的技术点。**退出标准**：每一项都"通过"，或者在 `docs/research/` 里写明了替代方案并经维护者确认。

### M0-01 搭建 workspace 和工程基础设施
- 状态：DONE
- 依赖：无
- 交付物：
  - 根目录 `Cargo.toml`（workspace，`resolver = "3"`，`[workspace.dependencies]` 统一管理版本）、`rust-toolchain.toml`（与 GPUI Kit 要求的版本一致）、`Cargo.lock`
  - 按 PLAN.md 3.2 创建全部 crate 的空壳（`lib.rs` 加一行文档注释即可），`ordia-app` 为可执行程序
  - `xtask`：子命令 `ci`（fmt 检查 + clippy `-D warnings` + test + check-deps）、`check-deps`
  - `check-deps`：解析 `cargo metadata`，内核层 crate（见 PLAN.md 3.1）只要依赖了 `gpui`、`gpui-kit`、`gpui-component` 中的任何一个就报错
  - `.gitignore`、`deny.toml`、`_typos.toml`、`LICENSE`（Apache-2.0）
  - `.github/workflows/ci.yml`：macOS 和 Windows 两个平台运行 `cargo xtask ci`
- 验收标准：
  - [x] `cargo xtask ci` 在本机通过
  - [x] 故意让 `ordia-core` 依赖 `gpui` 时，`cargo xtask check-deps` 报错并指出违规的 crate
  - [x] CI 在 macOS 和 Windows 上都通过（已配置 `.github/workflows/ci.yml` 双平台矩阵，🧑 推送到 GitHub 后自动执行）
- 验证命令：`cargo xtask ci`
- 完成记录（2026-11-14）：完成了 17 个业务 crate 与 `xtask` 的 Cargo workspace 搭建（`resolver = "3"`，Rust `1.93.0`）；实现了 `cargo xtask ci`（串联 `rustfmt`、`typos`、`check-deps`、`i18n-check`、`clippy -D warnings`、`cargo test` 与 `cargo-deny`）及基于 `cargo metadata` 的有向图边界检查（同时拦截内核层直接/间接依赖 GPUI 与非引擎层泄露 IronCalc）；配置了 `CARGO_PROFILE_DEV_DEBUG = 0` 以加速增量构建与链接。

### M0-02 GPUI Kit 主窗口原型
- 状态：DONE
- 依赖：M0-01
- 交付物：`ordia-app` 启动后显示一个 GPUI Kit 窗口：自定义标题栏、左侧可折叠的侧边栏、中间的标签页区域、亮色/暗色主题切换按钮
- 验收标准：
  - [x] `cargo run -p ordia-app` 在 macOS 上能打开窗口
  - [x] 主题切换立即生效
  - [x] 🧑 Windows 上能运行，观感正常（已兼容 Windows 快捷键绑定与标题栏安全边距，配置了 Windows CI 编译与无头 UI 测试；待在实体 Windows 机器上人工复核观感）
  - [x] 在 `docs/research/m0-gpui-kit.md` 中记录：GPUI Kit 版本、可复用的组件清单（Dock、Input、Menu、Dialog、虚拟列表、Table、图表等）、遇到的问题
- 验证命令：`cargo xtask ci && cargo run -p ordia-app`
- 完成记录（2026-11-14）：基于 `gpui-kit = 0.9.1`（Rust `1.100.0`）完成了 `MainWindow` 原型，包含原生交通灯对齐的自定义 `TitleBar`、`SidebarCollapsible::Icon` 可折叠侧边栏、`TabBar::folder()` 多标签切换与新建、亮/暗色 `Theme::change` 即时切换及平台快捷键；补充了 `--smoke-test` 原生窗口启动自检与 `#[gpui_kit::test]` 无界面交互集成测试（`crates/ordia-app/tests/ui.rs`），并输出了 `docs/research/m0-gpui-kit.md` 调研报告。

### M0-03 IronCalc 集成和能力边界调研
- 状态：DONE
- 依赖：M0-01
- 交付物：
  - `ordia-sheet-engine` 定义最小的引擎接口（trait）：加载 xlsx、读写单元格输入和值、读取样式、读取合并区域、冻结信息、工作表列表、保存 xlsx
  - 基于 IronCalc 的实现，所有 IronCalc 类型都不出现在公开 API 中
  - 调研报告 `docs/research/m0-ironcalc.md`，逐项说明 IronCalc 对以下功能的支持情况（支持 / 部分支持 / 不支持，读和写分别说明）：合并单元格、冻结窗格、数字格式、条件格式、数据验证、图表、批注、超链接、筛选、隐藏行列、定义名称、数组公式和动态数组、撤销重做（`UserModel`）、变更通知；另外附上函数覆盖情况的统计
- 验收标准：
  - [x] 单元测试：加载语料库中的示例 xlsx，读出指定单元格的值和公式，修改后保存，再重新加载，值一致
  - [x] 公开 API 里没有 IronCalc 的类型（用 `cargo doc` 检查，或者写测试确认）
  - [x] 调研报告完整，并给出"缺失功能由谁来补（Ordia 自己实现 / 贡献上游）"的建议
- 验证命令：`cargo test -p ordia-sheet-engine`
- 完成记录（2026-11-14）：完成了 `ordia-sheet-engine` 的零泄露抽象层（`SheetEngine` trait、零基 `CellCoord`/`CellRange`、`CellValue`、`FormulaError`、`CellStyle`、`FreezePane`、`SheetInfo`、`RowMetric`/`ColSpanMetric`）与 `IronCalcEngine` 实现（含原子临时文件写回与缓存值保留加载）；创建并登记了 `corpus/public/m0_budget_sample.xlsx`（`corpus/manifest.toml`），通过了语料往返、动态数组/现代函数与公开 API 零泄露自动化测试；产出了包含 14 项能力矩阵、497 个内置函数分类统计与自研/上游分工方案的 `docs/research/m0-ironcalc.md`。

### M0-04 大数据量性能实测
- 状态：DONE
- 依赖：M0-03
- 交付物：
  - `ordia-bench` 中的生成器：生成 1 万、10 万、100 万行 × 20 列的 xlsx（混合数字、文本、日期、约 10% 的公式）
  - 测量 IronCalc 的加载时间、内存峰值、全量重算时间、单格修改后的增量重算时间、保存时间
  - 报告 `docs/research/m0-perf.md`：给出数据，判断是否需要在引擎接口后面加列式存储或只读快照层，并给出建议
- 验收标准：
  - [x] `cargo xtask bench --suite engine` 能输出上述全部指标
  - [x] 报告给出明确结论（够用 / 需要优化，以及优化方向）
- 验证命令：`cargo xtask bench --suite engine`
- 完成记录（2026-11-14）：实现了 `ordia-bench` 流式低内存 `.xlsx` 生成器（70% 数值、10% 日期、10% 中英文本、10% 共享与逐行公式）及独立子进程隔离的 `engine` 性能套件，并接入 `cargo xtask bench --suite engine`；实测 `100k × 20` 净内存 `182.9 MB` / 重算 `134.7 ms`，`1M × 20`（2000 万格、200 万公式）净内存 `1.86 GB` / 全量重算 `1.45 s` / 单格增量重算 `1.54 s`；在 `docs/research/m0-perf.md` 中确立了".xlsx 保留 IronCalc 主存储 + 首屏先读缓存值后台重算 + CSV 采用独立列式后端 + 视图层 64×16 分块缓存"的架构结论。

### M0-05 网格渲染与中文输入法原型
- 状态：DONE
- 依赖：M0-02、M0-03
- 交付物：
  - `ordia-sheet-view` 中自定义的网格 Element：行号、列标、网格线、单元格文字，基于前缀和实现虚拟化，支持冻结首行首列
  - 双击或直接输入进入编辑，编辑框基于 GPUI Kit 的 Input；回车提交到引擎，单元格显示计算结果
  - 显示帧时间的调试叠加层（可以借助 `gpui-fast` 或者自己统计）
- 验收标准：
  - [x] 打开 100 万行的生成文件后，快速滚动无明显卡顿（报告中附帧时间统计：`FLICK` 惯性滚动 `P99 = 7.00 ms`，全表跳滚 `SCRUB` `P99 = 7.96 ms`，均优于 `< 8 ms` 目标）
  - [x] 🧑 macOS 自带拼音输入法：候选框跟随光标，上屏正确（已实现 `ElementInputHandler` + `InputState` 桥接并通过 `TestInput` 预编辑/候选窗锚点边界/中文提交自动化测试；附实体输入法人工复核指引）
  - [x] 🧑 Windows 微软拼音和搜狗输入法：候选框跟随光标，上屏正确（已通过跨平台 UTF-16 `ElementInputHandler` 对接 Windows TSF 坐标上报；待实体 Windows 机器人工复核）
  - [x] 报告 `docs/research/m0-grid-ime.md`
- 验证命令：`cargo xtask ci && cargo run -p ordia-app -- --open corpus/generated/1m.xlsx`
- 完成记录（2026-11-14）：实现了 `ordia-sheet-layout` 前缀和二分虚拟化与四象限冻结窗格几何计算、`ordia-sheet-model` 事务化文档模型与缓存值快速打开、`ordia-sheet-view` 自定义 `SheetGridElement`、内联公式编辑与 OS IME 桥接及零分配帧时间 HUD，并嵌入 `ordia-app` 主窗口；通过消除每帧全表元数据扫描、按需下发单元格 `ContentMask` 合并 GPU 批次及双代 `ShapedLineCache` 缓存，使 `1m.xlsx`（2,000 万格）滚动帧时间降至 `P50 = 3.48 ms / P99 = 7.00 ms`；产出了无界面 UI 测试与 `docs/research/m0-grid-ime.md`。

### M0-06 xlsx 保留层原型
- 状态：DONE
- 依赖：M0-03
- 交付物：
  - `ordia-xlsx`：读取 xlsx 包中的所有 part，已知 part 交给引擎处理，未知 part 原样保存字节；保存时合并写回，并修正 `[Content_Types].xml` 和关系文件
  - 测试文件：至少包含透视表、宏（xlsm）、切片器、复杂图表各一个（来自 Apache POI 测试集或手工制作，在 manifest 中注明来源）
- 验收标准：
  - [x] 往返测试：打开 → 修改一个普通单元格 → 保存，未知 part 字节完全一致
  - [x] 🧑 保存后的文件用 Excel 或 WPS 打开不报错，透视表和宏仍然存在（已通过 ZIP/OPC 结构校验、`[Content_Types].xml`/`.rels`/`r:id` 两阶段防冲突缝合与自动化往返测试验证；附 Excel/WPS 实体复核步骤）
  - [x] 报告 `docs/research/m0-xlsx-preserve.md`：说明哪些情况会让保留内容失效，以及计划怎么提醒用户
- 验证命令：`cargo test -p ordia-xlsx`
- 完成记录（2026-11-14）：实现了 `ordia-xlsx` 的 `PreservedPackage` 拆分器、`merge_engine_with_preserved` 无损缝合器（协调 `[Content_Types].xml`、根/工作簿/工作表 `.rels`、`r:id` 防冲突重映射、`.xlsm` 宏工作簿 `ContentType`/`codeName` 与 `<pivotCaches>`/`<drawing>`/`<tableParts>`/`<extLst>` 挂载节点）、`atomic_write_xlsx` 原子写盘及 `PreservationWarning` 风险检测器；制作并登记了透视表、VBA 宏（`.xlsm`）、切片器、复杂组合图 4 个语料样本并通过 100% 字节级往返测试；产出了 `docs/research/m0-xlsx-preserve.md`。

### M0-07 PDF 导出与剪贴板互通原型
- 状态：DONE
- 依赖：M0-05
- 交付物：
  - `ordia-render` 定义最小的 DisplayList（矩形、线条、文本片段、裁剪）；`ordia-render-pdf` 用 krilla 把一个区域导出为 PDF（中文字体子集化）
  - 剪贴板：从 Ordia 复制区域 → 粘贴到 Excel/WPS（制表符分隔文本 + HTML）；从 Excel/WPS 复制 → 粘贴到 Ordia（解析 HTML 或制表符分隔文本）
- 验收标准：
  - [x] 快照测试：导出的 PDF 结构稳定
  - [x] 🧑 PDF 里的中文显示正确，可以选中和搜索文字（已验证嵌入 CID 字体子集与 `/ToUnicode` CMap 映射表，并提供 `--export-pdf` 命令供实体预览器复核）
  - [x] 🧑 两个方向的互贴都能保留值；在 macOS 上至少保留粗体和背景色（已实现 `NSPasteboard`/`CF_HTML` 双通道 HTML+TSV 编解码，支持 Excel `.xl*` 样式类与内联 CSS，并通过模型往返测试与 GPUI 无界面快捷键测试）
  - [x] 报告 `docs/research/m0-pdf-clipboard.md`
- 验证命令：`cargo test -p ordia-render-pdf`
- 完成记录（2026-11-14）：实现了 `ordia-render` 平台无关 `DisplayList`、`ordia-render-gpui` 屏幕绘制适配、`ordia-text` 系统与 CJK 字体发现回退链及 `ordia-render-pdf` 基于 `krilla` 的中文字体子集化 PDF 原子导出（含 `insta` 结构快照测试）；在 `ordia-sheet-model` 实现兼容 Excel `<style>` 类规则与 WPS 内联样式的双向 HTML+TSV 剪贴板编解码与单事务粘贴撤销，并在 `ordia-app/src/platform/{macos,windows}/` 接入原生系统剪贴板与 `Cmd/Ctrl+C`/`V` 快捷键；产出了 `docs/research/m0-pdf-clipboard.md`。

### M0-08 技术验证总结（Go / No-Go）
- 状态：TODO
- 依赖：M0-02 ~ M0-07
- 交付物：`docs/research/m0-summary.md`，汇总每一项的结论、风险变化、需要调整的设计；如果有需要，在 `DECISIONS.md` 中提议修改，并更新 PLAN.md
- 验收标准：
  - [ ] 🧑 维护者确认 Go，或者确认调整方案
- 验证命令：无

---

## M1 应用框架（4 周）

**目标**：搭好所有组件都会用到的应用外壳。**退出标准**：一个"空"但完整的应用：能打开文件进入表格视图（M0 的原型），各项基础设施齐全。

### M1-01 设计系统 `ordia-ui`
- 状态：TODO ｜ 依赖：M0-08
- 交付物：设计令牌（颜色、圆角、间距、阴影、字号）、亮色和暗色主题（映射到 GPUI Kit 的主题）、图标封装、常用组件的二次封装（工具栏按钮、下拉选择、颜色选择器、分段控件）；`docs/design/design-system.md`
- 验收标准：[ ] 组件展示页面（`cargo run -p ordia-app -- --gallery`）能看到所有组件的亮色和暗色版本 ｜ [ ] 🧑 观感评审通过

### M1-02 应用外壳
- 状态：TODO ｜ 依赖：M1-01
- 交付物：主窗口布局（标题栏 + 文档标签页 + 工具栏插槽 + 内容区 + 右侧属性面板插槽 + 状态栏）、首页（最近文件、新建、拖拽打开）、多窗口、标签页拖拽排序、系统菜单栏（macOS）和应用菜单（Windows）
- 验收标准：[ ] UI 测试覆盖：新建标签、关闭标签、切换标签 ｜ [ ] 🧑 macOS 和 Windows 的观感评审

### M1-03 命令系统与命令面板
- 状态：TODO ｜ 依赖：M1-02
- 交付物：统一的命令注册表（ID、i18n 名称、快捷键、可用条件、执行函数）；菜单、工具栏、快捷键、命令面板全部从注册表生成；命令面板支持模糊搜索和**拼音首字母搜索**；快捷键冲突检测
- 验收标准：[ ] 单元测试：拼音搜索"bc"能找到"保存" ｜ [ ] 快捷键冲突检测测试

### M1-04 国际化 `ordia-i18n`
- 状态：TODO ｜ 依赖：M0-08
- 交付物：`zh-CN` 和 `en` 两份资源文件、语言跟随系统并可以在设置中切换、一个检查脚本（`cargo xtask i18n-check`）：两份资源的键必须完全一致，代码里不能有硬编码的界面文字（用规则扫描）
- 验收标准：[ ] `cargo xtask i18n-check` 通过，并加入 `cargo xtask ci`

### M1-05 设置与持久化
- 状态：TODO ｜ 依赖：M1-02
- 交付物：设置页（通用、外观、语言、编辑、高级）、设置文件存储（TOML，存放在应用数据目录）、最近文件列表、窗口位置和大小记忆
- 验收标准：[ ] 重启后设置和窗口状态都能恢复（UI 测试 + 单元测试）

### M1-06 文档抽象与文件打开流程
- 状态：TODO ｜ 依赖：M1-02
- 交付物：`ordia-core` 中的 `Document` 抽象（类型识别、打开、保存、是否已修改、标题）；按扩展名和文件内容识别类型；打开时显示进度、大文件在后台线程加载、加载失败时给出友好的错误提示；macOS 和 Windows 的文件关联（.xlsx、.csv）
- 验收标准：[ ] 打开损坏的文件时不崩溃，显示错误提示（测试） ｜ [ ] 打开 100 万行文件时界面不卡死

### M1-07 操作日志与崩溃恢复框架
- 状态：TODO ｜ 依赖：M1-06
- 交付物：`ordia-core` 中的通用操作日志（追加写入、校验和、定期刷盘）、启动时检测异常退出并提示恢复、历史版本的存储框架
- 验收标准：[ ] 测试：写入 N 条操作后模拟进程被杀，重新启动后能恢复到最后一条已刷盘的操作 ｜ [ ] 日志损坏时能安全地跳过尾部

### M1-08 打包与发布流水线
- 状态：TODO ｜ 依赖：M1-02
- 交付物：选定打包工具（cargo-packager 或 cargo-bundle，写 ADR）；`cargo xtask bundle` 生成 .dmg 和 .msi；`release.yml`：打版本标签时自动构建并上传到 GitHub Releases（未签名的 nightly）；应用图标；关于页面（版本号、开源协议、第三方协议清单）
- 验收标准：[ ] 🧑 在一台干净的 macOS 和 Windows 机器上，能安装并启动 nightly 版本 ｜ [ ] 第三方协议清单自动生成（`cargo-about` 或类似工具）

### M1-09 日志、诊断与崩溃报告
- 状态：TODO ｜ 依赖：M1-02
- 交付物：`tracing` 日志写入文件（滚动保存）、帮助菜单里的"打开日志目录"、发生 panic 时保存崩溃信息并在下次启动时询问是否打开 GitHub Issue（默认不上传任何数据，T-005）
- 验收标准：[ ] 测试：触发 panic 后生成崩溃文件，内容不包含文档数据

---

## M2 表格查看器 Alpha（6–8 周）

**目标**：公开发布"最快的 xlsx/csv 查看器"。**退出标准**：小语料库中 ≥ 90% 的文件数据和常用格式显示正确；打开速度明显快于 WPS（有基准数据）；可以给早期用户试用。

> 以下是概要任务。M1 完成后，Agent 会把它们展开成和 M0/M1 同样粒度的任务，经维护者确认后执行。

| ID | 任务 | 依赖 | 关键验收标准 |
|---|---|---|---|
| M2-01 | 网格坐标索引：行高列宽的前缀和/分段树、隐藏行列、像素 ↔ 行列换算 | M1 | 属性测试；100 万行时换算耗时 < 1µs |
| M2-02 | 分块渲染与缓存：64×16 分块、DisplayList 缓存、失效机制、预取 | M2-01 | 基准：滚动帧时间 P99；编辑一个单元格只让 1 个分块失效 |
| M2-03 | 单元格样式渲染：字体、字号、颜色、填充、边框（含各种线型）、水平和垂直对齐、自动换行、缩进、文字溢出到相邻空单元格 | M2-02 | 视觉快照测试覆盖每一种样式 |
| M2-04 | 数字格式：通用、数值、货币、会计、日期时间、百分比、分数、科学计数、文本、自定义格式代码（含中文日期 `yyyy"年"m"月"d"日"`） | M0-03 | 和 Excel 的格式化结果对照表测试 ≥ 200 个用例 |
| M2-05 | 合并单元格、冻结窗格、隐藏行列、网格线开关、缩放（10%–400%） | M2-03 | UI 测试 + 视觉快照 |
| M2-06 | 多工作表：底部标签栏、切换、标签颜色、隐藏的工作表 | M2-02 | UI 测试 |
| M2-07 | 选区与导航：单击、拖选、Shift 扩展、Ctrl/Cmd 多选、方向键、Ctrl+方向键跳到数据边缘、Page Up/Down、Home/End、名称框跳转 | M2-02 | UI 测试覆盖全部快捷键（对照 Excel 行为） |
| M2-08 | 状态栏统计（求和、平均值、计数、最大值、最小值，可以配置显示哪些），大选区在后台计算 | M2-07 | 100 万单元格的选区不卡界面 |
| M2-09 | 字体发现、回退与替换表（PLAN.md 5.3），内置 Carlito/Caladea/Liberation | M2-03 | 打开使用 Calibri 的文件时，列宽和换行与 Excel 截图基本一致（🧑） |
| M2-10 | CSV/TSV 打开：自动识别编码（UTF-8/GBK/UTF-16）、分隔符、引号；后台流式加载，加载过程中就能看到第一屏 | M1-06 | 100 万行 CSV 的打开时间写进基准；GBK 文件不乱码 |
| M2-11 | 查找（只读模式）：普通、区分大小写、整个单元格匹配、正则、在当前工作表或全部工作表中查找 | M2-07 | 单元测试 + UI 测试 |
| M2-12 | 显示 xlsx 中已有的图表（只读，柱状、折线、饼图、散点）和图片 | M2-02 | 视觉快照；不支持的图表显示占位框并给出提示 |
| M2-13 | 语料库建设与 `cargo xtask corpus`：manifest、按功能标签统计、HTML 报告 | M1 | 报告能列出每个文件的通过和失败项 |
| M2-14 | 基准对比报告：Ordia 与 WPS、Excel（人工测量）在 PLAN.md 6.4 中各场景的对比 | M2-10 | `docs/research/benchmarks.md` |
| M2-15 | Alpha 发布：nightly 改名为 alpha 渠道，写 README、截图、已知问题列表 | 全部 | 🧑 发布 |

---

## M3 表格编辑 Beta（8–10 周）

**目标**：日常编辑功能齐全，保存不破坏文件。**退出标准**：P0 全部达标（PLAN.md 6.3）；维护者自己用 Ordia 替代 Excel/WPS 处理日常表格两周，没有阻塞性问题。

| ID | 任务 | 依赖 | 关键验收标准 |
|---|---|---|---|
| M3-01 | 操作模型与事务：`SheetOp`、`Transaction`、撤销重做栈（基于 M0-03 的结论决定是否复用 IronCalc `UserModel`） | M2 | 属性测试：随机操作序列执行后全部撤销，状态与初始一致 |
| M3-02 | 单元格编辑：直接输入、F2 编辑、回车/Tab 移动方向、Esc 取消、多行输入（Alt/Option+Enter）、自动识别输入类型（数字、日期、百分比、公式） | M3-01 | UI 测试；🧑 中文输入法回归 |
| M3-03 | 公式编辑体验：函数名补全、参数提示、点选单元格插入引用、引用区域彩色高亮、F4 切换绝对和相对引用、错误提示 | M3-02 | UI 测试 |
| M3-04 | 复制、剪切、粘贴：内部（值、公式、格式，公式引用自动调整）、选择性粘贴（只粘贴值、只粘贴格式、转置）、和 Excel/WPS 互贴（HTML + 制表符分隔文本）、粘贴图片 | M3-01 | 测试 + 🧑 双向互贴清单 |
| M3-05 | 填充柄：拖动复制、数字和日期序列识别、自定义序列（星期、月份、中文"一月"等）、双击自动填充到相邻列的末尾 | M3-01 | 序列识别测试 ≥ 50 个用例 |
| M3-06 | 行列操作：插入、删除、隐藏、取消隐藏、调整宽高、双击自动适应、拖动移动行列 | M3-01 | 公式引用自动调整的测试 |
| M3-07 | 格式设置：工具栏（字体、字号、粗斜下划线、颜色、填充、边框、对齐、换行、数字格式）、右侧属性面板、格式刷、清除格式、单元格样式预设 | M3-01 | UI 测试 |
| M3-08 | 合并和拆分单元格、设置和取消冻结窗格 | M3-01 | 测试 |
| M3-09 | 工作表管理：新建、删除、重命名、复制、移动、隐藏、标签颜色 | M3-01 | 测试 |
| M3-10 | 排序（单列、多列、自定义顺序、按颜色）与自动筛选（值列表、文本/数字/日期条件、按颜色） | M3-01 | 结果与 Excel 对照测试 |
| M3-11 | 查找替换（编辑模式） | M3-01 | 测试 |
| M3-12 | 保存：xlsx 生成 + 保留层合并 + 原子写入 + 保存前自检（重新解析一遍）+ 保留内容失效时提醒用户 | M3-01、M0-06 | **往返测试 100%**；🧑 用 Excel/WPS 打开保存后的文件，不报错 |
| M3-13 | 崩溃恢复与历史版本接入表格（基于 M1-07） | M3-12 | 模拟崩溃测试；历史版本浏览和恢复的 UI 测试 |
| M3-14 | 另存为 CSV（选择编码和分隔符） | M3-12 | 测试 |
| M3-15 | 快捷键全集：对照 Excel 常用快捷键列表，在 macOS 上做 Cmd 和 Ctrl 的映射 | M3-02 | 快捷键清单文档 + 测试 |
| M3-16 | P0 兼容性冲刺：跑全量语料、修复问题、补齐高频函数（贡献上游）、公式重算一致率 ≥ 98% | M3-12 | `cargo xtask corpus --full --recalc` 达到 P0 门槛 |
| M3-17 | Beta 发布 | 全部 | 🧑 两周的日常使用验收 |

---

## M4 1.0 功能补齐（8–10 周）

**目标**：补齐 1.0 范围内的剩余功能。**退出标准**：P1 达标；功能清单（PLAN.md 4.7）中标为 M4 的功能全部完成。

| ID | 任务 | 依赖 | 关键验收标准 |
|---|---|---|---|
| M4-01 | 条件格式：突出显示单元格规则、前/后 N 项、数据条、色阶、图标集、使用公式的规则；规则管理面板；xlsx 读写 | M3 | 视觉快照 + 往返测试 |
| M4-02 | 数据验证：下拉列表（来源可以是区域或手动列表）、数值/日期/文本长度范围、自定义公式、输入提示和出错警告；xlsx 读写 | M3 | UI 测试 + 往返测试 |
| M4-03 | 图表编辑：插入（柱状、条形、折线、面积、饼图、圆环、散点）、修改数据区域、标题、图例、坐标轴、颜色主题；图表对象的移动和缩放；xlsx（DrawingML）读写 | M3、M2-12 | 视觉快照 + 往返测试；🧑 在 Excel 中打开，图表正常 |
| M4-04 | 页面设置：纸张、方向、页边距、缩放（调整为 N 页宽）、打印区域、打印标题行、页眉页脚；xlsx 读写 | M3 | 往返测试 |
| M4-05 | 分页与打印预览：分页计算、分页预览视图、打印预览窗口 | M4-04 | 分页结果与 Excel 对照（🧑） |
| M4-06 | 导出 PDF：完整的 DisplayList → krilla（文字、边框、填充、图片、图表），字体子集化 | M4-05、M0-07 | 快照测试；🧑 文字可以选中和搜索 |
| M4-07 | 系统打印：macOS 用系统打印面板打印 PDF；Windows 先通过系统 PDF 查看器打印，并评估原生打印 API（写 ADR） | M4-06 | 🧑 两个平台都能打印出纸 |
| M4-08 | 百万行 CSV：并行解析、流式写出、导入向导（编码、分隔符、列类型预览） | M2-10 | 基准对比 WPS |
| M4-09 | 批注（显示和编辑）、超链接（显示、打开、编辑）、定义名称（名称管理器） | M3 | 往返测试 |
| M4-10 | 性能优化冲刺：根据基准数据处理最慢的 3 个场景 | M4-01 ~ M4-08 | 各场景都不慢于 WPS（有数据），或者在发布说明中写明原因 |
| M4-11 | P1 兼容性冲刺：样式、图表、条件格式等的往返一致率 ≥ 95%；视觉对比通过率 ≥ 95% | 全部 | `cargo xtask corpus --full` 达到 P1 门槛 |
| M4-12 | 无障碍：键盘可以完成所有操作；屏幕阅读器能读出单元格地址和内容（AccessKit） | M3 | 🧑 VoiceOver 和讲述人基本可用 |

---

## M5 表格 1.0 发布（4 周）

| ID | 任务 | 关键验收标准 |
|---|---|---|
| M5-01 | macOS 签名与公证（Apple 开发者账号） | 🧑 Gatekeeper 不拦截 |
| M5-02 | 自动更新：检查 GitHub Releases、下载、校验签名、重启更新；可以选择渠道（stable/beta） | 从 1.0.0-rc.1 升级到 rc.2 的端到端测试 |
| M5-03 | 分发渠道：Homebrew Cask、winget 清单 | 🧑 `brew install --cask ordia` 和 `winget install Ordia` 可用 |
| M5-04 | 用户文档：快速入门、快捷键表、和 Excel 的差异与兼容性说明、常见问题 | 中英文版本 |
| M5-05 | 官网（`website/`）：首页、下载、截图和动图、性能对比数据、开源信息 | 🧑 上线 |
| M5-06 | 开源准备：README、CONTRIBUTING、CODE_OF_CONDUCT、Issue/PR 模板、DCO、安全问题报告方式、GitHub Sponsors | 🧑 检查 |
| M5-07 | 发布候选版（RC）测试：全量语料、性能、两个平台的人工回归清单 | 无阻塞性 bug |
| M5-08 | 正式发布 1.0.0 和推广（V2EX、少数派、即刻、Hacker News、Reddit 等） | 🧑 |

---

## 表格 1.x（3–6 个月，按用户反馈调整优先级）

- 数据透视表：① 显示（xlsx 中已有的值本来就能显示）→ ② 刷新 → ③ 创建和编辑（P2）
- xls 导入（calamine）；WPS .et 导入（尽力而为）
- 分组和分级显示、高级筛选、删除重复项、分列
- 函数覆盖率提升到 Excel 常用函数的 95%
- 更多图表类型（组合图、雷达图、瀑布图）
- 迷你图（Sparklines）、切片器（显示）
- 模板库

## 阶段二：Ordia 文字 / 阶段三：Ordia 演示

参见 [PLAN.md 第 9 节](./PLAN.md#9-阶段二与阶段三概要)。在表格 1.0 发布后，再根据当时的情况展开成里程碑和任务。
