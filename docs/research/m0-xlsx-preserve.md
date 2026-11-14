# M0-06 研究报告：xlsx 保留层原型（不认识也不破坏）

## 1. 目标与背景

Excel / WPS 在真实企业场景中生成的 `.xlsx` / `.xlsm` 包通常包含远超普通单元格与公式的 OOXML 扩展部件，例如数据透视表（`pivotTables` / `pivotCache`）、VBA 宏工程（`vbaProject.bin`）、切片器（`slicers` / `slicerCaches`）、复杂组合图表（`drawings` / `charts`）、OLE 嵌入对象、批注及扩展列表（`<extLst>`）。

根据架构硬性规则 **§4.4（"不认识也不破坏"）** 和 **§4.5（原子保存）**，`ordia-xlsx` 必须在计算引擎（IronCalc）只重建核心工作簿部件的前提下，将所有未被引擎接管的未知/高阶部件原封不动地保留并重新挂载回最终的 ZIP 包，同时自动检测用户编辑是否可能破坏保留部件的数据源一致性。

---

## 2. 拆分与合并架构（`PreservedPackage` + `merge_engine_with_preserved`）

### 2.1 引擎托管部件 vs 原样保留部件

打开 `.xlsx` / `.xlsm` 时，`PreservedPackage::extract_from_bytes` 扫描 OPC ZIP 包中的所有条目，将其划分为两类：

| 类别 | 路径集合 | 处理策略 |
|---|---|---|
| **引擎托管部件** | `[Content_Types].xml`、`_rels/.rels`、`docProps/{app,core}.xml`、`xl/{workbook,sharedStrings,styles,metadata}.xml`、`xl/theme/theme1.xml`、`xl/_rels/workbook.xml.rels`、`xl/worksheets/sheet{N}.xml`、`xl/worksheets/_rels/sheet{N}.xml.rels` | 交由 `IronCalcEngine` 解析并在保存时重新导出 |
| **原样保留部件** | `xl/vbaProject.bin`、`xl/pivotTables/*`、`xl/pivotCache/*`、`xl/slicers/*`、`xl/slicerCaches/*`、`xl/drawings/*`、`xl/charts/*`、`xl/tables/*`、`xl/media/*`、`customXml/*` 等其余全部条目 | 在 `PreservedPackage::raw_parts` 中逐字节原样驻留，写回时零重编码输出 |

### 2.2 OPC 关系与 XML 挂载点缝合

由于 IronCalc 导出的核心 XML 不包含未知部件的引用，`merge_engine_with_preserved` 在保存阶段执行以下无损缝合：

1. **`[Content_Types].xml` 协调**：合并原始包中非引擎部件的 `<Default>`（如 `.bin` 宏二进制流）与 `<Override>` 条目；若工作簿包含 `vbaProject.bin` 或原本即为 `.xlsm`，自动将 `/xl/workbook.xml` 的 `ContentType` 从普通 `sheet.main+xml` 升级回 `sheet.macroEnabled.main+xml`。
2. **`_rels/.rels` 与 `xl/_rels/workbook.xml.rels` 协调**：保留非引擎根级关系与工作簿级关系（`vbaProject`、`pivotCacheDefinition`、`slicerCache`、`customXml` 等），通过两阶段 `r:id` 分配器消除与引擎新分配 `rId` 的冲突。
3. **`xl/workbook.xml` 尾部节点恢复**：恢复 `<workbookPr codeName="ThisWorkbook"/>` 属性，并在 `</workbook>` 闭合前按 OOXML Schema 顺序注入重写过 `r:id` 的 `<pivotCaches>` 与非引擎 `<extLst>`（如 `x14:slicerCaches`）。
4. **`xl/worksheets/sheet{N}.xml` 与 `sheet{N}.xml.rels` 协调**：
   - 将工作表依附通过**工作表名称**（而非可能因增删工作表而漂移的序号）绑定。
   - 保留引擎导出的外部超链接关系，同时追加原有的 `drawing`、`pivotTable`、`table`、`vmlDrawing`、`comments`、`slicers` 关系并完成两阶段防冲突 `r:id` 重映射。
   - 在 `</worksheet>` 闭合前严格按 ECMA-376 顺序（`<drawing>` → `<legacyDrawing>` → `<tableParts>` → `<extLst>`）重新注入挂载节点。
5. **原子写盘（`atomic_write_xlsx`）**：先写入同目录带 `PID + 纳秒时间戳 + 原子递增序号` 的隐藏临时文件 → `File::sync_all()` 刷盘 → 重新用 `ZipArchive` 校验归档完整性与核心条目存在性 → `fs::rename` 原子替换目标路径。

---

## 3. 往返验证结果

我们在 `corpus/public/` 中构建并登记了 4 个覆盖典型高阶特性的手工合成语料文件（MIT 协议，无外部隐私数据）：

| 语料文件 | 包含的未知/高阶部件 | 修改普通单元格后未知部件字节一致性 |
|---|---|---|
| `m0_pivot_table.xlsx` | `xl/pivotCache/pivotCacheDefinition1.xml`、`pivotCacheRecords1.xml`、`xl/pivotTables/pivotTable1.xml` | **100% 字节完全一致** |
| `m0_macro_vba.xlsm` | `xl/vbaProject.bin`、`.xlsm` 宏工作簿 `ContentType`、`codeName="ThisWorkbook"` | **100% 字节完全一致** |
| `m0_slicer.xlsx` | `xl/slicerCaches/slicerCache1.xml`、`xl/slicers/slicer1.xml`、工作簿与工作表 `<extLst>` | **100% 字节完全一致** |
| `m0_complex_chart.xlsx` | `xl/drawings/drawing1.xml`、`xl/charts/chart1.xml`（柱线双轴组合图） | **100% 字节完全一致** |

自动化验证测试（`crates/ordia-xlsx/tests/preservation_roundtrip.rs`）已覆盖全部 4 个文件的“打开 → 修改普通数据格并重算公式 → 原子保存 → 逐字节比对所有保留部件与关系挂载点”。

### 人工复核指引（🧑 验收项）
在安装有 Microsoft Excel 或 WPS Office 的机器上：
1. 运行 `cargo test -p ordia-xlsx`（或使用 Ordia 打开并另存 `corpus/public/m0_pivot_table.xlsx` 与 `corpus/public/m0_macro_vba.xlsm`）。
2. 在 Excel / WPS 中打开保存后的文件，确认无“发现不可读取的内容 / 修复工作簿”弹窗，透视表字段面板正常展开，且 `.xlsm` 的 VBA 工程模块仍可在宏编辑器中查看。

---

## 4. 哪些编辑会让保留内容失效？如何提醒用户？

字节级原样保留能保证“未触碰区域的零损往返”，但当用户的编辑跨越了未知部件所依赖的结构边界时，保留部件内部的引用可能与新工作表状态产生语义脱节：

| 失效场景 (`PreservationHazardKind`) | 成因说明 | 当前检测状态与后续策略 |
|---|---|---|
| **删除含依附部件的工作表** (`AttachedSheetDeleted`) | 工作表被删除后，挂载在其上的透视表、切片器、图表或表格定义失去宿主工作表 | **已在 `check_preservation_warnings` 实现**；合并器自动剥离孤儿工作表关系，避免生成损坏的 `.xlsx` |
| **清空透视表 / 图表的数据源区域** (`PivotSourceCleared` / `ChartSourceCleared`) | `<cacheSource><worksheetSource ref="..."/>` 或图表 `<c:f>Sheet1!$A$1:$B$4</c:f>` 指向的区域被全部清空，导致下次在 Excel 中刷新透视表或查看图表时变为空白 | **已在 `check_preservation_warnings` 实现**；解析 `pivotCacheDefinition` 与 `chart*.xml` 引用并在保存前扫描源区域非空性 |
| **数据源区域内的插入/删除行列** | 在透视表缓存源或图表公式引用中间插入/删除行列时，若未同步重写 `pivotCacheDefinition` 的 `ref` 或 `chart.xml` 的 `<c:f>`，范围边界可能偏移 | M3（行列结构编辑与文件保存完善阶段）在执行结构化 `SheetOp` 时同步重写引用坐标，无法安全重写时升级为 `PreservationWarning` |
| **重命名被 VBA 宏硬编码按名称引用的工作表** | `vbaProject.bin` 为压缩二进制流（MS-OVBA），若宏代码写了 `Sheets("Sales")` 而用户改名，宏运行时会报下标越界（使用 `codeName` 则不受影响） | 当 `has_vba_project == true` 且发生工作表重命名/删除时，弹出非阻塞风险提示 |

### 用户提醒交互设计（对接 M1/M3 保存流程）
1. **打开时轻提示（非打断）**：当 `PreservedPackage` 检测到透视表、VBA 宏或切片器等当前版本仅支持保留/只读展示的部件时，在工作簿顶部横幅（Banner）温和提示：“此工作簿包含宏 / 透视表，Ordia 将在保存时原样保留它们”，不阻塞正常浏览与编辑。
2. **保存前风险确认框（仅在命中 `PreservationWarning` 时触发）**：保存前调用 `XlsxWorkbook::preservation_warnings()`。若返回非空警告列表，弹出对话框明确列出受影响的部件与原因（例如：“工作表 `Sales` 的数据源 `A1:C5` 已被清空，关联的数据透视表刷新后可能为空”），提供 **“继续保存”** 与 **“另存为副本”** 两个选项，把选择权交给用户。

---

## 5. 结论

**Go**。基于 `quick-xml` + `zip` 的 OPC 拆分/缝合架构在不侵入 `IronCalc` 内部数据结构的前提下，以极低开销实现了 `.xlsx` / `.xlsm` 未知部件的 100% 字节级无损保留与数据源失效预警，完全满足架构规则 §4.4 与 §4.5。
