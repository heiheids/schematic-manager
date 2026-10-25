//! 从 Minecraft 客户端/模组 jar 构建内部资源包。
//!
//! 串联各层：jar 索引 -> blockstate 解析 -> 模型继承链 -> 模型转换
//! -> 贴图解码与去重 -> 组装方块库 -> 序列化。
//! 对照 legacy-cpp/packbuild.cpp 的 BuildFromJar 主流程。
//!
//! 不直接接触文件系统：内容通过 `JarSource` 提供，因此可以用内存中的
//! 假 jar 完整测试整条链路，不必依赖真实的客户端 jar。

use crate::block::db::{BlockDb, BlockDef, LangEntry, Model, TextureInfo, Variant, VersionDef};
use crate::block::state::parse_blockstate;
use crate::error::{AppError, AppResult};
use crate::pack::index::{self, JarIndex};
use crate::block::state_chain::{build_chain, Chain};
use crate::block::pool::{model_key, ContentPool};
use std::collections::HashMap;

/// jar 内容来源
pub trait JarSource {
    fn entries(&self) -> Vec<String>;
    fn read(&self, name: &str) -> AppResult<Vec<u8>>;

    /// jar 之外**额外**提供的语言文件路径（形如 `assets/minecraft/lang/zh_cn.json`）。
    ///
    /// 官方客户端 jar 里只有 `en_us.json`，中文等语言文件随资源索引下载后存在
    /// `.minecraft/assets/objects` 里，不在 jar 内。缺了中文，方块列表与统计表
    /// 的中文名整列都是空的，因此调用方可以把这些文件按同样的路径喂进来。
    fn extra_lang_paths(&self) -> Vec<String> {
        Vec::new()
    }

    /// 读一个额外语言文件；没有时返回 None，由调用方回落到 jar 内读取
    fn read_extra_lang(&self, _name: &str) -> Option<Vec<u8>> {
        None
    }
}

/// 构建结果统计
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildStats {
    pub namespaces: usize,
    pub blocks: usize,
    pub models: usize,
    pub textures: usize,
    /// 方块名语言表条目数（中英文名）
    pub langs: usize,
    /// 解析失败而被跳过的 blockstate 数
    pub failed_blockstates: usize,
    /// 因无法解析模型而被跳过的方块数
    pub failed_blocks: usize,
}

/// 构建产物
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltPack {
    pub data: Vec<u8>,
    pub stats: BuildStats,
}

/// 贴图池的一项
#[derive(Debug, Clone, PartialEq, Eq)]
struct TexEntry {
    w: u16,
    h: u16,
    rgba: Vec<u8>,
    has_alpha: bool,
}

/// 字符串池：保持插入顺序并记录下标
#[derive(Default, Debug)]
struct StringPool {
    items: Vec<String>,
    index: HashMap<String, u32>,
}

impl StringPool {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.index.get(s) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(s.to_string());
        self.index.insert(s.to_string(), i);
        i
    }
    fn get(&self, s: &str) -> Option<u32> {
        self.index.get(s).copied()
    }
}

/// 从 jar 里读语言文件，建出方块中英文名表。
///
/// 基础库在用户机器上构建，语言表只能从客户端 jar 现取：客户端 jar 带着
/// `assets/minecraft/lang/<语言>.json`（模组 jar 在各自命名空间下同理）。
/// 少了这张表，方块列表与统计表的中文名整列都是空的——旧版随包分发的
/// `blocks.dat.z` 由离线流水线补过这张表，运行时构建必须自己补上。
///
/// 键名形如 `block.minecraft.stone`、`item.minecraft.water_bucket`，名字取
/// 「命名空间 + 余下部分」拼成 `minecraft:stone`，与语言表查询侧的写法一致
///（见 block::names::lookup_lang）。余下部分含点号的键（例如
/// `block.minecraft.banner.base.black` 这类图案名）不是方块或物品名，跳过。
///
/// 物品键也要收：统计按「建造所需的物品」折算后，有些行代表的是物品而不是
/// 方块（水与岩浆折成水桶/岩浆桶），只收方块键会让这些行没有中文名。
/// 方块键与物品键同名时（例如 `oak_slab`）以方块键为准，两个键的译名一致，
/// 但方块键更贴合「方块统计」的语境。
fn load_lang_table<S: JarSource>(src: &S) -> Vec<LangEntry> {
    use std::collections::BTreeMap;

    // 语言文件按命名空间分目录，先收集「哪些命名空间提供了哪种语言」。
    // jar 内与 jar 外（.minecraft/assets 的对象库）都要看：官方客户端 jar
    // 只带 en_us.json，中文要靠 jar 外那一份。
    let mut found: BTreeMap<String, BTreeMap<&str, String>> = BTreeMap::new();
    for name in src.entries().into_iter().chain(src.extra_lang_paths()) {
        let mut it = name.split('/');
        if it.next() != Some("assets") {
            continue;
        }
        let Some(ns) = it.next() else { continue };
        if it.next() != Some("lang") {
            continue;
        }
        let Some(file) = it.next() else { continue };
        let lang = match file {
            "en_us.json" => "en",
            "zh_cn.json" => "zh",
            _ => continue,
        };
        found
            .entry(ns.to_string())
            .or_default()
            .insert(lang, name.clone());
    }

    // 名字 -> (英文, 中文)；两边缺哪侧就留空，查询侧会用其它版本的表补
    let mut table: BTreeMap<String, (String, String)> = BTreeMap::new();
    for (ns, files) in &found {
        let mut texts: BTreeMap<&str, serde_json::Map<String, serde_json::Value>> =
            BTreeMap::new();
        for (lang, path) in files {
            // 语言文件坏了不该让整次构建失败：方块与贴图仍然可用，
            // 只是名字缺失，因此这里跳过并继续
            let raw = match src.read_extra_lang(path) {
                Some(v) => v,
                None => match src.read(path) {
                    Ok(v) => v,
                    Err(_) => continue,
                },
            };
            match serde_json::from_slice::<serde_json::Value>(&raw) {
                Ok(serde_json::Value::Object(map)) => {
                    texts.insert(lang, map);
                }
                _ => continue,
            }
        }
        let mut keys: Vec<&String> = texts.values().flat_map(|m| m.keys()).collect();
        keys.sort();
        keys.dedup();
        // 先物品后方块，同名的键由方块覆盖
        for prefix in [format!("item.{ns}."), format!("block.{ns}.")] {
            for key in &keys {
                let Some(path) = key.strip_prefix(&prefix) else { continue };
                if path.contains('.') {
                    continue;
                }
                let name = format!("{ns}:{path}");
                let entry = table.entry(name).or_default();
                for (lang, map) in &texts {
                    let Some(v) = map.get(*key).and_then(|x| x.as_str()) else { continue };
                    match *lang {
                        "en" => entry.0 = v.to_string(),
                        "zh" => entry.1 = v.to_string(),
                        _ => {}
                    }
                }
            }
        }
    }

    table
        .into_iter()
        .map(|(block_name, (en, zh))| LangEntry { block_name, en, zh })
        .collect()
}

/// 从 jar 构建资源包。
///
/// `data_version` 为 0 时尝试从 version.json 读取；模组包应显式给出，
/// 因为「叠加到哪个游戏版本」在 jar 里没有。
pub fn build<S, L>(
    src: &S,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
    mut load_texture: L,
) -> AppResult<BuiltPack>
where
    S: JarSource,
    L: FnMut(&str) -> Option<(u16, u16, Vec<u8>)>,
{
    let names = src.entries();
    let index = index::build(&names);
    if index.is_empty() {
        return Err(AppError::Parse(
            "jar 里没有找到任何资源（不是客户端 jar 或模组 jar？）".into(),
        ));
    }

    let (mut version_id, mut dv) = (String::new(), data_version);
    if let Ok(raw) = src.read("version.json") {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
            if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
                version_id = id.to_string();
            }
            if dv <= 0 {
                if let Some(n) = v.get("world_version").and_then(|x| x.as_i64()) {
                    // 用 try_from 而不是 `n as i32`：后者静默截断，而截断结果
                    // 可能落在一个**看似合法**的版本号上。例如
                    // 4294970761 as i32 == 3465，正好是 1.20.1 的编号，
                    // 于是损坏的版本号会被当成合法版本，把资源包归到错误的
                    // 游戏版本上，后续的版本匹配与转换目标都会跟着错。
                    // 超出 i32 时按「没有版本号」处理（dv 保持原值），
                    // 与字段缺失一致，界面显示为未知版本。
                    if let Ok(v32) = i32::try_from(n) {
                        dv = v32;
                    }
                }
            }
        }
    }
    if version_id.is_empty() {
        version_id = pack_name.to_string();
    }

    let mut stats = BuildStats::default();
    let mut strings = StringPool::default();
    let mut models: ContentPool<Model> = ContentPool::new();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    // 贴图路径 -> 贴图池下标
    let mut tex_by_path: HashMap<String, u32> = HashMap::new();
    let mut blocks: Vec<BlockDef> = Vec::new();

    let namespaces: Vec<String> = index
        .blockstate_namespaces()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    stats.namespaces = namespaces.len();
    if namespaces.is_empty() {
        return Err(AppError::Parse("jar 里没有找到任何 blockstates".into()));
    }

    for ns in &namespaces {
        let Some(assets) = index.namespaces.get(ns) else { continue };
        let bs_entries: Vec<String> = assets.blockstates.clone();

        for bs_entry in &bs_entries {
            let Ok(raw) = src.read(bs_entry) else {
                stats.failed_blockstates += 1;
                continue;
            };
            let Ok(def) = parse_blockstate(&raw) else {
                stats.failed_blockstates += 1;
                continue;
            };
            if def.is_empty() {
                stats.failed_blockstates += 1;
                continue;
            }
            let Some(block_name) = block_name_of(bs_entry) else {
                stats.failed_blockstates += 1;
                continue;
            };
            let qualified = format!("{ns}:{block_name}");

            // 先把本方块引用到的模型（含父链）全部解析出来。
            // 分两步是因为登记字符串与查表借用冲突：
            // 先得到「模型 -> 已解析的转换结果 + 用到的贴图路径」，
            // 再统一登记，避免边查边写。
            // ---- 两遍处理 ----
            // 第一遍：解析本方块引用到的全部模型（含父链），得到
            // 「模型链 + 转换所需信息」，并把用到的贴图路径登记进字符串池
            // 与贴图池。只有先登记完，第二遍转换才能查到下标。
            let mut resolved: Vec<(bool, crate::block::state::VariantEntry, Chain, crate::block::state::ModelDef)> =
                Vec::new();
            // 箱子一类方块的几何由游戏代码绘制，jar 里只有空壳模型，
            // 因此改由 pack::code_models 按方块名与状态给出几何；贴图仍取自 jar。
            for cv in crate::pack::code_models::variants_of(&qualified).unwrap_or_default() {
                let chain = Chain {
                    textures: std::collections::HashMap::new(),
                    elements: cv.elements.clone(),
                    force_translucent: false,
                };
                let mdef = crate::block::state::ModelDef {
                    elements: cv.elements,
                    ..Default::default()
                };
                for path in crate::pack::model_pack::collect_texture_paths(&chain, &chain.elements, ns) {
                    intern_texture(&path, src, ns, &mut strings, &mut textures, &mut tex_by_path, &mut load_texture);
                }
                resolved.push((
                    false,
                    crate::block::state::VariantEntry {
                        when: cv.when,
                        // 代码模型的几何不来自某个模型文件，这里留空；
                        // 第二遍只用到条件与旋转。
                        model: String::new(),
                        rot_x_div90: cv.rot_x_div90 as u8,
                        rot_y_div90: cv.rot_y_div90 as u8,
                        uvlock: false,
                    },
                    chain,
                    mdef,
                ));
            }
            for (is_multipart, list) in [(false, &def.variants), (true, &def.multipart)] {
                for v in list {
                    let Some((chain, mdef)) = load_model(src, &index, ns, &v.model) else {
                        continue;
                    };
                    let elements = if chain.elements.is_empty() {
                        &mdef.elements
                    } else {
                        &chain.elements
                    };
                    // 登记该模型用到的一切贴图
                    for path in
                        crate::pack::model_pack::collect_texture_paths(&chain, elements, ns)
                    {
                        intern_texture(&path, src, ns, &mut strings, &mut textures, &mut tex_by_path, &mut load_texture);
                    }
                    resolved.push((is_multipart, v.clone(), chain, mdef));
                }
            }
            if resolved.is_empty() {
                stats.failed_blocks += 1;
                continue;
            }

            // 第二遍：此时路径到字符串下标已完整，可以转换
            let mut variants: Vec<Variant> = Vec::new();
            let mut multipart: Vec<Variant> = Vec::new();
            for (is_multipart, v, chain, mdef) in &resolved {
                let Some(converted) = crate::pack::model_pack::convert_model(
                    mdef,
                    chain,
                    |face_ref| {
                        let path = crate::pack::model_pack::resolve_face_texture_path(chain, face_ref, ns)?;
                        let idx = strings.get(&path)?;
                        Some((idx, path))
                    },
                ) else {
                    continue;
                };
                let mi = models.intern(&model_key(&converted.model), converted.model);
                let entry = Variant {
                    when: v.when.clone(),
                    model_idx: mi as u32,
                    // blockstate 解析时已换算成 90 度倍数（见 get_rot_div90），
                    // 磁盘格式也是这个口径，直接写出即可。
                    // 曾经写成 `v.rot_x_div90 % 4`：在字段还是「角度」时，
                    // 90 被写成 2（读回 180 度）、180 被写成 0（读回 0 度）。
                    rot_x_div90: v.rot_x_div90 % 4,
                    rot_y_div90: v.rot_y_div90 % 4,
                    uvlock: v.uvlock,
                };
                if *is_multipart {
                    multipart.push(entry);
                } else {
                    variants.push(entry);
                }
            }
            if variants.is_empty() && multipart.is_empty() {
                stats.failed_blocks += 1;
                continue;
            }
            strings.intern(&qualified);
            blocks.push(BlockDef { name: qualified, variants, multipart });
        }
    }

    if blocks.is_empty() {
        return Err(AppError::Parse(
            "没有解析出任何方块（jar 内的 blockstates 都无法解析？）".into(),
        ));
    }

    stats.blocks = blocks.len();
    stats.models = models.len();
    stats.textures = textures.len();

    // ---- 组装方块库 ----
    let mut tex_infos: Vec<TextureInfo> = Vec::with_capacity(textures.len());
    let mut pixels: Vec<u8> = Vec::new();
    for t in textures.items() {
        tex_infos.push(TextureInfo {
            w: t.w,
            h: t.h,
            offset: pixels.len() as u32,
            len: t.rgba.len() as u32,
            has_alpha: t.has_alpha,
        });
        pixels.extend_from_slice(&t.rgba);
    }

    // ---- 语言表：方块名的中英文 ----
    let lang_entries = load_lang_table(src);
    stats.langs = lang_entries.len();

    let version = VersionDef {
        name: version_id,
        data_version: dv,
        lang_idx: 0,
        texture_by_path: tex_by_path,
        blocks,
        // 单版本包在下标 0 处放它自己的语言表（合并多个包时下标会重排，
        // 见 BlockDb::merge）
        langs: Vec::new(),
        is_overlay,
    };

    let db = BlockDb {
        strings: strings.items.clone(),
        langs: vec![lang_entries],
        textures: tex_infos,
        models: models.items().to_vec(),
        versions: vec![version],
        pixels,
    };

    let data = crate::block::db_write::serialize(&db)?;
    Ok(BuiltPack { data, stats })
}

/// 加载模型链：返回（已合并的继承链, 模型自身定义）
fn load_model<S: JarSource>(
    src: &S,
    index: &JarIndex,
    ns: &str,
    raw_id: &str,
) -> Option<(Chain, crate::block::state::ModelDef)> {
    let assets = index.namespaces.get(ns)?;
    // 父链的命名空间沿用当前模型的：parent 常写作 "block/cube"，不带前缀
    let mut load = |id: &str| -> Option<crate::block::state::ModelDef> {
        let entry = assets
            .models
            .get(id)
            .cloned()
            // minecraft 的短名在其他命名空间里也可能被引用，
            // 因此在全部命名空间中回退查找
            .or_else(|| index.namespaces.values().find_map(|a| a.models.get(id).cloned()))?;
        let bytes = src.read(&entry).ok()?;
        crate::block::state::parse_model(&bytes).ok()
    };
    let chain = build_chain(raw_id, ns, &mut load);
    let def = load(raw_id)?;
    Some((chain, def))
}

/// 登记一张贴图：解码、按内容去重、记录路径到池下标与字符串下标。
///
/// 贴图加载失败时只登记字符串下标（模型面仍需引用该路径），
/// 但不产生贴图记录——这样缺失的贴图在渲染时表现为占位，
/// 而不是让整个模型解析失败。
fn intern_texture<S: JarSource, L>(
    path: &str,
    src: &S,
    ns: &str,
    strings: &mut StringPool,
    textures: &mut ContentPool<TexEntry>,
    tex_by_path: &mut HashMap<String, u32>,
    load_texture: &mut L,
) where
    S: JarSource,
    L: FnMut(&str) -> Option<(u16, u16, Vec<u8>)>,
{
    // 字符串表始终登记：模型面存的是它的下标
    strings.intern(path);
    if tex_by_path.contains_key(path) {
        return;
    }
    // 贴图条目路径：<ns>:<path> -> assets/<ns>/textures/<path>.png
    let entry = texture_entry_of(path, ns).or_else(|| index_texture_entry(src, path));
    let Some(entry) = entry else { return };
    let decoded = src
        .read(&entry)
        .ok()
        .and_then(|bytes| crate::format::png::decode(&bytes).ok())
        .map(|img| (img.width as u16, img.height as u16, img.rgba));
    // 优先用调用方提供的加载器（测试可注入），否则用 jar 内解码结果
    let data = load_texture(path).or(decoded);
    let Some((w, h, rgba)) = data else { return };
    if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 {
        return; // 尺寸与像素不一致，宁可丢弃也不要写出错数据
    }
    let (w, h, rgba) = first_frame_of_animated(src, &entry, w, h, rgba);
    let has_alpha = rgba.chunks_exact(4).any(|p| p[3] != 255);
    let mut key = format!("{w}x{h}:").into_bytes();
    key.extend_from_slice(&rgba);
    let idx = textures.intern(&key, TexEntry { w, h, rgba, has_alpha }) as u32;
    tex_by_path.insert(path.to_string(), idx);
}

/// 动画贴图只取第一帧，当作静态贴图用。
///
/// 动画贴图是一条竖排的帧带：火焰 `fire_0.png` 是 16×512 的 32 帧。模型的 UV
/// 只按「整张图」取样，帧带会被整条压进一个方块面，看上去就是花屏；而这里要
/// 的只是「火焰长什么样」，一帧足够，也就不必在渲染端实现逐帧动画。
///
/// 帧尺寸取自同名 `.mcmeta` 的 `animation` 段：帧宽默认整图宽，帧高默认整图宽
/// （原版动画贴图的帧都是正方形）；显式写了 `frameWidth`/`frameHeight` 的以它
/// 为准。没有 `.mcmeta`、或 `.mcmeta` 里没有 `animation` 段（例如仅含
/// `villager` 段的实体贴图）时原样返回，避免把静态贴图裁坏。
fn first_frame_of_animated<S: JarSource>(
    src: &S,
    entry: &str,
    w: u16,
    h: u16,
    rgba: Vec<u8>,
) -> (u16, u16, Vec<u8>) {
    let Ok(raw) = src.read(&format!("{entry}.mcmeta")) else {
        return (w, h, rgba);
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return (w, h, rgba);
    };
    let Some(anim) = json.get("animation") else {
        return (w, h, rgba);
    };

    let fw = anim
        .get("frameWidth")
        .and_then(|v| v.as_u64())
        .unwrap_or(w as u64) as usize;
    let fh = anim
        .get("frameHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(w as u64) as usize;
    // 帧尺寸必须落在图内，且整图要装得下完整的一帧；不满足就当作没动画
    if fw == 0 || fh == 0 || fw > w as usize || fh > h as usize {
        return (w, h, rgba);
    }
    if fw == w as usize && fh == h as usize {
        return (w, h, rgba); // 只有一帧
    }

    let stride = w as usize * 4;
    let mut out = Vec::with_capacity(fw * fh * 4);
    for y in 0..fh {
        let start = y * stride;
        out.extend_from_slice(&rgba[start..start + fw * 4]);
    }
    (fw as u16, fh as u16, out)
}

/// 贴图路径 -> jar 内条目路径
fn texture_entry_of(path: &str, default_ns: &str) -> Option<String> {
    let (ns, p) = match path.split_once(':') {
        Some((n, p)) => (n, p),
        None => (default_ns, path),
    };
    if ns.is_empty() || p.is_empty() {
        return None;
    }
    Some(format!("assets/{ns}/textures/{p}.png"))
}

/// 在 jar 里查该贴图的条目路径（处理大写扩展名等情况）
fn index_texture_entry<S: JarSource>(src: &S, path: &str) -> Option<String> {
    let lower = format!("assets/{}.png", path.replace(':', "/textures/"));
    let upper = format!("assets/{}.PNG", path.replace(':', "/textures/"));
    src.entries()
        .into_iter()
        .find(|e| *e == lower || *e == upper)
}

/// 从 blockstate 条目名取出方块名（不含命名空间）
fn block_name_of(entry: &str) -> Option<String> {
    let rest = entry.strip_prefix("assets/")?;
    let (_, after) = rest.split_once('/')?;
    let stem = after.strip_prefix("blockstates/")?.strip_suffix(".json")?;
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/pack/build.rs"]
mod tests;
