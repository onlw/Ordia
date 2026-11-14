# M0-08 技术验证总结（Go / No-Go 评审报告）

- **日期**：2026-11-14
- **范围**：汇总 `M0-01` ~ `M0-07` 全部技术验证结论、风险矩阵变化与 M1–M4 设计收敛项
- **总体结论**：**全面通过（GO）** —— `DECISIONS.md` 中的 `D-001 ~ D-017` 与 `T-001 ~ T-005` 全部经受住原型实测检验，无需推翻任何既定架构决策，可直接推进至 **M1（应用框架）**。

---

## 1. M0 各子项验证结论一览

| 任务 | 验证核心问题 | 实测关键指标与证据 | 结论 |
|---|---|---|---|
| **M0-01** 仓库骨架与 CI | 17 个分层 crate + `xtask` 架构依赖守卫 + 许可证合规能否闭环？ | `cargo xtask check-deps` 自动拦截 11 个内核 crate 对 GPUI 的直接/传递依赖及 IronCalc 外泄；`cargo deny` 全绿。 | **GO** |
| **M0-02** GPUI Kit 主窗口 | `gpui-kit =0.9.1` 组件完备度、亮暗主题切换、无界面测试能力如何？ | 固定 Rust `1.100.0`（edition 2024）；复用 `TitleBar`、`TabBar`、`Sidebar`、`Input`、`Dialog`、`Menu`；`#[gpui_kit::test]` 无界面测试毫秒级通过。 | **GO** |
| **M0-03** IronCalc 引擎边界 | `ironcalc =0.9.1` 函数覆盖率、OOXML 读写范围与零泄露封装是否可行？ | 已覆盖 **497 个 Excel 函数（~94%+，高频函数 ~99%）**、18 类条件格式、动态数组溢出；公开 API 零 IronCalc 类型泄露。 | **GO** |
| **M0-04** 大数据量性能基准 | 10k / 100k / 1M × 20 行的加载、重算、保存耗时与内存占用是多少？ | 10 万行净内存 **182.9 MB**、重算 **131 ms**；100 万行（2,000 万格 + 200 万公式）净内存 **1.86 GB**（~97 B/格），但单格重算等同全量重算（`1.53 s`）。 | **GO（含定向增强）** |
| **M0-05** 百万行网格与中文 IME | 100 万行滚动能否稳定 `< 8 ms`？中文输入法候选框能否跟随单元格？ | 消除全表元数据扫描 + 按需下发 `ContentMask` 合批 + 双代 `ShapedLineCache` 后，100 万行惯性滚动 **`P50 = 3.48 ms / P99 = 7.00 ms`**，全表冷跳滚 **`P99 = 7.96 ms`**；`ElementInputHandler` 中文预编辑锚点精确对齐单元格。 | **GO** |
| **M0-06** xlsx 保留层原型 | 含透视表、VBA 宏（`.xlsm`）、切片器、复杂图表的文件编辑保存后能否零损保留？ | `PreservedPackage` + `merge_engine_with_preserved` 完成未知部件 100% 字节级原样写回、两阶段防冲突 `r:id` 重映射、原子写盘及数据源清空风险检测（`PreservationWarning`）。 | **GO** |
| **M0-07** PDF 导出与剪贴板互通 | 纯 Rust 中文子集化 PDF 与 Excel/WPS 双向富格式互贴是否可行？ | `ordia-render` + `krilla 0.8` 导出含中文与边框的预算表仅 **~20 KB**，内嵌 CID 字体子集与 `/ToUnicode` CMap（可选中/可搜索）；`arboard` 双通道 HTML+TSV 兼容 Excel `.xl*` 样式类与内联样式，支持单步撤销。 | **GO** |

---

## 2. 风险矩阵更新（对比 `PLAN.md §8`）

| 风险编号 | 原风险描述 | M0 后风险等级变化 | 验证后的应对落地机制 |
|---|---|---|---|
| **R1** | GPUI / GPUI Kit API 变动 | 高 → **可控（低）** | `gpui-kit =0.9.1` + `Cargo.lock` + `rust-toolchain.toml` 已锁定，且 `check-deps` 强制将 GPUI 隔离在 4 个上层 crate 内。 |
| **R2** | IronCalc 大数据量性能或内存不足 | 中 → **低** | 内存表现优异（100 万行净占 1.86 GB）；针对 `> 8 MiB` `.xlsx` 采用 `from_xlsx_bytes_unrecalculated` 读 `<v>` 缓存值秒开，针对百万行 `.csv` 在 `SheetEngine` 下新增列式只读/按需写时复制引擎 `CsvSheetEngine`（M2-10）。 |
| **R3** | IronCalc 函数覆盖或计算差异 | 高 → **低** | 497 个函数已覆盖绝大多数场景；在 M2-13 建立 `cargo xtask corpus --recalc` 自动对比 Excel `<v>` 缓存答案。 |
| **R4** | 中文输入法在自定义网格异常 | 中 → **低** | 已通过 `ElementInputHandler` + `InputState` 桥接跑通原地首字母唤起预编辑与候选窗坐标跟随，并用 `OrdiaSheetGrid && !Input` 消除快捷键抢占。 |
| **R5** | xlsx 往返保存损坏未知内容 | 中（极高影响） → **可控（低）** | `ordia-xlsx` 字节级保留层、OPC 校验、同目录临时文件 `sync_all` + `rename` 原子替换与 `PreservationWarning` 预警均已跑通。 |
| **R6 / R9** | 中文 PDF 体积膨胀或乱码 | 中 → **已消除** | `fontdb`（含现代 macOS `MobileAsset` `PingFang.ttc` 探测）+ `krilla` 子集化实测单页仅 20 KB 且自带 `/ToUnicode`。 |

---

## 3. 收敛并回写 `PLAN.md` 的具体设计细化（不触及 `DECISIONS.md`）

1. **撤销重做统一采用 Ordia 自研 `SheetOp` / `Transaction`（对应 `PLAN.md §4.2`）**：
   - M0-03 查明 IronCalc `UserModel` 的 `Diff` 为私有不透明字节，无法携带选区前后状态、i18n 标签、CRC32 崩溃恢复日志及排序/筛选/图表/数据验证等扩展操作，故正式确立在 `ordia-sheet-model` + `ordia-core` 中统一维护可序列化 `Transaction`。
2. **超大文件分层打开策略（对应 `PLAN.md §4.3`）**：
   - `> 8 MiB` 的 `.xlsx` 在后台线程加载并直接使用缓存结果 `<v>` 首屏渲染；百万行 `.csv`/`.tsv` 通过 `ordia-csv` 流式列存储实现 `< 1.5 s` 打开。
3. **剪贴板选型明确为 `arboard`（无 `unsafe`、MIT/Apache-2.0，对应 `PLAN.md §3.3`）**：
   - 平台原生调用统一收敛于 `ordia-app/src/platform/{macos,windows}/`，模型层 HTML 解析器同时支持内联 `style` 与 Excel `<style>` `.xl*` 类表。

---

## 4. 里程碑切换评审清单

- [x] `M0-01` ~ `M0-07` 所有自动化测试、快照测试、无界面 UI 测试、性能基准与依赖/许可证检查 100% 通过
- [x] 无违反 `AGENTS.md §4` 架构硬性规则项
- [x] 无新增非允许开源协议依赖
- [x] 正式进入 **M1：应用框架（`M1-01` ~ `M1-09`）**
