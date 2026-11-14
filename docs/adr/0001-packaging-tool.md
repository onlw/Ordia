# ADR 0001：桌面安装包构建方案选型（`cargo xtask bundle` vs `cargo-packager` / `cargo-bundle`）

- **状态**：Accepted
- **日期**：2026-11-14
- **关联任务**：M1-08（打包与发布流水线）、M5-01（macOS 签名与公证）、D-016

---

## 1. 背景与需求

Ordia 需要一套在本地开发环境与 GitHub Actions（`macos-latest`、`windows-latest`）中均可一键复现的安装包构建流水线：

1. **macOS**：输出标准 `.app` 应用包与含 `/Applications` 拖拽快捷方式的压缩 `.dmg` 镜像，精确嵌入 `assets/macos/Info.plist`（含 `.xlsx`、`.xlsm`、`.csv`、`.tsv` 的 `CFBundleDocumentTypes` UTI 文件关联）与 `.icns` 多分辨率图标，并为 M5-01 的 `codesign --options runtime` 与 `xcrun notarytool` 公证预留确定性钩子。
2. **Windows**：输出标准 `.msi` 安装包（含安装目录、开始菜单快捷方式、应用图标及 `.xlsx`/`.xlsm`/`.csv`/`.tsv` 的 ProgID 文件关联）。
3. **合规性**：构建过程自动从 `cargo metadata` 解析全量第三方依赖协议并生成 `assets/THIRD_PARTY_LICENSES.txt`，同时内嵌到客户端“关于 Ordia”页面与安装包资源目录中。

---

## 2. 备选方案对比

| 维度 | 方案 A：`cargo-bundle` | 方案 B：`cargo-packager` | 方案 C：`cargo xtask bundle`（系统原生工具链 + WiX） |
|---|---|---|---|
| **维护活跃度** | 低，发版周期长 | 活跃（CrabNebula / Tauri 派生） | 自有代码（< 250 行 Rust），零外部工具锁死 |
| **额外工具安装耗时** | 需编译安装外部 CLI | 需编译庞大依赖树并在运行时联网拉取 NSIS/WiX | **0 额外安装**：macOS 自带 `iconutil` + `hdiutil`；GitHub `windows-latest` 预装 WiX Toolset |
| **自定义 UTI / ProgID 文件关联** | 仅支持简单扩展名列表，难以精确控制 `LSHandlerRank` 与多 MIME/UTI 映射 | 模板封装较深，自定义合并策略复杂 | **100% 原样控制**：直接使用仓库版本化管理的 `assets/macos/Info.plist` 与生成的 `Ordia.wxs` |
| **M5-01 Apple 签名与公证衔接** | 需事后拆包重签 | 支持部分钩子但排障黑盒 | 在组装 `.app` 与封装 `.dmg` 之间直接插入 `codesign` 与 `notarytool`，完全透明可控 |
| **第三方协议清单集成** | 不支持 | 不支持 | 在同一 `xtask` 流程内直接读取 `cargo metadata` 生成并校验许可协议 |

---

## 3. 决策

采用 **方案 C：在 `xtask` 中原生实现 `cargo xtask bundle`**（结合 macOS 系统自带 `hdiutil` 与 Windows 预装 WiX Toolset `candle.exe`/`light.exe` 或 `wix.exe`）：

1. **第三方协议自动生成**：`cargo xtask bundle` 首先调用 `cargo metadata --format-version 1` 提取所有非工作区第三方 crate 的名称、版本号、SPDX 许可证与仓库地址，生成确定性排序的 `assets/THIRD_PARTY_LICENSES.txt`（并在“关于 Ordia”界面内嵌展示）。
2. **macOS `.app` + `.dmg`**：组装 `target/bundle/macos/Ordia.app/Contents/{MacOS,Resources,Info.plist}`，注入当前版本号、拷贝 `assets/icons/Ordia.icns` 与 `THIRD_PARTY_LICENSES.txt`，建立 `/Applications` 符号链接，调用 `/usr/bin/hdiutil create -format UDZO` 输出 `target/bundle/Ordia-<version>-macos-<arch>.dmg`。
3. **Windows `.wxs` + `.msi`**：自动渲染确定性 GUID 的 WiX 安装清单 `target/bundle/windows/Ordia.wxs`（注册程序文件、开始菜单快捷方式及 `.xlsx`/`.xlsm`/`.csv`/`.tsv` 文件关联），在 Windows 主机调用 WiX Toolset 编译输出 `target/bundle/Ordia-<version>-windows-<arch>.msi`；在非 Windows 主机仍生成并校验 `.wxs` 清单以便跨平台审查。

---

## 4. 影响与后续演进

- CI 发布工作流（`.github/workflows/release.yml`）无需额外 `cargo install` 步骤，构建耗时显著缩短且无外部下载源抖动风险。
- 在 `M5-01` 接入 Apple 开发者证书时，只需在 `xtask/src/bundle.rs` 的 `.app` 组装完成与 `hdiutil create` 之间按环境变量 `APPLE_SIGNING_IDENTITY` 启用签名与公证步骤即可平滑演进。
