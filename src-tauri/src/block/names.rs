//! 方块库语言表里的名称解析：方块、实体与方块实体。
//!
//! 名称来自打包时抓进库的语言表，按版本保存；查询时先选对该版本的表，
//! 缺的一侧再从整个语言表池里补。放在方块库一侧，是因为它只依赖
//! `BlockDb`，渲染与命令层都只是使用者。

use crate::block::db::BlockDb;

/// 在语言表里查一个键，返回 (中文, 英文)；查不到返回两个空串。
///
/// 语言表按版本保存（`VersionDef::lang_idx` 指向 `db.langs` 的一张表），
/// 因此必须先选对版本，否则查到的名字与投影里的方块对不上。
///
/// **中英文必须分别补齐**，不能一命中就返回：打包时只有一部分版本抓到了
/// 官方中文语言文件（实测 1.13.2~1.21.11 里约一半的表只有英文，
/// 用户那个 1.21.5 对应的表恰恰一条中文都没有）。早期写法在本版本的
/// 条目只有英文时直接返回，于是中文恒为空——这是实现者自己的缺陷，
/// 不是数据缺失。
///
/// 缺的一侧从整个语言表池里找同名的键补上：方块名跨版本相当稳定，
/// 与贴图解析的处理一致（见 `BlockDb::texture_pool_index` 的全局回退）。
pub(crate) fn lookup_lang(db: &BlockDb, version_idx: Option<usize>, key: &str) -> (String, String) {
    let mut zh = String::new();
    let mut en = String::new();

    // 本版本的表优先：它的命名最贴合该版本
    if let Some(vi) = version_idx {
        if let Some(version) = db.versions.get(vi) {
            if let Some(table) = db.langs.get(version.lang_idx as usize) {
                if let Some(e) = table.iter().find(|e| e.block_name == key) {
                    zh = e.zh.clone();
                    en = e.en.clone();
                }
            }
        }
    }
    if !zh.is_empty() && !en.is_empty() {
        return (zh, en);
    }

    // 缺哪一侧就补哪一侧
    for table in &db.langs {
        for e in table.iter().filter(|e| e.block_name == key) {
            if zh.is_empty() && !e.zh.is_empty() {
                zh = e.zh.clone();
            }
            if en.is_empty() && !e.en.is_empty() {
                en = e.en.clone();
            }
            if !zh.is_empty() && !en.is_empty() {
                return (zh, en);
            }
        }
    }
    (zh, en)
}

/// 查方块的中英文名。
///
/// 方块语言表的键有两种写法：老版本直接用方块名（`minecraft:stone`），
/// 新版本用 `block.minecraft.stone`。两种都试，避免因版本差异整列空着。
pub(crate) fn block_display_names(db: &BlockDb, version_idx: Option<usize>, name: &str) -> (String, String) {
    let (zh, en) = lookup_lang(db, version_idx, name);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    if let Some(short) = name.strip_prefix("minecraft:") {
        let dotted = format!("block.minecraft.{short}");
        let (zh, en) = lookup_lang(db, version_idx, &dotted);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}

/// 查实体的中英文名。实体 id 形如 `minecraft:zombie`，键是 `entity.minecraft.zombie`。
pub(crate) fn entity_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    // 投影里的 id 可能不带命名空间，补上再查
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    let dotted = format!("entity.{}", full.replace(':', "."));
    let (zh, en) = lookup_lang(db, version_idx, &dotted);
    if !zh.is_empty() || !en.is_empty() {
        return (zh, en);
    }
    // 有些语言表直接以 id 为键
    lookup_lang(db, version_idx, &full)
}

/// 查方块实体（箱子、告示牌等）的中英文名。
///
/// 它们与实体共用 `entity.*` 命名空间，但方块实体在语言表里也有
/// `block.*` 的别名，因此两种都试。
pub(crate) fn tile_display_names(db: &BlockDb, version_idx: Option<usize>, id: &str) -> (String, String) {
    let full = if id.contains(':') { id.to_string() } else { format!("minecraft:{id}") };
    for key in [
        format!("block.{}", full.replace(':', ".")),
        format!("entity.{}", full.replace(':', ".")),
        full.clone(),
    ] {
        let (zh, en) = lookup_lang(db, version_idx, &key);
        if !zh.is_empty() || !en.is_empty() {
            return (zh, en);
        }
    }
    (String::new(), String::new())
}
