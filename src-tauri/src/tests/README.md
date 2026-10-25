# 单元测试包

本目录存放 Rust 侧的单元测试，按 `src-tauri/src/` 的包结构一一对应分文件
（例如 `src/block/db.rs` 的测试在 `src/tests/block/db.rs`）。

当前规模：46 个测试文件、607 项测试，其中 21 项标了 `#[ignore]` 按需运行。

测试文件本身是各模块 `mod tests` 的本体：源码里只保留一处挂载声明，

```rust
#[cfg(test)]
#[path = "../tests/block/db.rs"]
mod tests;
```

因此测试仍是该模块的子模块，可以访问私有项；所有测试随
`cargo test --manifest-path src-tauri/Cargo.toml --lib` 一起运行，不需要额外的测试 target。

- `support.rs` 挂载为 `crate::test_support`，放造内存 zip/PNG、按用例隔离的临时目录等共用辅助。
- `real_project.rs` 是按需运行的端到端核验（用例都标了 `#[ignore]`），用 `-- --ignored` 运行。
- `data/` 是测试用的真实 `.litematic` 样本。

新增测试时：在 `src/tests/` 下按被测模块的路径新建文件，写入 `use super::*;` 与测试函数，
再在该模块末尾加上述挂载声明即可。
