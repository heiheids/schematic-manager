//! 仅用于本机类型检查，不参与实际构建。
//!
//! 注意：本 crate 只包含纯逻辑模块，不含 commands.rs / dto.rs，
//! 因此编译器会把「被 commands 使用」的项误报为 dead_code。
//! 这里关掉该 lint —— 本 crate 的职责是类型检查，不是死代码分析。
#![allow(dead_code)]
//! 通过 #[path] 引用 src-tauri/src 下的真实文件；error 与 flate2 用桩替代。
#[path = "error_stub.rs"] mod error;
#[path = "../../src-tauri/src/duplicate.rs"] mod duplicate;
#[path = "../../src-tauri/src/mesh.rs"] mod mesh;
#[path = "../../src-tauri/src/model_pack.rs"] mod model_pack;
#[path = "../../src-tauri/src/nbt.rs"] mod nbt;
#[path = "../../src-tauri/src/png.rs"] mod png;
#[path = "../../src-tauri/src/pool.rs"] mod pool;
#[path = "../../src-tauri/src/mcver.rs"] mod mcver;
#[path = "../../src-tauri/src/jar_file.rs"] mod jar_file;
#[path = "../../src-tauri/src/jar_index.rs"] mod jar_index;
#[path = "../../src-tauri/src/litematic.rs"] mod litematic;
#[path = "../../src-tauri/src/blockdb.rs"] mod blockdb;
#[path = "../../src-tauri/src/blockdb_write.rs"] mod blockdb_write;
#[path = "../../src-tauri/src/block_model.rs"] mod block_model;
#[path = "../../src-tauri/src/block_search.rs"] mod block_search;
#[path = "../../src-tauri/src/blockstate.rs"] mod blockstate;
#[path = "../../src-tauri/src/blockstate_chain.rs"] mod blockstate_chain;
#[path = "../../src-tauri/src/geometry.rs"] mod geometry;
#[path = "../../src-tauri/src/convert.rs"] mod convert;
#[path = "../../src-tauri/src/pack_build.rs"] mod pack_build;
#[path = "../../src-tauri/src/pack_registry.rs"] mod pack_registry;
#[path = "../../src-tauri/src/replace.rs"] mod replace;
#[path = "../../src-tauri/src/scene.rs"] mod scene;
// 方块染色（草/树叶/藤蔓/红石/水的着色系数）
#[path = "../../src-tauri/src/tint.rs"] mod tint;
#[path = "../../src-tauri/src/transform.rs"] mod transform;
#[path = "../../src-tauri/src/visibility.rs"] mod visibility;
#[path = "../../src-tauri/src/zip.rs"] mod zip;
#[path = "../../src-tauri/src/backup.rs"] mod backup;
#[path = "../../src-tauri/src/startup.rs"] mod startup;
// 运行日志：只依赖 std 与 flate2（桩里已有 gzip），因此能离线编译并跑它的
// 等级门控、转义、滚动与编号等用例。
#[path = "../../src-tauri/src/run_log.rs"] mod run_log;
// 目录栏的文件操作（删除/复制/打开文件夹）。
// Windows 实现用 `#[link]` 直接声明系统函数，不依赖外部 crate，
// 因此这里也能离线编译并跑它的单元测试。
#[path = "../../src-tauri/src/file_ops.rs"] mod file_ops;
// 真实投影的端到端核验。用例都标了 #[ignore]，这里的价值是把它们**编译**
// 一遍：写错字段名或签名时，离线也能发现，不必等有 cargo 依赖时才暴露。
#[path = "../../src-tauri/src/real_project.rs"] mod real_project;
