# AGENTS.md — Ordia 的 AI Agent 工作规范

> 每个在本仓库工作的 AI Agent（以及人类贡献者），开工前都必须读完本文件。

## 1. 项目是什么

Ordia 是一个基于 **Rust + GPUI + GPUI Kit** 的高性能、高颜值跨平台办公套件，目标是替代 Office 和 WPS。第一个产品是**表格**（对标 Excel），平台为 macOS + Windows。

必读文档：
- [docs/PLAN.md](docs/PLAN.md)：总体方案与架构（**事实来源**）
- [docs/DECISIONS.md](docs/DECISIONS.md)：已经拍板的决策（**不能擅自推翻**）
- [docs/TASKS.md](docs/TASKS.md)：任务清单与状态

## 2. 工作流程

1. 读 `docs/TASKS.md`，选择**第一个依赖已满足的 `TODO` 任务**，把状态改成 `DOING`
2. 阅读 PLAN.md 中与该任务相关的章节
3. 实现 → 写测试 → 运行任务中的验证命令和 `cargo xtask ci`
4. 验收标准**全部满足**后，把状态改成 `DONE`，在任务下方追加：
   ```
   - 完成记录（YYYY-MM-DD）：做了什么；关键设计选择；遗留问题和后续建议
   ```
5. 如果有标 🧑 的人工验收项：完成其余部分后，标为 `BLOCKED（等待人工验收）`，并写清楚人需要检查什么、怎么检查
6. 一次只做一个任务；每个任务单独提交（Conventional Commits，例如 `feat(sheet-view): virtualized grid element`）

## 3. 必须停下来问人的情况

- 想修改或违背 `docs/DECISIONS.md` 中的任何决策
- 需要引入新的**非 Apache-2.0 / MIT / BSD / ISC / Zlib / Unicode-3.0 / MPL-2.0** 协议的依赖（GPL、AGPL、LGPL 一律禁止）
- 需要升级 GPUI Kit 或 GPUI 的版本
- 架构层面的改动：新增 crate、改变依赖层次、改变操作模型或 DisplayList 的核心结构
- 验收标准无法达成，需要降低标准
- 任务描述有歧义，存在两种以上合理但效果不同的理解

## 4. 架构硬性规则（CI 会检查）

1. **内核层不依赖 GPUI**：`ordia-core`、`ordia-i18n`、`ordia-text`、`ordia-render`、`ordia-render-pdf`、`ordia-sheet-engine`、`ordia-sheet-model`、`ordia-sheet-layout`、`ordia-chart`、`ordia-xlsx`、`ordia-csv` 禁止依赖 `gpui`、`gpui-kit`、`gpui-component`
2. **IronCalc 不外泄**：只有 `ordia-sheet-engine`（以及 `ordia-xlsx` 中必要的部分）可以引用 IronCalc 的类型；公开 API 只暴露 Ordia 自己的类型
3. **所有修改都走操作模型**：文档的任何修改都必须表达成 `Operation` / `Transaction`，禁止绕开它直接修改模型（否则撤销、崩溃恢复、历史版本都会出问题）
4. **"不认识也不破坏"**：处理 xlsx 时，任何没有被修改的内容都必须原样写回；不能为了省事而丢弃不认识的 XML 或 part
5. **保存必须是原子的**：先写临时文件，校验后再替换原文件
6. **平台相关代码**集中放在 `ordia-app/src/platform/{macos,windows}/`

## 5. 代码规范

- Rust 版本由 `rust-toolchain.toml` 固定，edition 2024
- `cargo fmt`；`cargo clippy --all-targets -- -D warnings` 必须没有警告
- 库代码禁止 `unwrap()`、`expect()`、`panic!()`；确实不会失败的地方，用 `expect("原因")` 并加注释说明（只允许在不变量明确的地方使用）
- 错误处理：库代码用 `thiserror` 定义错误类型，程序入口用 `anyhow`
- 公开的类型和函数必须写文档注释（`///`）
- **界面文字一律走 i18n**，同时在 `zh-CN` 和 `en` 中添加，`cargo xtask i18n-check` 必须通过
- 性能敏感路径（绘制、布局、滚动、命中测试）：禁止在每帧中分配大块内存、加锁、做 IO；修改后附上 `cargo xtask bench` 的前后对比
- 优先使用 GPUI Kit 已有的组件；需要自定义时，先在 `ordia-ui` 中封装，不要在业务代码里到处写样式
- 不写无意义的注释；注释用来解释"为什么"，而不是"做了什么"

## 6. 测试要求

- 每个任务都必须带测试；修复 bug 时必须先写一个能复现的测试
- 纯逻辑：单元测试 + 必要时的属性测试（`proptest`）
- 渲染输出：对 DisplayList 做快照测试（`insta`），更新快照时要在提交说明中解释原因
- 界面交互：使用 GPUI Kit 的无界面 UI 测试
- 文件格式：加入 `corpus/` 并在 `corpus/manifest.toml` 中登记（来源、协议、功能标签），**不能加入来源不明或有隐私数据的文件**

## 7. 常用命令

```sh
cargo xtask ci              # 提交前必须通过：fmt、clippy、test、依赖检查、i18n 检查
cargo xtask check-deps      # 检查分层依赖规则
cargo xtask i18n-check      # 检查翻译资源
cargo xtask corpus          # 小语料往返测试
cargo xtask corpus --full --recalc   # 全量语料 + 公式重算对比
cargo xtask bench           # 性能基准
cargo xtask bundle          # 打包 .dmg / .msi
cargo run -p ordia-app      # 运行应用
```

> 在 M0-01 完成之前，这些命令还不存在。M0-01 的任务就是把它们建立起来。

## 8. 完成的定义（Definition of Done）

- [ ] 任务中的验收标准全部满足（🧑 项交给人）
- [ ] `cargo xtask ci` 通过
- [ ] 新增和修改的代码有测试
- [ ] 界面文字有中英文两份
- [ ] 没有违反第 4 节的架构规则
- [ ] `docs/TASKS.md` 已更新状态和完成记录；如果有影响方案的发现，已同步更新 PLAN.md 或在 `docs/research/` 中记录
