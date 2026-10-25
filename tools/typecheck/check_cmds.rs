//! 命令层与 DTO 的离线类型检查。
//!
//! 命令层使用 tauri 的过程宏（`#[tauri::command]`）与 serde 的派生宏，
//! 而过程宏无法用普通 Rust 源码替代——但风险恰在函数体（跨模块接口调用、
//! DTO 转换），那部分剥掉属性后仍会被完整检查。
//!
//! 做法：gen_cmd_check.py 生成剥离宏的副本，用独立 crate 形式的 tauri /
//! serde / ureq / flate2 桩替代依赖，再把全部业务包挂进来编译。
//!
//! 覆盖不到的部分（明确记录，不假装覆盖）：
//!   - #[tauri::command] 宏展开（参数名映射、返回值序列化）
//!   - serde 的字段重命名与序列化行为
//!   - lib.rs 的 generate_handler! 列表，由 CI 的 cargo 构建覆盖
#![allow(dead_code)]
#![allow(unused_imports)]

#[path = "error_stub.rs"] mod error;

#[path = "../../src-tauri/src/block/mod.rs"] mod block;
#[path = "../../src-tauri/src/format/mod.rs"] mod format;
#[path = "../../src-tauri/src/pack/mod.rs"] mod pack;
#[path = "../../src-tauri/src/render/mod.rs"] mod render;
#[path = "../../src-tauri/src/schematic/mod.rs"] mod schematic;

mod app {
    #[path = "../../../src-tauri/src/app/backup.rs"] pub mod backup;
    #[path = "../../../src-tauri/src/app/data_dir.rs"] pub mod data_dir;
    #[path = "../../../src-tauri/src/app/file_ops.rs"] pub mod file_ops;
    #[path = "../../../src-tauri/src/app/open_url.rs"] pub mod open_url;
    #[path = "../../../src-tauri/src/app/run_log.rs"] pub mod run_log;
    #[path = "../../../src-tauri/src/app/startup.rs"] pub mod startup;
    #[path = "../../../src-tauri/src/app/update.rs"] pub mod update;
}

mod mc {
    #[path = "../../../src-tauri/src/mc/download.rs"] pub mod download;
    #[path = "../../../src-tauri/src/mc/ver.rs"] pub mod ver;
}

// 待检查的命令层（生成副本，宏属性已剥离，测试挂载指向真实测试文件）
#[path = "generated/commands/mod.rs"] mod commands;

#[cfg(test)]
#[path = "../../src-tauri/src/tests/support.rs"]
pub(crate) mod test_support;

fn main() {}
