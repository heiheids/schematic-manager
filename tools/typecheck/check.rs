//! 仅用于本机类型检查，不参与实际构建。
//!
//! 按 `src-tauri/src` 的包结构挂载模块：包的 `mod.rs` 会自行带上子模块，而各模块
//! 末尾的 `#[cfg(test)] #[path = "../tests/…"] mod tests;` 会把 `src/tests` 下的
//! 测试文件一并带进 `--test` 构建，因此这里不需要逐文件登记模块或测试。
//!
//! 不含 commands / lib / main：commands 依赖 tauri 的过程宏，由 check_cmds 覆盖。
//! app 的 data_dir/update 与 mc 的 download 依赖 tauri / ureq，用同目录的桩替代。
//!
//! 本 crate 只含纯逻辑模块，编译器会把「被 commands 使用」的项误报为 dead_code，
//! 这里关掉该 lint —— 本 crate 的职责是类型检查，不是死代码分析。
#![allow(dead_code)]

// error 与 flate2 用桩替代
#[path = "error_stub.rs"] mod error;

#[path = "../../src-tauri/src/block/mod.rs"] mod block;
#[path = "../../src-tauri/src/format/mod.rs"] mod format;
#[path = "../../src-tauri/src/pack/mod.rs"] mod pack;
#[path = "../../src-tauri/src/render/mod.rs"] mod render;
#[path = "../../src-tauri/src/schematic/mod.rs"] mod schematic;

mod app {
    #[path = "../../../src-tauri/src/app/backup.rs"] pub mod backup;
    #[path = "../../../src-tauri/src/app/file_ops.rs"] pub mod file_ops;
    #[path = "../../../src-tauri/src/app/open_url.rs"] pub mod open_url;
    #[path = "../../../src-tauri/src/app/run_log.rs"] pub mod run_log;
    #[path = "../../../src-tauri/src/app/startup.rs"] pub mod startup;
}

mod mc {
    #[path = "../../../src-tauri/src/mc/download.rs"] pub mod download;
    #[path = "../../../src-tauri/src/mc/ver.rs"] pub mod ver;
}

#[cfg(test)]
#[path = "../../src-tauri/src/tests/support.rs"]
pub(crate) mod test_support;

// 真实投影的端到端核验。用例都标了 #[ignore]，这里的价值是把它们编译一遍。
#[cfg(test)]
#[path = "../../src-tauri/src/tests/real_project.rs"]
mod real_project;

fn main() {}
