# 离线类型检查

本目录是一套**离线类型检查工具**，用于在没有网络、无法使用 `cargo`（拉不到
crates.io 依赖）的环境下，检查 Rust 源码是否存在真实编译错误。

## 为什么需要它

项目的 Rust 侧依赖 `tauri`、`serde`、`flate2`、`thiserror`、`ureq` 等 crate。
若本机无法访问 crates.io，`cargo check` 直接失败，导致**没有任何编译反馈通道**，
只能靠人工审读——而人工审读漏掉的错误会一直累积到 CI 才爆发。

类型检查本身不需要真实依赖实现，只需要**接口形状**。因此这里用最小桩替代依赖，
再用 `rustc` 直接检查源码。

## 用法

```powershell
# 本机没有 pwsh 时用 Windows PowerShell：
powershell -NoProfile -ExecutionPolicy Bypass -File tools/typecheck/run.ps1
```

退出码为错误数量，可直接用于脚本判断。

## 覆盖范围

通过 `#[path]` 引用 `src-tauri/src` 下的**真实文件**，按源码的包结构挂载：

- `block/`、`format/`、`pack/`、`render/`、`schematic/` 整包挂载；
- `app/` 与 `mc/` 中不依赖 tauri / ureq 的模块逐个挂载；
- `src/tests/` 下的测试文件经各模块末尾的 `#[cfg(test)] #[path]` 自动跟随，
  不需要逐文件登记（`run.ps1` 会核对每个源码模块都被某个挂载覆盖）；
- `commands/` 由 `gen_cmd_check.py` 生成剥掉过程宏的副本后检查，测试挂载
  指向 `src/tests/commands/` 下的真实测试文件。

桩位于同目录：`flate2.rs`、`serde_stub_crate.rs`、`serde_json_stub_crate.rs`、
`tauri_stub_crate.rs`、`ureq_stub_crate.rs`。其中 serde_json 桩实现了真正的
JSON 解析（blockstate 解析测试要在本地跑），tauri / ureq 桩只提供签名。

## 局限（务必了解）

- **不是完整构建**：只做类型检查并用桩运行测试，不能替代 CI 上的真实依赖构建。
- **命令层只检查函数体**：`#[tauri::command]` 的宏展开（参数名映射、返回值
  序列化）与 serde 的字段重命名 / 序列化行为无法用桩验证；DTO 里依赖
  `rename_all` 的 3 个测试在本地注定失败，`run.ps1` 按已知项过滤，CI 用真实
  serde 覆盖。
- **桩只提供被用到的 API 表面**：源码新用其它接口时要同步补桩，否则会报
  “方法不存在”的假错误。
- 真正的验证仍是 CI 上的完整构建与测试。
