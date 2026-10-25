use super::*;

/// 16x16、alpha 全 255 的合成贴图。
fn tex_a() -> Vec<u8> {
    let mut v = Vec::with_capacity(16 * 16 * 4);
    for i in 0..16 * 16 {
        v.extend_from_slice(&[(i % 251) as u8, (i * 7 % 253) as u8, 9, 255]);
    }
    v
}

/// 8x8、带透明像素的合成贴图。
fn tex_b() -> Vec<u8> {
    vec![7u8; 8 * 8 * 4]
}

/// 一个小型合成库：两个版本共享贴图表，路径到贴图的映射各不相同。
/// 构造结构体后经写入端序列化再读回，覆盖 字符串表/贴图表/像素池/版本
/// 全链路，比手写字节更能跟着格式演进。
fn sample_db() -> EntityDb {
    let a = tex_a();
    let b = tex_b();
    let mut pixels = Vec::new();
    pixels.extend_from_slice(&a);
    pixels.extend_from_slice(&b);
    EntityDb {
        paths: vec![
            "entity/chest/ender".into(),
            "entity/chest/normal".into(),
            "entity/shulker/shulker".into(),
        ],
        textures: vec![
            EntityTexture {
                w: 16,
                h: 16,
                offset: 0,
                len: a.len() as u32,
                has_alpha: false,
            },
            EntityTexture {
                w: 8,
                h: 8,
                offset: a.len() as u32,
                len: b.len() as u32,
                has_alpha: true,
            },
        ],
        versions: vec![
            EntityVersion {
                data_version: 2225,
                name: "1.15".into(),
                by_path: HashMap::from([
                    ("entity/chest/normal".to_string(), 0u32),
                    ("entity/shulker/shulker".to_string(), 1u32),
                ]),
            },
            EntityVersion {
                data_version: 3465,
                name: "1.20.1".into(),
                by_path: HashMap::from([
                    ("entity/chest/normal".to_string(), 0u32),
                    ("entity/chest/ender".to_string(), 1u32),
                ]),
            },
        ],
        pixels,
    }
}

fn roundtrip(db: &EntityDb) -> EntityDb {
    let bytes = crate::pack::entity_write::serialize(db).expect("序列化失败");
    EntityDb::load_from_bytes(&bytes).expect("写出的库应能读回")
}

/// 读回后按路径取到的必须是当初放进去的那张贴图：
/// 断言尺寸、像素与透明标记，而不只是「取到了」。
#[test]
fn roundtrip_preserves_lookups() {
    let db = roundtrip(&sample_db());
    let ver = db.version_for(3465).expect("应有 dataVersion 3465");
    assert_eq!(ver.name, "1.20.1");

    let (t, px) = db
        .pixels_of(ver, "entity/chest/normal")
        .expect("应有普通箱子贴图");
    assert_eq!((t.w, t.h), (16, 16));
    assert!(!t.has_alpha);
    assert_eq!(px, tex_a().as_slice(), "像素应与写入时一致");

    let (t, px) = db
        .pixels_of(ver, "entity/chest/ender")
        .expect("应有末影箱贴图");
    assert_eq!((t.w, t.h), (8, 8));
    assert!(t.has_alpha);
    assert_eq!(px, tex_b().as_slice());
}

/// 贴图路径与版本必须一一对得上：同一个 dataVersion 下，
/// 不同方块实体的贴图不能是同一张。
#[test]
fn paths_map_to_distinct_textures() {
    let db = roundtrip(&sample_db());
    let ver = db.version_for(3465).expect("应有 1.20.1");
    let (_, ender) = db.pixels_of(ver, "entity/chest/ender").unwrap();
    let (_, normal) = db.pixels_of(ver, "entity/chest/normal").unwrap();
    assert_ne!(ender, normal, "末影箱与普通箱子的贴图不应相同");
    // 调用两次必须得到同一张（路径 -> 下标的映射是确定的）
    let (a, _) = db.pixels_of(ver, "entity/chest/normal").unwrap();
    let (b, _) = db.pixels_of(ver, "entity/chest/normal").unwrap();
    assert_eq!(a, b);
}

/// 同样的像素被两个版本的路径引用时，应指向同一个贴图下标：
/// 运行时是把整块像素池读进内存的，重复存会让内存随版本数膨胀。
#[test]
fn shared_pixels_keep_one_pool_entry() {
    let db = roundtrip(&sample_db());
    let v115 = db.version_for(2225).expect("应有 1.15");
    let v121 = db.version_for(3465).expect("应有 1.20.1");
    assert_eq!(
        v115.by_path["entity/chest/normal"], v121.by_path["entity/chest/normal"],
        "两个版本里相同的贴图应共享下标"
    );
    // 跨版本同一张图（1.15 的潜影盒 == 1.20.1 的末影箱）同样共享
    assert_eq!(
        v115.by_path["entity/shulker/shulker"], v121.by_path["entity/chest/ender"]
    );
    assert_eq!(db.pixels.len(), tex_a().len() + tex_b().len());
}

/// 版本查不到、路径查不到都要返回 None，而不是给一张空图。
#[test]
fn unknown_version_or_path_yields_nothing() {
    let db = roundtrip(&sample_db());
    assert!(db.version_for(-1).is_none(), "不存在的 dataVersion 应查不到");
    let ver = db.version_for(3465).expect("应有 1.20.1");
    assert!(db.pixels_of(ver, "entity/does_not_exist").is_none());
}

/// 损坏数据必须报错，不能返回半截像素或 panic。
#[test]
fn corrupt_data_is_rejected() {
    // 魔数正确但被截断
    let mut bad = db_pack_head();
    bad.truncate(bad.len().min(20));
    assert!(EntityDb::load_from_bytes(&bad).is_err(), "截断的数据应报错");

    // 魔数不对
    let mut wrong = db_pack_head();
    wrong[0] = b'X';
    let e = EntityDb::load_from_bytes(&wrong).unwrap_err().to_string();
    assert!(e.contains("魔数"), "{e}");

    // 格式版本不对
    let mut newer = db_pack_head();
    newer[4..8].copy_from_slice(&999u32.to_le_bytes());
    let e = EntityDb::load_from_bytes(&newer).unwrap_err().to_string();
    assert!(e.contains("格式版本"), "{e}");
}

/// 一个最小的合法包头（魔数 + 格式版本 + 各段计数为 0）。
fn db_pack_head() -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(MAGIC);
    v.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    v.extend_from_slice(&0u32.to_le_bytes()); // 字符串
    v.extend_from_slice(&0u32.to_le_bytes()); // 贴图
    v.extend_from_slice(&0u32.to_le_bytes()); // raw_size
    v.extend_from_slice(&0u32.to_le_bytes()); // comp_size
    v.extend_from_slice(&0u32.to_le_bytes()); // 版本
    v
}

/// 空库（各段为 0）是合法输入，不应报错。
#[test]
fn empty_pack_is_valid() {
    let db = EntityDb::load_from_bytes(&db_pack_head()).expect("空包应能解析");
    assert!(db.textures.is_empty());
    assert!(db.versions.is_empty());
    assert!(db.pixels.is_empty());
}
