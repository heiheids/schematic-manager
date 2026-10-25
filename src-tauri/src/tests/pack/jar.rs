use super::*;

use crate::test_support::{make_png, make_zip};

/// 测试用的临时目录。
///
/// 落在 target 目录下而不是系统临时目录：部分受限环境不允许写系统
/// 临时目录，而这类环境与「代码是否正确」无关，测试不应因此失败。
/// target 目录由构建过程创建，必然可写。
fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!(
            "jar_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&d).expect("应能在 target 下建立测试目录");
    d
}

#[test]
fn opens_and_lists_entries() {
    let zip = make_zip(&[("a.txt", b"hello"), ("assets/x/y.json", b"{}")]);
    let d = tmp("list");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();

    let jar = JarFile::open(p.to_str().unwrap()).unwrap();
    assert_eq!(jar.entry_count(), 2);
    let names = JarSource::entries(&jar);
    assert_eq!(names, vec!["a.txt", "assets/x/y.json"]);
    assert_eq!(jar.read("a.txt").unwrap(), b"hello");
}

#[test]
fn missing_entry_is_reported_with_name() {
    let zip = make_zip(&[("a.txt", b"x")]);
    let d = tmp("missing");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();
    let jar = JarFile::open(p.to_str().unwrap()).unwrap();
    let e = jar.read("nope.txt").unwrap_err().to_string();
    assert!(e.contains("nope.txt"), "错误里应指出缺失的条目名：{e}");
}

#[test]
fn rejects_non_zip_file() {
    let d = tmp("bad");
    let p = d.join("t.jar");
    std::fs::write(&p, b"this is definitely not a zip archive at all").unwrap();
    assert!(JarFile::open(p.to_str().unwrap()).is_err());
}

#[test]
fn rejects_missing_file_with_path_in_message() {
    let e = JarFile::open("C:/definitely/not/here.jar").unwrap_err().to_string();
    assert!(e.contains("here.jar"), "错误里应包含路径：{e}");
}

#[test]
fn decodes_texture_with_explicit_namespace() {
    let png = make_png(2, 2, 255);
    let zip = make_zip(&[("assets/minecraft/textures/block/stone.png", &png)]);
    let d = tmp("tex");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();
    let jar = JarFile::open(p.to_str().unwrap()).unwrap();

    let (w, h, rgba) = jar.decode_texture("minecraft:block/stone").expect("应解出贴图");
    assert_eq!((w, h), (2, 2));
    assert_eq!(rgba.len(), 16);
}

#[test]
fn decodes_texture_without_namespace_as_minecraft() {
    // 模型里的贴图引用常省略命名空间
    let png = make_png(1, 1, 255);
    let zip = make_zip(&[("assets/minecraft/textures/block/x.png", &png)]);
    let d = tmp("texns");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();
    let jar = JarFile::open(p.to_str().unwrap()).unwrap();
    assert!(jar.decode_texture("block/x").is_some(), "无前缀应按 minecraft 处理");
}

#[test]
fn missing_texture_returns_none() {
    let zip = make_zip(&[("a.txt", b"x")]);
    let d = tmp("notex");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();
    let jar = JarFile::open(p.to_str().unwrap()).unwrap();
    assert!(jar.decode_texture("minecraft:block/nope").is_none());
}

#[test]
fn corrupt_png_returns_none_not_panic() {
    let zip = make_zip(&[("assets/minecraft/textures/block/bad.png", b"not a png")]);
    let d = tmp("badpng");
    let p = d.join("t.jar");
    std::fs::write(&p, &zip).unwrap();
    let jar = JarFile::open(p.to_str().unwrap()).unwrap();
    assert!(
        jar.decode_texture("minecraft:block/bad").is_none(),
        "损坏的 PNG 应返回 None 而不是崩溃"
    );
}

#[test]
fn builds_pack_from_real_jar_file_and_writes_it() {
    let png = make_png(2, 2, 255);
    let zip = make_zip(&[
        ("version.json", br##"{"id":"1.20.1","world_version":3465}"##),
        (
            "assets/minecraft/blockstates/stone.json",
            br##"{"variants":{"":{"model":"block/stone"}}}"##,
        ),
        (
            "assets/minecraft/models/block/stone.json",
            br##"{"textures":{"all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
        ("assets/minecraft/textures/block/stone.png", &png),
    ]);
    let d = tmp("build");
    let jar_path = d.join("client.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    let out = d.join("test.pack");

    let stats = build_pack_to(
        jar_path.to_str().unwrap(),
        out.to_str().unwrap(),
        "test",
        0,
        false,
    )
    .expect("构建失败");

    assert_eq!(stats.blocks, 1);
    assert_eq!(stats.models, 1);
    assert_eq!(stats.textures, 1, "贴图应从 jar 内解码并登记");
    assert!(out.is_file(), "产出文件应已落盘");

    // 真正读回一次，确认写出的文件可用
    let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), false).expect("读回失败");
    assert_eq!(db.versions.len(), 1);
    assert_eq!(db.versions[0].name, "1.20.1");
    assert_eq!(db.versions[0].blocks[0].name, "minecraft:stone");
    assert_eq!(db.textures.len(), 1);
    assert_eq!(db.models.len(), 1);
}

/// 联网验证「补齐语言表后，库里存的确实是中文名」。
///
/// 整条链路走一遍：从资源索引下载 zh_cn → 建包 → 读回 → 取显示名称。
/// 只测下载或只测建包都不够：前者证明不了名字进了库，后者用的是造出来的
/// 语言表、证明不了真实语言表能被认出来（键名如 `block.minecraft.stone`
/// 必须与构建侧的查表方式对得上）。
///
/// 默认不跑（`cargo test -- --ignored` 手动执行）：单元测试不该依赖网络。
#[test]
#[ignore = "需要联网，手动运行 cargo test -- --ignored"]
fn downloaded_lang_file_makes_the_pack_store_chinese_names() {
    let manifest = crate::mc::download::fetch_manifest().expect("应能取到版本清单");
    let req = crate::mc::download::LangDownload {
        version_id: "1.21.1",
        source: crate::mc::download::AssetSource::Official,
        mc_dir: None,
        manifest: Some(&manifest),
        langs: &["zh_cn"],
    };
    let langs = crate::mc::download::download_lang_files(&req).expect("应能下载中文语言表");
    assert_eq!(langs.len(), 1, "应当只下到请求的那一个语言表");

    let png = make_png(2, 2, 255);
    let zip = make_zip(&[
        ("version.json", br##"{"id":"1.21.1","world_version":3955}"##),
        (
            "assets/minecraft/blockstates/stone.json",
            br##"{"variants":{"":{"model":"block/stone"}}}"##,
        ),
        (
            "assets/minecraft/models/block/stone.json",
            br##"{"textures":{"all":"block/stone"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
        ("assets/minecraft/textures/block/stone.png", &png),
        // 真实客户端 jar 里带的就是这份英文表，中文另外从资源库补
        (
            "assets/minecraft/lang/en_us.json",
            br##"{"block.minecraft.stone":"Stone"}"##,
        ),
    ]);
    let d = tmp("lang_e2e");
    let jar_path = d.join("client.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    let out = d.join("lang.pack");

    build_pack_to_with_langs(
        jar_path.to_str().unwrap(),
        out.to_str().unwrap(),
        "1.21.1",
        3955,
        false,
        langs,
    )
    .expect("构建失败");

    let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), false).expect("读回失败");
    let name = db.versions[0].display_name("minecraft:stone");
    assert_eq!(name, "石头", "建好的库里应当存着中文名，实际是：{name}");

    // 英文那套也要在：界面语言切到英语时靠它
    let en = db.versions[0]
        .langs
        .iter()
        .find(|e| e.block_name == "minecraft:stone")
        .map(|e| e.en.clone())
        .unwrap_or_default();
    assert_eq!(en, "Stone", "英文名同样要留着，实际是：{en}");
}

#[test]
fn rebuild_overwrites_existing_file() {
    // 重复导入同一个 jar 不应产生多份副本
    let mut files: Vec<(&str, &[u8])> = vec![
        (
            "assets/minecraft/blockstates/a.json",
            br##"{"variants":{"":{"model":"block/a"}}}"##,
        ),
        (
            "assets/minecraft/models/block/a.json",
            br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
    ];
    let png = make_png(1, 1, 255);
    files.push(("assets/minecraft/textures/block/a.png", &png));
    let zip = make_zip(&files);

    let d = tmp("overwrite");
    let jar_path = d.join("c.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    let out = d.join("o.pack");

    build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();
    let first = std::fs::metadata(&out).unwrap().len();
    build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();
    let second = std::fs::metadata(&out).unwrap().len();
    assert_eq!(first, second, "重复导入应覆盖为相同内容，而不是追加");
    // 目录里只有一个产出文件
    let n = std::fs::read_dir(&d)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "pack").unwrap_or(false))
        .count();
    assert_eq!(n, 1);
}

#[test]
fn output_directory_is_created_when_absent() {
    let png = make_png(1, 1, 255);
    let zip = make_zip(&[
        (
            "assets/minecraft/blockstates/a.json",
            br##"{"variants":{"":{"model":"block/a"}}}"##,
        ),
        (
            "assets/minecraft/models/block/a.json",
            br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
        ("assets/minecraft/textures/block/a.png", &png),
    ]);
    let d = tmp("mkdir");
    let jar_path = d.join("c.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    // 多级不存在的目录
    let out = d.join("deep").join("nested").join("o.pack");
    build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false)
        .expect("应自动建立输出目录");
    assert!(out.is_file());
}

#[test]
fn overlay_flag_and_explicit_version_are_passed_through() {
    let png = make_png(1, 1, 255);
    let zip = make_zip(&[
        ("version.json", br##"{"id":"1.20.1","world_version":3465}"##),
        (
            "assets/minecraft/blockstates/a.json",
            br##"{"variants":{"":{"model":"block/a"}}}"##,
        ),
        (
            "assets/minecraft/models/block/a.json",
            br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
        ("assets/minecraft/textures/block/a.png", &png),
    ]);
    let d = tmp("overlay");
    let jar_path = d.join("mod.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    let out = d.join("m.pack");
    build_pack_to(
        jar_path.to_str().unwrap(),
        out.to_str().unwrap(),
        "mymod",
        4444,
        true,
    )
    .unwrap();

    let db = crate::block::db::BlockDb::load(out.to_str().unwrap(), true).unwrap();
    assert_eq!(db.versions[0].data_version, 4444, "模组包应使用指定的数据版本");
    assert!(db.versions[0].is_overlay);
}

#[test]
fn packed_output_is_zlib() {
    // 产物与基础库单版本包用同一种容器，读取端才能按同一路径读
    let png = make_png(1, 1, 255);
    let zip = make_zip(&[
        (
            "assets/minecraft/blockstates/a.json",
            br##"{"variants":{"":{"model":"block/a"}}}"##,
        ),
        (
            "assets/minecraft/models/block/a.json",
            br##"{"textures":{"all":"block/a"},"elements":[{"from":[0,0,0],"to":[16,16,16],"faces":{"up":{"texture":"#all"}}}]}"##,
        ),
        ("assets/minecraft/textures/block/a.png", &png),
    ]);
    let d = tmp("zlib");
    let jar_path = d.join("c.jar");
    std::fs::write(&jar_path, &zip).unwrap();
    let out = d.join("o.pack");
    build_pack_to(jar_path.to_str().unwrap(), out.to_str().unwrap(), "x", 0, false).unwrap();

    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(bytes[0], 0x78, "应以 zlib 头开头（{:02x}）", bytes[0]);
    let hdr = bytes[0] as u16 * 256 + bytes[1] as u16;
    assert_eq!(hdr % 31, 0, "zlib 头校验应通过");
}

impl JarFile {
    /// 条目总数
    pub fn entry_count(&self) -> usize {
        self.archive.entries.len()
    }
}

/// 从 jar 文件构建资源包并落盘（不带 jar 之外的语言表）。
///
/// 返回产出信息；`out_path` 已存在时直接覆盖——导入是幂等操作，
/// 用户重复导入同一个 jar 不应产生多份副本。
///
/// 只给测试用：正式流程一律走 [`build_pack_to_with_langs`]，因为客户端 jar
/// 里只有 `en_us.json`，不额外带上中文语言表就会建出只有英文名的库。
pub fn build_pack_to(
    jar_path: &str,
    out_path: &str,
    pack_name: &str,
    data_version: i32,
    is_overlay: bool,
) -> AppResult<crate::pack::build::BuildStats> {
    build_pack_to_with_langs(
        jar_path,
        out_path,
        pack_name,
        data_version,
        is_overlay,
        Vec::new(),
    )
}
