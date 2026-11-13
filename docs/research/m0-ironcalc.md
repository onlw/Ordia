# M0-03 调研报告：IronCalc 集成与能力边界

- **日期**：2026-11-14
- **评估版本**：`ironcalc = 0.9.1`、`ironcalc_base = 0.9.1`（MIT OR Apache-2.0）
- **对应任务**：`M0-03`（`crates/ordia-sheet-engine`）

---

## 1. 架构封装与内部存储发现

1. **零泄露边界（`D-004`）**：`crates/ordia-sheet-engine` 定义了纯 Ordia 领域的 `SheetEngine` trait 与零基坐标（`(0, 0) == A1`）领域模型（`CellCoord`、`CellRange`、`CellValue`、`FormulaError`、`CellStyle`、`FreezePane`、`SheetInfo`、`RowMetric`、`ColSpanMetric` 等）。所有 `ironcalc` / `ironcalc_base` 类型严格封装在私有模块 `ironcalc_impl.rs` 内部，公开 API 零上游类型泄露（由 `public_api_exposes_zero_ironcalc_types` 测试守卫）。
2. **分块与紧凑数值存储**：源码审计发现 IronCalc `0.9.1` 的 `SheetData` 已采用按 `1024` 行分块（`CHUNK_BITS = 10`）的稀疏数组 + 行内有序 `(column, Cell)` 向量，并且对**连续纯数值行采用 `Row::Numbers` 紧凑 `Vec<f64>` 存储（每格仅 8 字节，按需构造 `Cow<Cell>`）**，避免了早期按格 HashMap 的内存膨胀和哈希开销。
3. **缓存值与重算分离**：`IronCalcEngine::from_xlsx_bytes_unrecalculated` 利用 `Model::from_workbook` 保留 Excel/WPS 写入的 `<v>` 缓存结果，随后调用 `evaluate()` 即可逐格比对重算结果，直接支撑 `cargo xtask corpus --recalc`。

---

## 2. 14 项核心能力逐项评估（读 / 写 / 计算）

| 功能项 | xlsx 读取 | xlsx 写出 | 引擎计算 / 操作 | 详细边界说明 |
|---|---|---|---|---|
| **1. 合并单元格** | **支持** | **支持** | **支持** | 完整解析与写出 `<mergeCells>`；导入时自动清理被覆盖格的残留值并保留样式；提供 `merge_cells` / `unmerge_cells` / `MergeStructure` 校验防重叠与防数组公式冲突。 |
| **2. 冻结窗格** | **部分支持** | **支持** | **支持** | 支持 `state="frozen"` 的 `xSplit` / `ySplit`（首行、首列、任意行列四象限冻结）读写；忽略普通拆分窗格（`split` / `frozenSplit`）与多 `sheetView`。 |
| **3. 数字格式** | **支持** | **支持** | **部分支持** | 读写 `xl/styles.xml` 中的内置与自定义 `<numFmts>`，支持 4 段式正/负/零/文本格式、千分位、百分比、科学计数、日期时间及 `TEXT()` 函数；对中文特殊格式（如 `[DBNum1]`/`[DBNum2]` 大写数字、部分复杂会计对齐 `_(* #,##0_)`）尚未完整覆盖。 |
| **4. 条件格式** | **支持** | **支持** | **支持** | 支持多达 **18 种规则**（`CellIs`、`Formula`、`Text`、`TimePeriod`、`DuplicateValues`、`UniqueValues`、`Blanks`/`NotBlanks`、`Errors`/`NoErrors`、`AboveAverage`/`BelowAverage`、`Top10`/`Bottom10`、`ColorScale`、`DataBar`、`IconSet`、`IconRating`），含 OOXML `x14` `extLst` 扩展与内联 `<x14:dxf>` 读写！仅 `DataBar` 的边框与坐标轴细节字段暂缺。 |
| **5. 数据验证** | **不支持** | **不支持** | **不支持** | `Worksheet` 未解析 `<dataValidations>`，导出时也会丢弃该节点。 |
| **6. 图表** | **不支持** | **不支持** | **不支持** | 不解析 `xl/charts/*`、`xl/drawings/*` 及 `<drawing r:id="..."/>`。 |
| **7. 批注** | **部分支持** | **不支持** | **只读** | 导入时通过工作表 `.rels` 解析传统批注 `xl/commentsN.xml` 到 `Worksheet::comments`（作者与纯文本），不支持现代线程式批注（`threadedComments`）；**导出 `save_xlsx_to_writer` 目前不会写回 `commentsN.xml` 或 VML drawing**。 |
| **8. 超链接** | **支持** | **支持** | **支持** | 支持外部链接（`xl/worksheets/_rels/sheetN.xml.rels`）和工作簿内部跳转（`location`）的确定性读写，并支持动态 `HYPERLINK()` 公式。注意：导出会重排工作表 `.rels` 的 `rId1..rIdN`，在 `ordia-xlsx` 合并未知关系时需重映射 `rId`。 |
| **9. 筛选** | **部分支持** | **不支持** | **部分支持** | 支持动态数组 `FILTER()` / `SORT()` / `SORTBY()` 函数，且导入了结构化表 `xl/tables/tableN.xml` 的 `has_filters` 标志；但**不支持工作表级 `<autoFilter>` 条件解析与行过滤状态执行**，导出也不写回 `<autoFilter>` 与 `xl/tables/*`。 |
| **10. 隐藏行列** | **支持** | **支持** | **支持** | 完整支持行 `hidden="1"`、列区间 `<col hidden="1"/>` 以及工作表级 `Visible` / `Hidden` / `VeryHidden` 读写，且 `SUBTOTAL` 函数能感知隐藏行。 |
| **11. 定义名称** | **支持** | **支持** | **支持** | 支持工作簿全局与工作表局部（`localSheetId`）定义名称及 `LAMBDA` 命名函数读写、重命名工作表自动重写引用。已知小瑕疵：导入阶段 `reparse_formula_hack` 对嵌套引用其他定义名称或结构化表的名称存在上游 `FIXME`。 |
| **12. 数组公式与动态数组** | **支持** | **支持** | **支持** | 完整支持传统 CSE 数组公式（`{=...}`）与现代动态数组自动溢出（`ArrayKind::Dynamic` + `SpillCell`）、`#SPILL!` 冲突检测、动态锚点拓扑重排（`RestartLog`）、隐式交集 `@` 以及 `xl/metadata.xml`（`XLDAPR`）导出。 |
| **13. 撤销重做（`UserModel`）** | — | — | **部分支持** | `UserModel` 内置基于 `Diff` 的撤销/重做栈（支持 `bitcode` 编码同步），覆盖单元格输入、样式、行列增删改移、合并单元格、冻结窗格、工作表管理、条件格式等。但 `Diff` 为 `pub(crate)` 不透明字节，不包含选区前后状态、i18n 标签，也不覆盖排序/筛选/数据验证/图表/页面设置等 Ordia 扩展能力。 |
| **14. 变更通知** | — | — | **不支持** | `Model::evaluate()` 采用全表自顶向下单遍求值（动态数组冲突时局部重启），不维护持久化反向依赖图，也不返回本次重算发生值变化的脏单元格集合（`DiffSet`）。 |

---

## 3. 函数覆盖率统计

通过对 `ironcalc_base-0.9.1/src/functions/mod.rs` 的 `Function` 枚举精确统计，IronCalc `0.9.1` 共实现了 **497 个 Excel 函数**（占 Excel 全量约 510–530 个函数的 **~94%+**，个人与企业高频函数覆盖率接近 **99%**）：

| 函数类别 | 已实现数量 | 代表函数（含中文与现代 Excel 365 重点函数） |
|---|---:|---|
| 统计（Statistical + 回归） | 110 | `AVERAGEIFS`, `COUNTIFS`, `MAXIFS`, `MINIFS`, `MEDIAN`, `FORECAST.ETS`, `LINEST`, `CORREL` |
| 数学与三角（Math & Trig） | 82 | `SUM`, `SUMIFS`, `SUMPRODUCT`, `ROUND`, `MROUND`, `SUBTOTAL`, `MMULT`, `MINVERSE`, `PERCENTOF` |
| 财务（Financial） | 55 | `NPV`, `XNPV`, `IRR`, `XIRR`, `PMT`, `IPMT`, `PPMT`, `DURATION`, `YIELD`, `VDB` |
| 工程（Engineering） | 55 | `CONVERT`, `BIN2HEX`, `BITAND`, `COMPLEX`, `IMSUM`, `ERF`, `BESSELJ` |
| 文本（Text，含双字节 DBCS） | 46 | `TEXT`, `TEXTSPLIT`, `TEXTJOIN`, `TEXTBEFORE`, `TEXTAFTER`, `REGEXEXTRACT`, `REGEXREPLACE`, `REGEXTEST`, `LENB`, `LEFTB`, `RIGHTB`, `MIDB`, `FINDB`, `SEARCHB`, `REPLACEB`, `ASC` |
| 兼容函数（Compatibility） | 37 | `NORMDIST`, `VARA`, `CHIINV`, `PERCENTILE`, `QUARTILE`, `RANK` 等旧版别名 |
| 查找与引用 + 动态数组 | 34 | `VLOOKUP`, `HLOOKUP`, `XLOOKUP`, `XMATCH`, `INDEX`, `MATCH`, `OFFSET`, `INDIRECT`, `FILTER`, `UNIQUE`, `SORT`, `SORTBY`, `SEQUENCE`, `RANDARRAY`, `HSTACK`, `VSTACK`, `TOCOL`, `TOROW`, `TAKE`, `DROP` |
| 日期与时间（Date & Time） | 25 | `DATE`, `DATEDIF`, `EDATE`, `EOMONTH`, `NETWORKDAYS.INTL`, `WORKDAY.INTL`, `ISOWEEKNUM`, `YEARFRAC` |
| 信息（Information） | 22 | `ISBLANK`, `ISERROR`, `ISFORMULA`, `ISTEXT`, `ISNUMBER`, `SHEET`, `SHEETS`, `CELL`, `INFO`, `FORMULATEXT` |
| 逻辑与 Lambda 高阶函数 | 19 | `IF`, `IFS`, `IFERROR`, `IFNA`, `SWITCH`, `LET`, `LAMBDA`, `MAP`, `REDUCE`, `SCAN`, `BYROW`, `BYCOL`, `MAKEARRAY` |
| 数据库（Database） | 12 | `DSUM`, `DAVERAGE`, `DCOUNT`, `DCOUNTA`, `DGET`, `DMAX`, `DMIN` |
| **合计** | **497** | — |

**尚未覆盖的少量函数**：主要是外部数据源类（`RTD`, `CUBE*`, `WEBSERVICE`, `FILTERXML`, `STOCKHISTORY`）、部分聚合辅助函数（`AGGREGATE`、`GROUPBY`、`PIVOTBY`）及个别东亚拼音函数（`PHONETIC`、`BAHTTEXT`、`JIS`）。

---

## 4. 缺失能力补齐分工建议（Ordia 自研 vs 贡献上游）

| 缺失或受限项 | 决策路径 | 落地模块与里程碑 | 理由 |
|---|---|---|---|
| **未知部件保留（透视表、VBA、切片器、自定义 XML、未改动的表格/批注/媒体）** | **Ordia 自研** | `ordia-xlsx`（M0-06 / M3-12） | 属于 Ordia 核心差异化能力（"不认识也不破坏"），需要基于原始 ZIP 归档做字节级合并与 `.rels` / `[Content_Types].xml` 重映射。 |
| **撤销重做与崩溃恢复日志** | **Ordia 自研** | `ordia-core` + `ordia-sheet-model`（M1-07 / M3-01） | 需要统一承载选区恢复、i18n 操作描述、CRC32 追加式崩溃恢复日志以及图表/筛选/数据验证等非引擎操作，不能受限于 `UserModel` 的私有 `Diff`。 |
| **变更通知 / 脏分块失效** | **Ordia 自研轻量层** | `ordia-sheet-model` + `ordia-sheet-layout`（M2-02 / M3-01） | 对普通输入直接失效目标块，包含公式时比对重算前后可见/缓存块摘要或按公式引用范围失效；若后续发现百万行重算瓶颈，再向 IronCalc 上游贡献增量依赖图 PR。 |
| **完备数字格式化（含中文日期与会计格式细节）** | **Ordia 自研展示层 + 回馈上游** | `ordia-sheet-layout` / `ordia-sheet-model`（M2-04） | 网格渲染与 PDF 导出需要支持颜色条件（`[Red]`）、会计星号填充、中文年月日/`[DBNum]` 等 ≥ 200 用例精确对齐 Excel。 |
| **自动筛选（`<autoFilter>`）与多列排序** | **Ordia 自研** | `ordia-sheet-model` + `ordia-xlsx`（M3-10） | 排序与筛选直接联动 UI 行隐藏状态、下拉过滤面板及可逆 `Transaction`。 |
| **数据验证（`<dataValidations>`）** | **Ordia 自研** | `ordia-sheet-model` + `ordia-xlsx` + `ordia-sheet-view`（M4-02） | 强交互特性（单元格下拉箭头、输入拦截弹窗、圈释无效数据）。 |
| **图表解析、渲染与编辑（DrawingML）** | **Ordia 自研** | `ordia-chart` + `ordia-xlsx`（M2-12 / M4-03） | 需输出统一 `DisplayList` 同时用于 GPUI 屏幕绘制与 `krilla` 矢量 PDF 导出。 |
| **批注写出、超链接编辑** | **Ordia 自研（后续可提 PR 给上游）** | `ordia-xlsx` + `ordia-sheet-model`（M4-09） | 先在 `ordia-xlsx` 补齐 `commentsN.xml` / VML 原样保留与增量写出，保持上游无分叉。 |
| **少量缺失公式函数（如 `AGGREGATE`）** | **优先贡献 IronCalc 上游** | M3-16 / M4-11 语料回归按需触发 | 遵循 `PLAN.md` §4.8，避免分叉计算引擎。 |
