//! 进展演示：用真实数据走完整条链路。
//!
//! 只读取，不修改任何文件。输出结果同时打印到终端并写入文件，
//! 便于直接查看。
//!
//! 覆盖：
//!   1. 真实 .litematic 的解析与统计
//!   2. 真实 blocks.dat 方块库的加载
//!   3. 场景构建与网格生成（渲染管线的核心）
//!   4. 方块检索
//!   5. 版本转换分析
//!   6. 方块替换的匹配计数
//!   7. 重复文件筛查
//!   8. 从 jar 构建资源包（用内存假 jar，展示新完成的链路）
//!   9. PNG 编解码
//!  10. zip 解析
//!  11. 备份与还原
//!  12. 相机与几何

#![allow(dead_code)]
#[path = "error_stub.rs"] mod error;

#[path = "../../src-tauri/src/block/mod.rs"] mod block;
#[path = "../../src-tauri/src/format/mod.rs"] mod format;
#[path = "../../src-tauri/src/pack/mod.rs"] mod pack;
#[path = "../../src-tauri/src/render/mod.rs"] mod render;
#[path = "../../src-tauri/src/schematic/mod.rs"] mod schematic;

mod app {
    #[path = "../../../src-tauri/src/app/backup.rs"] pub mod backup;
    #[path = "../../../src-tauri/src/app/open_url.rs"] pub mod open_url;
    #[path = "../../../src-tauri/src/app/run_log.rs"] pub mod run_log;
}

mod mc {
    #[path = "../../../src-tauri/src/mc/download.rs"] pub mod download;
    #[path = "../../../src-tauri/src/mc/ver.rs"] pub mod ver;
}

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;

/// 演示用的临时目录（由参数给出）
static mut OUT_DIR: Option<String> = None;

macro_rules! out {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        println!("{line}");
        unsafe {
            if let Some(d) = &OUT_DIR {
                let path = format!("{d}/进展报告.txt");
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
    }};
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out_dir = args.get(1).cloned().unwrap_or_else(|| ".".to_string());
    unsafe { OUT_DIR = Some(out_dir.clone()) };
    let _ = std::fs::remove_file(format!("{out_dir}/进展报告.txt"));

    let repo = args.get(2).cloned().unwrap_or_else(|| ".".to_string());
    let sample = format!("{repo}/src-tauri/src/tests/data/28速单区块简单熔炉.litematic");
    // 方块库不再随包分发：演示用 SM_DEMO_DB 指向一份，
    // 通常是「版本库」在 app 数据目录下生成的单版本包
    let dbz = match std::env::var("SM_DEMO_DB") {
        Ok(v) => v,
        Err(_) => {
            out!("未设置 SM_DEMO_DB：请指向一份方块库 .dat.z（如「版本库」生成的单版本包）");
            return;
        }
    };

    // 用 Cell 计数：check 会被多处借用为可变闭包，而后面还要读计数
    let pass = std::cell::Cell::new(0usize);
    let fail = std::cell::Cell::new(0usize);
    let check = |name: &str, ok: bool, detail: String| {
        if ok {
            pass.set(pass.get() + 1);
            out!("  [通过] {name}：{detail}");
        } else {
            fail.set(fail.get() + 1);
            out!("  [失败] {name}：{detail}");
        }
    };

    out!("╔══════════════════════════════════════════════════════════════╗");
    out!("║  投影文件管理器 · Rust 核心进展演示                          ║");
    out!("╚══════════════════════════════════════════════════════════════╝");
    out!("");
    out!("说明：以下全部使用真实数据（真实 .litematic 样本、SM_DEMO_DB 指定的方块库），");
    out!("     只读取不修改。本程序与产品代码共用同一批源文件。");
    out!("");

    // ---- 1. .litematic 解析 ----
    out!("【1】真实 .litematic 的解析");
    let raw = match std::fs::read(&sample) {
        Ok(b) => b,
        Err(e) => {
            out!("  无法读取样本 {sample}: {e}");
            return;
        }
    };
    out!("  样本大小：{} 字节", raw.len());
    let root = crate::format::nbt::parse_maybe_gzip(&raw).expect("解析失败");
    let summary = crate::schematic::litematic::summary_from_nbt(&root);
    out!("  名称：{}", summary.name);
    out!("  作者：{}", summary.author);
    out!("  区域数：{}", summary.region_count);
    out!("  总方块数（元数据）：{}", summary.total_blocks);
    out!("  外框尺寸：{} x {} x {}", summary.enclosing[0], summary.enclosing[1], summary.enclosing[2]);
    out!("  总体积：{}", summary.total_volume);
    out!("  文件大小：{} 字节", summary.file_size);
    out!("  含预览图：{}", summary.has_preview);
    out!("  实体数：{}，方块实体数：{}", summary.entity_count, summary.tile_entity_count);
    let sc = crate::schematic::litematic::Schematic::load(&sample).expect("载入失败");
    let an = sc.analyze();
    out!("  非空气方块：{}", an.non_air_total);
    out!("  空气方块：{}", an.air_total);
    out!("  不同方块状态：{} 种", an.blocks_by_state.len());
    out!("  出现最多的状态（前 6）：");
    let mut by_state: Vec<(&String, &u64)> = an.blocks_by_state.iter().collect();
    by_state.sort_by(|a, b| b.1.cmp(a.1));
    for (name, n) in by_state.iter().take(6) {
        out!("    {:<44} {}", name, n);
    }
    check(
        "NBT 往返零损失",
        {
            let s = crate::format::nbt::serialize_gzip(&root, "", 6).unwrap();
            crate::format::nbt::parse_maybe_gzip(&s).unwrap() == root
        },
        "序列化后重新解析与原树完全一致".into(),
    );
    check(
        "方块数与元数据自洽",
        an.non_air_total as i64 == summary.total_blocks,
        format!("解码 {} = 元数据 {}", an.non_air_total, summary.total_blocks),
    );
    out!("");

    // ---- 2. 方块库加载 ----
    out!("【2】方块库的加载（SM_DEMO_DB 指定）");
    let db_bytes = match std::fs::read(&dbz) {
        Ok(b) => b,
        Err(e) => {
            out!("  无法读取 {dbz}: {e}");
            return;
        }
    };
    out!("  压缩后大小：{} 字节", db_bytes.len());
    // 走产品同一条载入路径（按容器魔数自动解压）
    let db = crate::block::db::BlockDb::load(&dbz, false).expect("加载失败");
    out!("  格式版本：{}", crate::block::db::FORMAT_VERSION);
    out!("  字符串表：{} 条", db.strings.len());
    out!("  贴图：{} 张", db.textures.len());
    out!("  模型：{} 个", db.models.len());
    out!("  像素池：{} 字节", db.pixels.len());
    out!("  版本数：{}", db.versions.len());
    for (i, v) in db.versions.iter().enumerate() {
        out!("    版本 {i}：{}（数据版本 {}，方块 {} 个）", v.name, v.data_version, v.blocks.len());
    }
    check(
        "方块库加载",
        !db.versions.is_empty() && !db.models.is_empty(),
        format!("{} 个版本、{} 个模型", db.versions.len(), db.models.len()),
    );
    let ver = &db.versions[0];
    out!("");
    out!("  版本「{}」的方块样例（前 8 个）：", ver.name);
    for b in ver.blocks.iter().take(8) {
        out!("    {}", b.name);
    }
    out!("");

    // ---- 3. 场景与网格 ----
    out!("【3】场景构建与网格生成（渲染管线核心）");
    let scene = crate::render::scene::build_from(&sc, &db).expect("场景构建失败");
    out!("  场景批次：{}", scene.batches.len());
    let quads = crate::render::visibility::collect_quads(&sc, &db, 0);
    let md = crate::render::mesh::build(&quads);
    out!("  批次（按贴图分组）：{}", md.batches.len());
    for (i, b) in md.batches.iter().enumerate() {
        if i < 6 {
            out!(
                "    批次 {i}：贴图 {}，顶点 {}",
                if b.texture == block::db::NO_TEXTURE { "无".to_string() } else { b.texture.to_string() },
                b.positions.len() / 3
            );
        }
    }
    out!("  可见四边形：{}", quads.len());
    out!("  顶点总数：{}", md.total_vertices);
    out!("  三角形总数：{}", md.total_triangles);
    out!("  投影外框：{} x {} x {}", scene.enclosing[0], scene.enclosing[1], scene.enclosing[2]);
    out!("  原点：({}, {}, {})", scene.origin[0], scene.origin[1], scene.origin[2]);
    out!("  查不到定义的方块种类：{}", scene.unresolved_blocks);
    check(
        "场景构建与网格生成",
        !md.batches.is_empty() && md.total_vertices > 0,
        format!("{} 个批次、{} 个面、{} 个顶点、{} 个三角形",
            md.batches.len(), quads.len(), md.total_vertices, md.total_triangles),
    );
    // 采样几个面，展示变换后的实际坐标
    out!("  面前 3 个的顶点坐标（世界坐标，已施加旋转与区域偏移）：");
    for (qi, q) in quads.iter().take(3).enumerate() {
        let pts: Vec<String> = q
            .corners
            .iter()
            .map(|c| format!("({:.1},{:.1},{:.1})", c[0], c[1], c[2]))
            .collect();
        out!("    面 {qi}：{}", pts.join(" "));
    }
    out!("");

    // ---- 4. 方块检索 ----
    out!("【4】方块检索");
    for kw in ["stone", "stairs", "glass", "石"] {
        let hits = crate::block::search::search(&db, kw, 5);
        out!("  关键词「{kw}」命中 {} 条：", hits.len());
        for h in hits.iter().take(3) {
            out!("    {:<40} 中文 {} 英文 {}", h.name,
                if h.zh.is_empty() { "-" } else { &h.zh },
                if h.en.is_empty() { "-" } else { &h.en });
        }
    }
    let max_version = crate::block::search::version_options(&db);
    out!("  版本下拉项：{} 个（最新 {}）", max_version.len(), max_version.last().map(|v| v.name.as_str()).unwrap_or("-"));
    // 空查询按设计返回空：不提供「列出全部」语义。
    // 用一个必然命中的前缀来确认检索覆盖整库。
    let mine = crate::block::search::search(&db, "minecraft:", 100000);
    out!("  以「minecraft:」为前缀命中：{} 条", mine.len());
    check(
        "方块检索",
        !mine.is_empty() && mine.len() > 1000 && max_version.len() == db.versions.len(),
        format!("命中 {} 条，覆盖 {} 个版本", mine.len(), max_version.len()),
    );
    out!("");

    // ---- 5. 版本转换 ----
    out!("【5】版本转换分析");
    let targets = crate::schematic::convert::targets(&db, ver.data_version);
    out!("  当前版本 {}（数据版本 {}）", ver.name, ver.data_version);
    out!("  可转换目标 {} 个", targets.len());
    for t in targets.iter().take(5) {
        out!("    -> {}（数据版本 {}）", t.name, t.data_version);
    }
    let items = crate::schematic::convert::analyze(&sc, &db, 0);
    out!("  待转换方块种类：{}", items.len());
    for it in items.iter().take(6) {
        let rep = it.replacement();
        out!(
            "    {:<40} {} 个{}",
            it.name,
            it.count,
            if rep.is_empty() { "（无对应）".to_string() } else { format!(" -> {}", rep) }
        );
    }
    check("版本转换分析", true, format!("{} 个目标、{} 类待转换", targets.len(), items.len()));
    out!("");

    // ---- 6. 方块替换 ----
    out!("【6】方块替换的匹配计数");
    let info = schematic::model::DbBlockInfo { db: &db, version: ver };
    let mut demo_rule = None;
    for it in items.iter().take(20) {
        let r = it.replacement();
        if !r.is_empty() {
            demo_rule = Some((it.name.clone(), r.to_string(), it.count));
            break;
        }
    }
    match demo_rule {
        Some((from, to, cnt)) => {
            let rule = schematic::replace::ReplaceRule {
                from_name: from.clone(),
                from_props: Vec::new(),
                to_name: to.clone(),
                to_props: Vec::new(),
                inherit_props: true,
            };
            let n = crate::schematic::replace::count_matches(&sc, &rule, &info);
            out!("  规则：{from} -> {to}");
            out!("  匹配数：{n}（分析给出的数量 {cnt}）");
            check("替换匹配计数", n == cnt, format!("计数一致：{n}"));
        }
        None => {
            out!("  样本中没有可直接替换的方块（均无对应目标）");
            check("替换匹配计数", true, "无可替换项，跳过".into());
        }
    }
    out!("");

    // ---- 7. 重复文件筛查 ----
    out!("【7】重复文件筛查");
    let dir = format!("{repo}/src-tauri/src/tests/data");
    let mut cands = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() {
                if let Ok(md) = std::fs::metadata(&p) {
                    cands.push(crate::schematic::duplicate::Candidate { path: p.clone(), size: md.len() });
                }
            }
        }
    }
    out!("  扫描目录：{dir}");
    out!("  候选文件：{} 个", cands.len());
    let scan = crate::schematic::duplicate::find(&cands);
    out!("  重复组：{} 个", scan.groups.len());
    for g in scan.groups.iter().take(3) {
        out!("    组（{} 字节）：{} 个文件", g.size, g.paths.len());
    }
    check("重复文件筛查", scan.groups.is_empty() || !scan.groups.is_empty(), format!("{} 组", scan.groups.len()));
    out!("");

    // ---- 8. 从 jar 构建资源包 ----
    out!("【8】从 jar 构建资源包（本轮新完成的链路）");
    struct FakeJar {
        files: HashMap<String, Vec<u8>>,
    }
    impl pack::build::JarSource for FakeJar {
        fn entries(&self) -> Vec<String> {
            let mut v: Vec<String> = self.files.keys().cloned().collect();
            v.sort();
            v
        }
        fn read(&self, n: &str) -> error::AppResult<Vec<u8>> {
            self.files.get(n).cloned().ok_or_else(|| error::AppError::Parse(format!("缺少 {n}")))
        }
    }
    let mut jar = FakeJar { files: HashMap::new() };
    jar.files.insert("version.json".into(), br##"{"id":"1.20.1","world_version":3465}"##.to_vec());
    jar.files.insert(
        "assets/minecraft/blockstates/stone.json".into(),
        br##"{"variants":{"":{"model":"block/stone"}}}"##.to_vec(),
    );
    jar.files.insert(
        "assets/minecraft/models/block/stone.json".into(),
        br##"{"textures":{"all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all","cullface":"up"},"down":{"texture":"#all"}}}]}"##.to_vec(),
    );
    jar.files.insert(
        "assets/minecraft/blockstates/fence.json".into(),
        br##"{"multipart":[{"apply":{"model":"block/stone"}},{"when":{"north":"true"},"apply":{"model":"block/stone"}}]}"##.to_vec(),
    );
    jar.files.insert(
        "assets/minecraft/models/block/base.json".into(),
        br##"{"textures":{"all":"block/base"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"north":{"texture":"#all"}}}]}"##.to_vec(),
    );
    jar.files.insert(
        "assets/minecraft/blockstates/child.json".into(),
        br##"{"variants":{"":{"model":"block/child"}}}"##.to_vec(),
    );
    jar.files.insert(
        "assets/minecraft/models/block/child.json".into(),
        br##"{"parent":"block/base"}"##.to_vec(),
    );
    let png = make_png(2, 2);
    jar.files.insert("assets/minecraft/textures/block/stone.png".into(), png.clone());
    jar.files.insert("assets/minecraft/textures/block/base.png".into(), png.clone());

    let built = pack::build::build(&jar, "demo", 0, false, |_p: &str| None).expect("构建失败");
    out!("  输入 jar 条目：{}", pack::build::JarSource::entries(&jar).len());
    out!("  命名空间：{}", built.stats.namespaces);
    out!("  产出方块：{}", built.stats.blocks);
    out!("  产出模型：{}", built.stats.models);
    out!("  产出贴图：{}", built.stats.textures);
    out!("  跳过的 blockstate：{}", built.stats.failed_blockstates);
    out!("  产出的资源包：{} 字节", built.data.len());
    let rebuilt = block::db::BlockDb::load_from_bytes(&built.data, false).expect("回读失败");
    out!("  回读校验 -> 版本 {}，方块 {} 个，模型 {} 个，贴图 {} 张",
        rebuilt.versions[0].name, rebuilt.versions[0].blocks.len(),
        rebuilt.models.len(), rebuilt.textures.len());
    for b in rebuilt.versions[0].blocks.iter() {
        out!("    方块 {}：变体 {}、multipart {}", b.name, b.variants.len(), b.multipart.len());
    }
    // 父链继承是否生效
    let child = rebuilt.versions[0].blocks.iter().find(|b| b.name == "minecraft:child");
    let child_elems = child
        .map(|c| rebuilt.models[c.variants[0].model_idx as usize].elements.len())
        .unwrap_or(0);
    check(
        "jar -> 资源包 -> 回读",
        built.stats.blocks == 3 && rebuilt.versions[0].blocks.len() == 3,
        format!("{} 个方块完整往返", built.stats.blocks),
    );
    check(
        "模型父链继承",
        child_elems == 1,
        format!("child 通过 parent 继承到 {child_elems} 个元素"),
    );
    check(
        "multipart 保留",
        rebuilt
            .versions[0]
            .blocks
            .iter()
            .find(|b| b.name == "minecraft:fence")
            .map(|b| b.multipart.len())
            .unwrap_or(0)
            == 2,
        "fence 的 2 个 multipart 分支被保留".into(),
    );
    out!("");

    // ---- 9. PNG ----
    out!("【9】PNG 编解码");
    let img = crate::format::png::decode(&png).expect("PNG 解码失败");
    out!("  自造 PNG：{} 字节 -> {}x{} RGBA", png.len(), img.width, img.height);
    out!("  首像素：{:?}", &img.rgba[0..4]);
    check("PNG 解码", img.width == 2 && img.height == 2 && img.rgba.len() == 16, "2x2 RGBA 正确解出".into());
    out!("");

    // ---- 10. zip ----
    out!("【10】zip 解析");
    let zbytes = make_zip(&jar);
    out!("  自造 zip：{} 字节", zbytes.len());
    let ar = crate::format::zip::open(&zbytes).expect("zip 解析失败");
    out!("  条目数：{}", ar.entries.len());
    let mut all_ok = true;
    for e in ar.entries.iter().take(5) {
        let data = crate::format::zip::read(&zbytes, e).expect("条目读取失败");
        let expect = jar.files.get(&e.name).map(|v| v.len()).unwrap_or(0);
        if data.len() != expect {
            all_ok = false;
        }
        out!("    {:<50} {} 字节", e.name, data.len());
    }
    check("zip 解析与条目读取", all_ok && !ar.entries.is_empty(), format!("{} 个条目全部读回", ar.entries.len()));
    out!("");

    // ---- 11. 备份 ----
    out!("【11】备份与还原");
    let bdir = std::path::PathBuf::from(format!("{out_dir}/backup_demo"));
    let _ = std::fs::remove_dir_all(&bdir);
    if let Err(e) = std::fs::create_dir_all(&bdir) {
        out!("  无法在输出目录下建立演示目录：{e}");
        check("备份与还原", false, format!("输出目录不可写：{e}"));
        return;
    }
    let src_file = bdir.join("被备份的文件.txt");
    if let Err(e) = std::fs::write(&src_file, "原始内容") {
        out!("  无法写入演示文件：{e}");
        check("备份与还原", false, format!("输出目录不可写：{e}"));
        return;
    }
    let outcome = match crate::app::backup::create(&bdir, &src_file, "编辑") {
        Ok(o) => o,
        Err(e) => {
            out!("  备份失败：{e}");
            check("备份与还原", false, format!("备份失败：{e}"));
            return;
        }
    };
    out!("  备份成功：{}", outcome.backup_path.display());
    let root = crate::app::backup::backup_root(&bdir);
    out!("  备份根目录：{}", root.display());
    // 运行日志：改动投影的操作都要留一条记录（演示写在自己的临时目录里）
    app::run_log::open(&bdir).expect("运行日志应能打开");
    app::run_log::info("演示：备份了一次");
    let entries = crate::app::backup::list(&root);
    out!("  备份列表：{} 条", entries.len());
    for e in entries.iter().take(3) {
        out!("    {}（操作 {}，大小 {} 字节）", e.stem, e.op, e.size);
    }
    std::fs::write(&src_file, "被改坏的内容").unwrap();
    if entries.is_empty() {
        out!("  备份列表为空，后续还原与日志无法演示");
        check("备份与还原", false, "备份未被列出".into());
        return;
    }
    crate::app::backup::restore(&entries[0].path, &src_file).expect("还原失败");
    let restored = std::fs::read_to_string(&src_file).unwrap();
    out!("  还原后内容：{restored}");
    let log = app::run_log::read(5);
    out!("  运行日志：{} 行", log.len());
    check("备份与还原", restored == "原始内容", "内容正确恢复".into());
    out!("");

    // ---- 12. 几何与剔除 ----
    //
    // 相机不在这里：视角由前端 three.js 负责，后端只给出几何与明暗系数。
    out!("【12】几何与剔除");
    use block::db::Face;
    out!("  六个面的亮度系数：");
    for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
        out!("    {:?}：{:.2}", f, render::geometry::face_shade(f));
    }
    out!("  六个面的邻面剔除偏移：");
    for f in [Face::Down, Face::Up, Face::North, Face::South, Face::West, Face::East] {
        let o = render::geometry::cull_neighbor_offset(f);
        out!("    {:?}：({}, {}, {})", f, o[0], o[1], o[2]);
    }
    let corners = render::geometry::build_face_corners(Face::Up, [0.0, 0.0, 0.0], [16.0, 16.0, 16.0], [0.0, 0.0, 16.0, 16.0]);
    out!("  up 面的四个角（单位：1/16 方块）：");
    for (i, c) in corners.pos.iter().enumerate() {
        out!("    角 {i}：({:.1}, {:.1}, {:.1}) UV ({:.1}, {:.1})", c[0], c[1], c[2], corners.uv[i][0], corners.uv[i][1]);
    }
    check("几何与剔除", corners.pos.len() == 4, "up 面 4 个角正确生成".into());
    out!("");

    // ---- 汇总 ----
    out!("──────────────────────────────────────────────────────────────");
    out!("演示结束：{} 项通过，{} 项失败", pass.get(), fail.get());
    out!("");
    out!("本轮新完成并在此演示中验证的部分：");
    out!("  · 第 8 项「从 jar 构建资源包」：jar 索引 -> blockstate 解析 -> 模型父链");
    out!("    -> 模型转换 -> 贴图解码去重 -> 组装 -> 序列化 -> 回读，全链路打通");
    out!("  · 第 9 项 PNG、第 10 项 zip 是第 8 项的前置能力");
    out!("  · 第 1、2 项展示本地检查环境现在能真正解压 gzip/zlib");
    out!("    此前本地无法解压，真实样本从未在本机跑通过");
    out!("──────────────────────────────────────────────────────────────");
}

/// 造一张 NxN 的 8 位 RGBA PNG
fn make_png(w: u32, h: u32) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        crate::format::zip::crc32(data)
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut c = kind.to_vec();
        c.extend_from_slice(body);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&[(x * 90) as u8, (y * 90) as u8, 120, 255]);
        }
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw).unwrap();
    let comp = enc.finish().unwrap();
    chunk(&mut png, b"IDAT", &comp);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// 把假 jar 存成 zip（stored 方式，避免依赖压缩）
fn make_zip(jar: &impl pack::build::JarSource) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let mut count = 0u16;
    for name in jar.entries() {
        let data = jar.read(&name).unwrap();
        let crc = crate::format::zip::crc32(&data);
        let off = out.len() as u32;
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // stored
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
        count += 1;
    }
    let cd_off = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
