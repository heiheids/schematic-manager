use super::*;

fn test_dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test_tmp")
        .join(format!(
            "entitydb_write_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(&d).expect("应能在 target 下建立测试目录");
    d
}

/// 造一个带实体贴图的假客户端 jar；textures 是 (entity/ 下相对路径, 宽, 高)。
fn fake_client(dir: &Path, id: &str, textures: &[(&str, u32, u32)]) -> PathBuf {
    let mut entries: Vec<(String, Vec<u8>)> = vec![(
        "version.json".into(),
        br#"{"id":"x","world_version":3465}"#.to_vec(),
    )];
    for (path, w, h) in textures {
        entries.push((
            format!("assets/minecraft/textures/entity/{path}.png"),
            crate::test_support::make_png(*w, *h, 255),
        ));
    }
    let files: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let jar_path = dir.join(format!("{id}.jar"));
    std::fs::write(&jar_path, crate::test_support::make_zip(&files)).expect("写假 jar 失败");
    jar_path
}

#[test]
fn add_version_roundtrip() {
    let dir = test_dir("roundtrip");
    let jar_path = fake_client(
        &dir,
        "1.20.1",
        &[("chest/normal", 4, 2), ("chest/trapped", 3, 3)],
    );
    let jar = JarFile::open(&jar_path.to_string_lossy()).expect("打开假 jar 失败");
    let db_path = dir.join("entities.dat.z");

    let n = add_version(&db_path, &jar, 3465, "1.20.1").expect("并入失败");
    assert_eq!(n, 2, "应并入两张贴图");

    let db = EntityDb::load(&db_path).expect("写出的库应能读回");
    let v = db.version_for(3465).expect("应能按数据版本找到并入的版本");
    assert_eq!(v.name, "1.20.1");
    assert_eq!(v.by_path.len(), 2);
    let (t, px) = db
        .pixels_of(v, "entity/chest/normal")
        .expect("贴图字节应完整可读");
    assert_eq!(px.len(), 4 * 2 * 4);
    // make_png 的 alpha 全为 255，应判为不透明
    assert!(!t.has_alpha);
    assert!(db.pixels_of(v, "entity/chest/trapped").is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn identical_textures_share_pool_bytes() {
    let dir = test_dir("dedupe");
    // 两张尺寸相同、内容逐字节相同的贴图
    let png = crate::test_support::make_png(4, 4, 255);
    let zip = crate::test_support::make_zip(&[
        ("version.json", br#"{"id":"x","world_version":3465}"#.as_slice()),
        (
            "assets/minecraft/textures/entity/boat/oak.png",
            png.as_slice(),
        ),
        (
            "assets/minecraft/textures/entity/boat/spruce.png",
            png.as_slice(),
        ),
    ]);
    let jar_path = dir.join("c.jar");
    std::fs::write(&jar_path, zip).unwrap();
    let jar = JarFile::open(&jar_path.to_string_lossy()).unwrap();
    let db_path = dir.join("entities.dat.z");

    add_version(&db_path, &jar, 3465, "1.20.1").expect("并入失败");
    let db = EntityDb::load(&db_path).unwrap();
    let v = db.version_for(3465).unwrap();
    assert_eq!(
        v.by_path["entity/boat/oak"], v.by_path["entity/boat/spruce"],
        "内容相同的贴图应共享同一个像素区间"
    );
    assert_eq!(db.pixels.len(), 4 * 4 * 4, "像素池里应只存一份");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn readding_replaces_the_version() {
    let dir = test_dir("replace");
    let jar_path = fake_client(&dir, "1.20.1", &[("chest/normal", 2, 2)]);
    let jar = JarFile::open(&jar_path.to_string_lossy()).unwrap();
    let db_path = dir.join("entities.dat.z");

    add_version(&db_path, &jar, 3465, "1.20.1").expect("首次并入失败");
    add_version(&db_path, &jar, 3465, "1.20.1").expect("重复并入失败");

    let db = EntityDb::load(&db_path).unwrap();
    let matches: Vec<_> = db
        .versions
        .iter()
        .filter(|v| v.data_version == 3465)
        .collect();
    assert_eq!(matches.len(), 1, "重复添加应替换而不是叠加");
    assert_eq!(matches[0].by_path.len(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_entity_texture_is_an_error() {
    let dir = test_dir("broken");
    let zip = crate::test_support::make_zip(&[
        ("version.json", br#"{"id":"x","world_version":3465}"#.as_slice()),
        ("assets/minecraft/textures/entity/chest/normal.png", b"not a png"),
    ]);
    let jar_path = dir.join("c.jar");
    std::fs::write(&jar_path, zip).unwrap();
    let jar = JarFile::open(&jar_path.to_string_lossy()).unwrap();
    let db_path = dir.join("entities.dat.z");

    let e = add_version(&db_path, &jar, 3465, "1.20.1").unwrap_err();
    assert!(
        e.to_string().contains("解码失败"),
        "解码失败应报错而不是静默跳过：{e}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn serialize_output_is_deterministic() {
    // 字符串表与版本内路径都排序，同一输入应产出同一字节序列
    let dir = test_dir("deterministic");
    let jar_path = fake_client(
        &dir,
        "1.20.1",
        &[("chest/normal", 2, 2), ("chest/trapped", 2, 2)],
    );
    let jar = JarFile::open(&jar_path.to_string_lossy()).unwrap();
    let db_path = dir.join("entities.dat.z");
    add_version(&db_path, &jar, 3465, "1.20.1").expect("并入失败");
    let first = std::fs::read(&db_path).unwrap();
    // 删除重来，结果应逐字节相同
    std::fs::remove_file(&db_path).unwrap();
    add_version(&db_path, &jar, 3465, "1.20.1").expect("再次并入失败");
    let second = std::fs::read(&db_path).unwrap();
    assert_eq!(first, second, "同一输入应产出同一文件");

    let _ = std::fs::remove_dir_all(&dir);
}
