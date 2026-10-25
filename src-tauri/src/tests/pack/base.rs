use super::*;
use std::collections::HashMap;
use std::io::Write;

/// 测试目录落在 target 下：部分受限环境不允许写系统临时目录
///（与 pack/jar.rs 测试里的 tmp 同一考虑），而 target 必然可写。
fn test_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!(
            "base_db_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&d).expect("应能在 target 下建立测试目录");
    d
}

/// 造一个最小单版本 pack 文件（zlib 压缩，与真实产物一致）。
fn write_pack(dir: &Path, name: &str, dv: i32, version_name: &str) {
    let db = BlockDb {
        strings: Vec::new(),
        langs: Vec::new(),
        textures: Vec::new(),
        models: Vec::new(),
        versions: vec![crate::block::db::VersionDef {
            name: version_name.into(),
            data_version: dv,
            lang_idx: 0,
            texture_by_path: HashMap::new(),
            blocks: Vec::new(),
            langs: Vec::new(),
            is_overlay: false,
        }],
        pixels: Vec::new(),
    };
    let raw = crate::block::db_write::serialize(&db).expect("序列化失败");
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    e.write_all(&raw).expect("压缩失败");
    let packed = e.finish().expect("压缩失败");
    std::fs::write(dir.join(name), packed).expect("写 pack 失败");
}

#[test]
fn finds_language_files_through_the_asset_index() {
    // 中文不在客户端 jar 里，而在 .minecraft/assets 的对象库中，
    // 靠资源索引查到 hash 才能取到。找不到就会整列没有中文名。
    let mc = test_dir("lang");
    let versions = mc.join("versions").join("1.21.11");
    std::fs::create_dir_all(&versions).unwrap();
    std::fs::write(
        versions.join("1.21.11.json"),
        br#"{"id":"1.21.11","assetIndex":{"id":"29"}}"#,
    )
    .unwrap();

    let indexes = mc.join("assets").join("indexes");
    std::fs::create_dir_all(&indexes).unwrap();
    std::fs::write(
        indexes.join("29.json"),
        br#"{"objects":{"minecraft/lang/zh_cn.json":{"hash":"aabbcc","size":3}}}"#,
    )
    .unwrap();

    let zh = br#"{"block.minecraft.stone":"should-not-be-this"}"#;
    let obj = mc.join("assets").join("objects").join("aa");
    std::fs::create_dir_all(&obj).unwrap();
    std::fs::write(obj.join("aabbcc"), zh).unwrap();

    let got = find_lang_files(&mc, "1.21.11");
    assert_eq!(got.len(), 1, "应找到中文语言文件，实际 {got:?}");
    assert_eq!(got[0].0, "assets/minecraft/lang/zh_cn.json");
    assert_eq!(got[0].1, zh);
}

#[test]
fn language_lookup_falls_back_to_another_index_and_gives_up_quietly() {
    // 目标版本的索引不在本机时（只装过别的版本、索引被清理过），
    // 退到本机任意能查到该语言的索引；一个都查不到就不提供，
    // 让构建侧回落到英文名，而不是报错中断补库
    let mc = test_dir("lang_fallback");
    std::fs::create_dir_all(mc.join("assets").join("indexes")).unwrap();
    std::fs::write(
        mc.join("assets").join("indexes").join("17.json"),
        br#"{"objects":{"minecraft/lang/zh_cn.json":{"hash":"ddeeff"}}}"#,
    )
    .unwrap();
    let obj = mc.join("assets").join("objects").join("dd");
    std::fs::create_dir_all(&obj).unwrap();
    std::fs::write(obj.join("ddeeff"), b"{}").unwrap();

    let got = find_lang_files(&mc, "9.9.9");
    assert_eq!(got.len(), 1, "应退到本机其它索引，实际 {got:?}");

    // 资源库整体不存在时返回空，且不 panic
    let empty = test_dir("lang_missing");
    assert!(find_lang_files(&empty, "1.21.11").is_empty());
}

#[test]
fn scan_finds_versions_and_warns_on_broken_jars() {
    let dir = test_dir("scan");
    let versions = dir.join("versions");

    let good = versions.join("1.20.1");
    std::fs::create_dir_all(&good).unwrap();
    let jar =
        crate::test_support::make_zip(&[("version.json", br#"{"id":"1.20.1","world_version":3465}"#
            .as_slice())]);
    std::fs::write(good.join("1.20.1.jar"), jar).unwrap();

    let broken = versions.join("1.21.1");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("1.21.1.jar"), b"this is not a zip archive").unwrap();

    // 加载器目录没有同名 jar，应被跳过且不留警告
    std::fs::create_dir_all(versions.join("fabric-loader-0.15.0")).unwrap();

    let scan = scan_minecraft(&dir).expect("扫描失败");
    assert_eq!(scan.versions.len(), 1, "应只命中一个有效版本");
    let hit = &scan.versions[0];
    assert_eq!(hit.id, "1.20.1");
    assert_eq!(hit.data_version, 3465);
    assert!(hit.jar_path.is_file());
    assert_eq!(scan.warnings.len(), 1, "坏 jar 应各产生一条警告");
    assert!(
        scan.warnings[0].contains("1.21.1"),
        "警告应指出是哪个版本：{}",
        scan.warnings[0]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_recurses_into_subdirectories_of_any_directory() {
    // 目录不要求是 .minecraft：任意目录、任意层级里的客户端 jar 都要找到。
    // 版本 id 取自 jar 内的 version.json，而不是文件夹名——jar 可能被改名。
    let dir = test_dir("scan-recursive");
    let nested = dir.join("启动器").join("实例").join("versions").join("随便起的名字");
    std::fs::create_dir_all(&nested).unwrap();
    let jar = crate::test_support::make_zip(&[(
        "version.json",
        br#"{"id":"1.21.1","world_version":3955}"#.as_slice(),
    )]);
    std::fs::write(nested.join("client.jar"), jar).unwrap();

    // 模组：没有 version.json，应静默跳过而不是报一堆失败
    let mods = dir.join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    let mod_jar = crate::test_support::make_zip(&[(
        "fabric.mod.json",
        br#"{"id":"sodium"}"#.as_slice(),
    )]);
    std::fs::write(mods.join("sodium-0.5.jar"), mod_jar).unwrap();

    let scan = scan_minecraft(&dir).expect("扫描失败");
    assert_eq!(scan.versions.len(), 1, "应找到深层目录里的客户端 jar");
    assert_eq!(scan.versions[0].id, "1.21.1", "id 应取自 version.json");
    assert_eq!(scan.versions[0].data_version, 3955);
    assert!(scan.warnings.is_empty(), "模组不应产生警告：{:?}", scan.warnings);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_keeps_one_hit_per_version_and_sorts_by_data_version() {
    let dir = test_dir("scan-dedupe");
    for (tag, dv) in [("a", 3955), ("b", 3465)] {
        let d = dir.join(tag);
        std::fs::create_dir_all(&d).unwrap();
        let version = if dv == 3955 { "1.21.1" } else { "1.20.1" };
        let body = format!(r#"{{"id":"{version}","world_version":{dv}}}"#);
        std::fs::write(d.join("client.jar"), crate::test_support::make_zip(&[(
            "version.json",
            body.as_bytes(),
        )]))
        .unwrap();
    }
    // 同一版本的第二个副本：不该出现两行
    let dup = dir.join("c");
    std::fs::create_dir_all(&dup).unwrap();
    std::fs::write(dup.join("copy.jar"), crate::test_support::make_zip(&[(
        "version.json",
        br#"{"id":"1.20.1","world_version":3465}"#.as_slice(),
    )]))
    .unwrap();

    let scan = scan_minecraft(&dir).expect("扫描失败");
    assert_eq!(scan.versions.len(), 2, "同一版本只保留一条");
    let dvs: Vec<i32> = scan.versions.iter().map(|v| v.data_version).collect();
    assert_eq!(dvs, [3465, 3955], "结果按数据版本升序");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_rejects_a_missing_or_non_directory() {
    let dir = test_dir("scan-missing");
    let missing = dir.join("并不存在");
    let e = scan_minecraft(&missing).unwrap_err().to_string();
    assert!(e.contains("失败"), "目录不存在应报错：{e}");

    let file = dir.join("a.txt");
    std::fs::write(&file, b"x").unwrap();
    let e = scan_minecraft(&file).unwrap_err().to_string();
    assert!(e.contains("不是目录"), "指向文件时应说明不是目录：{e}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn base_db_merges_packs_by_data_version() {
    let dir = test_dir("merge");
    // 故意乱序写入：合成顺序必须由文件名里的数据版本决定，
    // 而不是文件写入先后
    write_pack(&dir, "3465-1.20.1.dat.z", 3465, "1.20.1");
    write_pack(&dir, "1952-1.14.dat.z", 1952, "1.14");

    let (db, failures) = load_base_block_db(&dir).expect("载入失败");
    assert!(failures.is_empty(), "不应有失败项：{failures:?}");
    let dvs: Vec<i32> = db.versions.iter().map(|v| v.data_version).collect();
    assert_eq!(dvs, [1952, 3465], "版本应按数据版本升序合成");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn base_db_survives_broken_files() {
    let dir = test_dir("broken");
    write_pack(&dir, "1952-1.14.dat.z", 1952, "1.14");
    std::fs::write(dir.join("3465-1.20.1.dat.z"), b"garbage").unwrap();
    std::fs::write(dir.join("随手丢的文件.dat.z"), b"garbage").unwrap();
    // 非 .dat.z 文件（如构建中的临时文件）不应惊动用户
    std::fs::write(dir.join("3700-1.20.4.dat.z.tmp"), b"partial").unwrap();

    let (db, failures) = load_base_block_db(&dir).expect("载入失败");
    assert_eq!(db.versions.len(), 1, "好包应照常载入");
    assert_eq!(db.versions[0].data_version, 1952);
    assert_eq!(failures.len(), 2, "两个坏文件各应有一条失败记录");
    assert!(
        failures.iter().any(|(f, _)| f == "3465-1.20.1.dat.z"),
        "失败项应指出文件名：{failures:?}"
    );
    assert!(
        failures.iter().any(|(f, _)| f == "随手丢的文件.dat.z"),
        "文件名不符约定也应指出：{failures:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_base_dir_is_an_empty_db() {
    let dir = test_dir("missing").join("不存在");
    let (db, failures) = load_base_block_db(&dir).expect("目录不存在不应报错");
    assert!(db.versions.is_empty());
    assert!(failures.is_empty());
}

#[test]
fn supported_releases_are_strictly_ascending() {
    // 合成顺序与界面排序都依赖这份清单按数据版本升序
    assert!(
        SUPPORTED_RELEASES.windows(2).all(|w| w[0].0 < w[1].0),
        "SUPPORTED_RELEASES 必须按数据版本严格升序"
    );
}
