<div align="center">

<img src="src-tauri/icons/icon.png" width="256" height="256" alt="投影文件管理器图标">

# 投影文件管理器 / Schematic Manager

**跨平台的 Minecraft 投影（`.litematic`）文件管理与预览工具。**

[English](README.en_US.md) | 简体中文

</div>

<img width="1560" height="980" alt="2026-10-12_010941" src="https://github.com/user-attachments/assets/d7855b1d-78b4-4957-9537-6c146bfee77f" />

> 本程序仅限于免费分享，禁止任何销售行为。
> 作者：黑山大叔 — https://space.bilibili.com/19428259

## 功能

| 功能 | 说明 |
|---|---|
| 投影列表 | 选定目录后递归扫描其中的 `.litematic`，显示尺寸、方块数、制作版本等概要 |
| 3D 预览 | 用真实方块模型与贴图渲染投影，支持多视角切换与导出透明背景 PNG；单个方块超过 3 万个时默认不渲染，需在预览区点「开始渲染」（离开该文件后要重新确认） |
| 方块统计 | 按**建造所需的物品**统计数量，口径与投影模组 Litematica 的材料清单一致：先把方块状态折算成物品（火把与墙上的火把算一种、水与岩浆只算源头一个桶、门与床只算一次、双层台阶算两个、雪层/海龟蛋/海泡菜/蜡烛按状态里的数量、发光地衣按面数、带花的花盆算花盆加植物），再按物品累加；状态栏写明合计了几种状态与形态，点行设为替换源时会说明这一行覆盖了哪些方块名 |
| 方块替换 | 按规则把一种方块替换为另一种；窗口分左右两栏，左栏填规则（源方块、属性限定、目标方块、属性覆盖），右栏可搜索并点选方块填入当前那一栏，源方块与目标方块都显示中文名，还没填的必填栏标红；替换前自动备份，写回后可用备份还原 |
| 版本转换 | 把投影转换到另一个游戏版本：先分析差异，再为缺失的方块指定替代品；窗口分左右两栏，左栏列出缺口，输入框红框=待指定、绿框=自动识别的推荐、用户自己选过的不上色，右栏可搜索并点选方块填入（目标版本里没有的方块会标注且不能选），结果另存为新文件 |
| 重复文件筛查 | 就地筛查已加载的目录（含子目录）：内容逐字节相同的列为**重复**，排到列表最前、按组同色标记，并默认勾选多余的副本；名称、尺寸、方块数相同而内容不同的列为**疑似重复**，只提示、不勾选 |
| 备份与日志 | 列出每次改动前的备份，可还原或删除；另附运行日志页签（含「日志管理」窗口入口） |
| 日志管理 | 查看运行日志（方块替换、版本转换、重命名、复制、删除与备份还原的记录），可切换等级（错误/警告/提示/调试）、刷新、清空，或在文件管理器里打开日志所在目录；日志文件超过 5MB 或每次重新打开管理器时滚动一次，旧内容归档为 `YYYY-MM-dd-<滚动次数>.log.gz`（滚动次数是全局递增的序号，取已有归档的最大值加一）；日志放在应用数据目录下的 `logs/`（Windows 上即 `%APPDATA%\io.github.heiheids.schematic-manager\logs`）；入口在备份与日志的「运行日志」页签里 |
| 元数据编辑 | 修改投影的名称、作者与描述 |
| 更新 | 显示当前版本、作者与作者主页（可点击跳转），点「检测更新」读取 GitHub 上的发布列表并判断有无新版本（含预发布），结果分「有更新 / 已是最新 / 没查到发布 / 检测失败」四种如实显示；下载地址可点击复制，也可用「打开下载地址」交给浏览器打开 |
| RMC投影站 | 顶部预留的入口，供附加程序接入；接入前点击不做任何事 |

投影文件本身在改动前都会先备份，备份位于程序目录下的 `backups/`。

## 关于贴图与方块库

方块库与实体贴图库不随安装包分发：首次启动时如果库里一个版本都没有，
程序会弹出「版本库」引导补齐。它按数据版本逐个构建单版本包，存放在
应用数据目录的 `base/` 下（Windows 为
`%APPDATA%\io.github.heiheids.schematic-manager\base`）。

补库有两条来源：**本机扫描**（选任意目录，含子目录一起递归搜索其中的客户端
jar，扫到即自动导入并标为已就绪，不耗流量）与**在线下载**（官方接口或
BMCLAPI，供清单里没有、本机也没装的版本）。支持补齐的正式版共 49 个，从
1.13.2 到 26.3；一个版本构建完成后即可用于方块检索、3D 预览与统计，之后也能
在「版本库」里重建。

构建时还会顺带把该版本的语言文件与贴图一起收进库里：客户端 jar 只带
英文语言文件，**中文取自 `.minecraft/assets` 的资源库**，因此中文名能否
显示取决于本机资源是否完整；资源缺失时会回落到英文名。动画贴图只取第一帧
作静态展示，不做逐帧动画。

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端 | Vite + Vue 3 + TypeScript |
| 样式 | Sass |
| 组件库 | Arco Design Vue |
| 后端 | Rust（除压缩外，`.litematic`、NBT、ZIP、PNG 的解析全部自行实现） |

## 平台支持

| 操作系统 | x86_64 | arm64 |
|---|---|---|
| Windows | ✅ | ✅ |
| Linux | ✅ | ✅ |
| macOS | ✅ | ✅ |

## 故障排查

**双击后没有任何反应？**

程序启动失败时会在两个位置留一份 `启动失败.log`，并弹出提示窗口：

1. 程序所在目录（默认 `%LOCALAPPDATA%\schematic-manager\`）
2. 若上处不可写，则退到系统临时目录

日志里含失败原因、出错位置与程序路径。

**提示「WebView2 错误」或「拒绝访问」**

程序界面依赖系统的 Microsoft Edge WebView2 运行时（Windows 11 自带）。
遇到这类错误时，多为其用户数据目录损坏，可删除后重试：

```
%LOCALAPPDATA%\io.github.heiheids.schematic-manager
```

若仍无法启动，请安装 WebView2 运行时：
<https://developer.microsoft.com/microsoft-edge/webview2/>

**无人值守环境**

环境变量 `SCHEMATIC_MANAGER_NO_ERROR_DIALOG=1` 可让启动失败只写日志、
不弹窗。适用于自动化脚本或计划任务——弹窗是模态的，无人点击时会一直
等待。默认不设置，普通用户不受影响。

## 项目结构

- `src/` —— Vue 前端
- `src-tauri/` —— Rust 后端
  - 方块库与实体贴图库不随包分发：首次运行时由「版本库」界面引导扫描本机目录
    （含子目录，扫到即导入）或经官方/BMCL API 下载客户端 jar，在 app 数据目录
    生成基础库
  - `src-tauri/tests/data/` —— 测试用的真实 `.litematic` 样本
- `legacy-cpp/` —— 原有的 Windows 专用 C++ 实现，**仅作参考**。
  其中的 `.litematic` 解析、NBT 位打包、ZIP/PNG 解码、多版本方块库烘焙等
  已通过实测验证的语义，是 Rust 重写时的对照依据。
- `tools/gen_app_icons.py` —— 由 `docs/` 下两张母版生成整套应用图标
- `tools/typecheck/` —— 无网络环境下的离线类型检查与单元测试脚本
- `tools/render/` —— 用无头浏览器渲染构建产物、核对界面确实画得出来
- `docs/` —— 技术文档与图标母版
- `.github/workflows/ci.yml` —— 多平台质量检查、自动构建与发布

## 命名

安装包名、安装目录、可执行文件名与包标识符统一用 ASCII 的
`schematic-manager`（见 `src-tauri/tauri.conf.json` 的 `productName` 与
`identifier`）；面向用户的显示名 **投影文件管理器** 由各处单独指定：

- 窗口标题：`tauri.conf.json` 的 `app.windows[0].title`，标题栏另有一份
- macOS 的 Finder 名称与菜单栏名称：`src-tauri/Info.plist`、
  `bundle.macOS.bundleName`
- Linux 桌面项名称：`src-tauri/linux/main.desktop`
- 启动失败提示：`src-tauri/src/startup.rs`

Windows 上不额外指定：安装器把开始菜单与桌面快捷方式、以及「应用和功能」
里的名称全部取自 `productName`，Tauri 没有把它们与其分开的配置项，因此这些
位置显示的是 `schematic-manager`；窗口标题与界面里的文字仍是中文。

Linux 上 deb 与 rpm 都用 `src-tauri/linux/main.desktop` 作为桌面项模板；
AppImage 没有对应的模板配置项，若打出 AppImage，应用菜单里显示的是
`schematic-manager`。

包标识符与 `productName` 同时变更会让系统把新版本视为另一个应用：Windows 上
旧的安装不再被识别为可升级（卸载项与安装目录都换了名字），升级前需要先卸载
旧版本；应用数据目录由程序在启动时自动搬迁。

## 构建

需要 Node.js（含 pnpm）与 Rust 工具链。

```bash
pnpm install
pnpm tauri dev        # 开发（起 dev server + 应用窗口）
pnpm app:build        # 出 Windows 安装包（与 CI 一致，用 nsis）
pnpm tauri build      # 按 tauri.conf.json 的 targets 打包
```

**`pnpm app:build` 而不是 `cargo build --release`。** 后者只编译 Rust，
**不会嵌入前端**：`dist/` 没被打进去，产物里留着 `devUrl`
（`http://localhost:1420`），双击后 WebView2 去连一个并不存在的开发服务器，
界面显示 `ERR_CONNECTION_REFUSED`。需要嵌入前端时必须经 Tauri CLI 构建，
它会在编译前先跑 `beforeBuildCommand`（`pnpm build`）并把 `frontendDist`
打进二进制。这是我实际踩过的坑。

另外 `pnpm tauri build` 会按 `targets: "all"` 试所有格式。MSI 曾经必然失败：
productName 是中文时，WiX 的 light.exe 按 `SummaryCodepage`（en-US，1252）
编码 `Product/@Name`，报 `LGHT0311`。productName 改成 ASCII 之后 MSI 实测能
产出，但发布链路还没把 `.msi` 纳入产物分类，所以 CI 仍只用 `nsis`；本地要出
与 CI 一致的包就走 `pnpm app:build`（等价于 `tauri build --bundles nsis`）。

前端类型检查、单元测试与构建：

```bash
pnpm typecheck       # vue-tsc
pnpm test            # vitest：API 契约 + 界面挂载与交互流程
pnpm check:ipc       # 前后端命令名与参数名的一致性检查
pnpm check:notes     # Release 说明的格式（文件表与下载链接）
pnpm check:workflow  # 工作流结构：needs / outputs / 矩阵键 / 表达式 / concurrency
pnpm build           # vite build
```

`pnpm check:notes` 与 `pnpm check:workflow` 都不联网、也不需要 token，
不受仓库当前 Release 状态影响，可在本地随时运行。

`check:workflow` 补的是「YAML 能解析 ≠ GitHub 能执行」这段差距：
`needs` 指向不存在的任务、引用未声明的 output、矩阵键没定义、
表达式括号不配对、矩阵里未加引号的逗号（会被当成条目分隔符）——
这些只在推送之后才由 GitHub 报出来，而本工作流的构建阶段要跑六个平台。

后端的单元测试：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

仓库内只随源码分发一个小体积样本（见 `src-tauri/tests/data/README.md`）。
若要拿自己收藏的真实投影核验各功能的覆盖面，用这几个按需运行的用例：

```bash
SM_REAL_DIR=/path/to/projects \
  cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored --nocapture
```

对目录下每个真实投影依次核对：

- 载入、区域结构自检、NBT 往返零信息损失、方块统计
- 用随包分发的方块库构建 3D 场景，并报出未能解析的方块数
- 方块替换：替换 → 写出 → 读回，核对体积与方块总数不变
- 版本转换：转换 → 写出 → 读回，核对 dataVersion 已改写且体积不变
- 查重：副本被判为重复、内容不同的文件不被误判
- 备份还原：备份 → 真的改写文件 → 还原，核对逐字节回到原样

未设置 `SM_REAL_DIR` 时会直接失败，不会静默跳过。
这些用例会在系统临时目录下建工作目录；临时目录不可写时
需先把 `TEMP`/`TMP` 指向可写位置（`tools/typecheck/run.ps1` 亦如此）。

界面渲染检查（需先 `pnpm build`，且本机装有 Chrome / Edge / Chromium）：

```bash
python tools/render/check-render.py
```

它用无头浏览器加载 dist 产物、自动走一遍真实操作路径
「添加目录 → 扫描 → 选中文件 → 逐个打开五个功能对话框」，
再从渲染结果里核对界面文案。单元测试跑在 jsdom 里，看不到真实浏览器中的
渲染：构建配置写错时 `vite build` 依旧成功、dist 也生成了，打开却是一片
空白。这一步专门守这种情况。

对话框单独验：它们打开时会各自加载数据（可转换版本、版本库清单、备份列表），
一旦抛错，界面表现就是「点了没反应」——最难排查的一类问题。
浏览器路径可用 `CHROME_PATH` 指定。

## 版本号规则

- 开发构建（预发布）：`v<MAJOR>.<MINOR>.<PATCH>-dev.<构建次数>`，推送 main 自动生成
- 正式版：`v<MAJOR>.<MINOR>.<PATCH>`，推送同版本号的 tag 触发；tag 版本必须与
  仓库内版本号一致，不一致时工作流直接失败

正式发布后，工作流自动把仓库内版本号递增到下一 PATCH 并推回 main。
递增以**所打的 tag** 为基准，且带幂等守卫：清单版本已经不是已发布版本时
（重跑或人工已递增过）跳过，不会重复递增或倒退。

仓库内的版本号共四处，由 `scripts/bump-version.mjs` 一并改写：
`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`
与 `src-tauri/Cargo.lock` 里本包那一条。

各平台的产物格式：Windows 为 NSIS 安装包（`-setup.exe`），macOS 为 DMG，
Linux 为 deb。

推送 main 触发多平台构建并以预发布形式发布；pull request 与手动触发只运行
质量检查，不产生 Release。Release 说明（下载表 + What's Changed 提交列表）
由 `scripts/release-notes-render.mjs` 自动生成。

## 许可证

本项目以 [GNU Affero General Public License v3.0](LICENSE)（AGPL-3.0-only）
发布。修改后对外提供（含网络方式）时必须公开对应的源代码。
