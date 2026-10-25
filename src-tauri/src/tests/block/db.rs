use super::*;

/// 构造一个最小可用的 pack 字节流，用于验证解析流程。
/// 真实文件由 legacy-cpp 的 packbuild 生成，这里只覆盖格式解析本身。
fn build_minimal_pack() -> Vec<u8> {
    build_minimal_pack_with_dv(3465)
}

/// 构造最小 pack，可指定 dataVersion
fn build_minimal_pack_with_dv(data_version: i32) -> Vec<u8> {
    build_minimal_pack_full(data_version, 0)
}

/// 构造最小 pack，可指定 dataVersion 与版本段的语言表下标。
///
/// 语言表下标做成参数，是为了能构造「越界」的包而不必猜字节偏移——
/// 手算偏移的测试在布局变动后会静默失效（改错了位置却仍指向某个 u32，
/// 于是校验的是别的字段）。
fn build_minimal_pack_full(data_version: i32, lang_idx: u32) -> Vec<u8> {
    let mut w: Vec<u8> = Vec::new();
    let s32 = |w: &mut Vec<u8>, v: i32| w.extend_from_slice(&v.to_le_bytes());
    let u32f = |w: &mut Vec<u8>, v: u32| w.extend_from_slice(&v.to_le_bytes());
    let u16f = |w: &mut Vec<u8>, v: u16| w.extend_from_slice(&v.to_le_bytes());

    w.extend_from_slice(b"PMBLKDB\0");
    u32f(&mut w, 3); // format
    u32f(&mut w, 1); // versions
    u32f(&mut w, 1); // textures
    u32f(&mut w, 1); // models
    u32f(&mut w, 1); // langs

    // 字符串表：索引 0=版本名 1=方块名 2=贴图路径 3=条件 4=en 5=zh
    let strs = ["1.20.1", "minecraft:stone", "block/stone", "", "Stone", "石头"];
    u32f(&mut w, strs.len() as u32);
    for s in strs {
        w.extend_from_slice(s.as_bytes());
        w.push(0);
    }

    // 语言表。注意读取端在「段内重复计数」之后直接按 langCount 逐组读取，
    // 没有独立的「组数」字段——多写一个 u32 会让后续全部错位。
    u32f(&mut w, 1); // 段内重复计数
    u32f(&mut w, 1); // 第 0 组的条目数
    u32f(&mut w, 1); // blockName 下标
    u32f(&mut w, 4); // en 下标
    u32f(&mut w, 5); // zh 下标

    // 贴图表：1 张 2x2 贴图 = 16 字节像素
    u32f(&mut w, 1);
    u16f(&mut w, 2);
    u16f(&mut w, 2);
    u32f(&mut w, 0);
    u32f(&mut w, 16);
    w.push(1); // hasAlpha
    let px: Vec<u8> = (0..16).collect();
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    enc.write_all(&px).unwrap();
    let comp = enc.finish().unwrap();
    u32f(&mut w, 16);
    u32f(&mut w, comp.len() as u32);
    w.extend_from_slice(&comp);

    // 模型表：1 个模型，1 个元素，仅含 Cube 的 down 面
    u32f(&mut w, 1);
    w.push(0); // translucent
    u16f(&mut w, 1); // 元素数
    for v in [0.0f32, 0.0, 0.0] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    for v in [16.0f32, 16.0, 16.0] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.push(0); // hasRot
    w.push(1); // faceMask = 仅位 0
    u32f(&mut w, 0); // texIdx
    for v in [0.0f32, 0.0, 16.0, 16.0] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.push(0xFF); // cullface
    w.extend_from_slice(&(-1i16).to_le_bytes()); // tintindex
    u16f(&mut w, 0); // rotation

    // 版本段
    u32f(&mut w, 1); // 段内重复计数
    u32f(&mut w, 0); // 版本名下标
    s32(&mut w, data_version); // dataVersion
    u32f(&mut w, lang_idx); // langTableIdx
    u32f(&mut w, 1); // 贴图路径数
    u32f(&mut w, 2);
    u32f(&mut w, 0);
    u32f(&mut w, 1); // 方块数
    u32f(&mut w, 1); // 方块名下标
    u16f(&mut w, 1); // 变体数
    u32f(&mut w, 3); // when
    u32f(&mut w, 0); // modelIdx
    w.push(0);
    w.push(0);
    w.push(1); // uvlock
    u16f(&mut w, 0); // multipart 数

    w
}

/// 版本段的语言表下标越界时必须报错，而不是静默给空语言表。
///
/// 静默降级时，该版本的所有方块都显示不出中文名，而用户无从判断是
/// 数据坏了还是本来就没有名字。lang_idx 来自文件，是不可信输入。
#[test]
fn out_of_range_lang_index_is_rejected_on_load() {
    // 包内只有 1 张语言表，下标却写 5
    let bad = build_minimal_pack_full(3465, 5);
    let err = BlockDb::load_from_bytes(&bad, false).expect_err("越界下标应报错");
    let msg = err.to_string();
    assert!(
        msg.contains("语言表下标"),
        "错误信息应指出语言表下标问题，实际：{msg}"
    );
    // 同一份数据、下标合法时应能正常解析，确认失败确实来自下标而非别的字段
    let ok = build_minimal_pack_full(3465, 0);
    assert!(
        BlockDb::load_from_bytes(&ok, false).is_ok(),
        "下标合法时不应报错"
    );
}

#[test]
fn parses_minimal_pack() {
    let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).expect("解析失败");
    assert_eq!(db.versions.len(), 1);
    assert_eq!(db.models.len(), 1);
    assert_eq!(db.textures.len(), 1);
    assert_eq!(db.pixels.len(), 16);

    let v = &db.versions[0];
    assert_eq!(v.name, "1.20.1");
    assert_eq!(v.data_version, 3465);
    assert!(!v.is_overlay);
    assert_eq!(v.blocks.len(), 1);
    assert_eq!(v.blocks[0].name, "minecraft:stone");
    assert_eq!(v.blocks[0].variants.len(), 1);
    assert!(v.blocks[0].variants[0].uvlock);
    assert_eq!(v.texture_by_path.get("block/stone"), Some(&0));
    // 中文名应能查到
    assert_eq!(v.display_name("minecraft:stone"), "石头");
    assert_eq!(v.display_name("minecraft:unknown"), "unknown");
}

#[test]
fn model_face_lookup_follows_mask_order() {
    let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
    let e = &db.models[0].elements[0];
    assert_eq!(e.face_mask, 1);
    assert!(e.face(Face::Down).is_some(), "位 0 对应 Down");
    assert!(e.face(Face::Up).is_none());
    assert_eq!(e.face(Face::Down).unwrap().tintindex, -1);
    assert_eq!(e.face(Face::Down).unwrap().cullface, 0xFF);
}

#[test]
fn texture_pixels_are_sliced_correctly() {
    let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
    let px = db.texture_pixels(0).unwrap();
    assert_eq!(px, &(0u8..16).collect::<Vec<u8>>()[..]);
    assert!(db.texture_pixels(99).is_none());
}

#[test]
fn pick_version_chooses_nearest() {
    let db = BlockDb::load_from_bytes(&build_minimal_pack(), false).unwrap();
    assert_eq!(db.pick_version(3000), Some(0));
    assert_eq!(db.pick_version(3465), Some(0));
}

#[test]
fn bad_magic_is_rejected() {
    let mut b = assert_baseline_parses();
    b[0] = b'X';
    // 断言错误内容而不是只断言「出错了」：只断言 is_err 时，
    // 任何原因导致的失败都会让这条通过（例如改动布局后别处先报错）
    let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
    assert!(e.contains("魔数"), "应指出魔数不匹配，实际：{e}");
}

#[test]
fn unsupported_format_version_is_rejected() {
    let mut b = assert_baseline_parses();
    b[8..12].copy_from_slice(&99u32.to_le_bytes());
    let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
    assert!(e.contains("格式版本"), "错误信息应说明版本不支持：{e}");
}

/// 截断的方块库必须返回错误，而不是 panic，也不能「加载成功」。
///
/// 原实现是 `let _ = ...`——返回值被丢弃，只验证了「不 panic」；
/// 若读取端回归成「截断时也返回 Ok」，旧写法照样通过。同文件下方
/// 已记录过「踩过『因为错误的原因通过』这个坑」，这里把断言补上。
#[test]
fn truncated_data_errors_instead_of_panicking() {
    let full = build_minimal_pack();
    for cut in [10usize, 40, 120, 200, full.len() - 1] {
        let cut = cut.min(full.len());
        let r = BlockDb::load_from_bytes(&full[..cut], false);
        assert!(
            r.is_err(),
            "截断到 {cut} 字节（完整 {} 字节）时应报错，实际加载成功",
            full.len()
        );
    }
}

/// 语言表段内重复计数字段的字节偏移。
///
/// 按布局逐段跳过：文件头(8+5*4) -> 字符串计数与各字符串 -> 该字段。
fn lang_section_count_offset(b: &[u8]) -> usize {
    let mut pos = 8 + 5 * 4;
    let n = u32::from_le_bytes([b[pos], b[pos + 1], b[pos + 2], b[pos + 3]]) as usize;
    pos += 4;
    for _ in 0..n {
        while b[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    pos
}

/// 篡改型测试的公共前置。
///
/// 必须先确认未篡改的 pack 能正常解析：否则篡改后的失败可能来自
/// 别的错误，测试就变成「因为错误的原因通过」——此前就踩过这个坑。
fn assert_baseline_parses() -> Vec<u8> {
    let good = build_minimal_pack();
    assert!(
        BlockDb::load_from_bytes(&good, false).is_ok(),
        "基准 pack 必须能正常解析，否则本测试验证的不是被篡改的那个校验"
    );
    good
}

#[test]
fn section_count_mismatch_is_reported() {
    let mut b = assert_baseline_parses();
    let pos = lang_section_count_offset(&b);
    b[pos..pos + 4].copy_from_slice(&7u32.to_le_bytes());

    let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
    assert!(e.contains("不一致"), "应报段内计数不一致：{e}");
    // 错误信息应指明是哪个段，便于排查
    assert!(e.contains("语言表"), "错误信息应指明段名：{e}");
}

#[test]
fn truncated_string_table_is_rejected() {
    // 截在字符串表中间：缺少终止符或数据不足，都应报错而不是越界
    let mut b = build_minimal_pack();
    b.truncate(8 + 5 * 4 + 4 + 3);
    // 断言是「截断」这一类错误，而不是碰巧因为别的原因失败。
    // 写成「不足 / 长度 / 越界」时这条会失败并暴露出实际措辞是
    // 「字符串缺少终止符，文件已截断」——只断言 is_err 就看不到这一点。
    let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
    assert!(
        e.contains("截断") || e.contains("终止符") || e.contains("不足"),
        "应指出数据被截断，实际：{e}"
    );
}

#[test]
fn texture_out_of_range_is_reported() {
    // 把贴图的长度改到超出像素池，应被显式拒绝而不是留下越界偏移
    let mut b = assert_baseline_parses();
    let mut pos = lang_section_count_offset(&b);
    pos += 4;                 // 语言表段内重复计数
    pos += 4;                 // 第 0 组条目数
    pos += 12;                // 三个字符串下标
    pos += 4;                 // 贴图段内重复计数
    pos += 2 + 2 + 4;         // w, h, offset
    b[pos..pos + 4].copy_from_slice(&9999u32.to_le_bytes()); // len 越界
    let e = BlockDb::load_from_bytes(&b, false).unwrap_err().to_string();
    assert!(e.contains("超出像素池"), "应报贴图范围越界：{e}");
}

/// 负的 dataVersion 不等于「叠加包」。
///
/// 打包脚本会把缺少 data_version 的**普通版本**记为 -1
/// （legacy-cpp/tools/pack_blockdb.py:446），而叠加包与否取决于
/// 资源包类型（导入时用户选择），并不在文件里。若用负值判定，
/// 这类基础版会被误标为叠加包，从而被排除在版本选择之外。
#[test]
fn negative_data_version_is_not_an_overlay() {
    let pack = build_minimal_pack_with_dv(-1);

    // 明确告知是基础版本：不应被判为叠加包
    let base = BlockDb::load_from_bytes(&pack, false).expect("解析失败");
    assert_eq!(base.versions[0].data_version, -1);
    assert!(!base.versions[0].is_overlay, "负 dataVersion 不应被判为叠加包");
    // 基础版本必须能参与版本选择；若被误标为叠加包就会被排除
    assert!(
        base.pick_version(-1).is_some(),
        "基础版本即便缺少 dataVersion 也应能参与版本选择"
    );

    // 明确告知是叠加包：应如实标记
    let ov = BlockDb::load_from_bytes(&pack, true).expect("解析失败");
    assert!(ov.versions[0].is_overlay, "调用方声明的叠加包应被标记");
}

// ---- 容器格式分派 ----

/// 方块库文件用 zlib 容器（与 legacy-cpp 的 uncompress 一致），
/// 而 .litematic 用 gzip。两者容器不同，只按一种解压会让方块库
/// 在发布版整块不可用——这条测试正是为此存在。
#[test]
fn decompress_accepts_zlib_container() {
    let payload = b"PMBLKDB\0some payload bytes";
    let packed = crate::format::nbt::gzip(payload, 6).unwrap();
    // 先把 gzip 解出来，再用 zlib 重新压——本地编码器产出 stored 块，
    // 仍能覆盖「按魔数分派」这一行为本身
    let plain = crate::format::nbt::gunzip(&packed).unwrap();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    z.write_all(&plain).unwrap();
    let zb = z.finish().unwrap();
    assert_eq!(&zb[..1], &[0x78], "zlib 头应以 78 开头");
    assert_eq!(BlockDb::decompress(&zb).unwrap(), payload);
}

#[test]
fn decompress_accepts_gzip_container() {
    let payload = b"litematic-like payload";
    let packed = crate::format::nbt::gzip(payload, 6).unwrap();
    assert_eq!(&packed[..2], &[0x1f, 0x8b], "gzip 头");
    assert_eq!(BlockDb::decompress(&packed).unwrap(), payload);
}

#[test]
fn decompress_passes_through_uncompressed() {
    // 既非 gzip 也非 zlib：按未压缩处理，交给后续解析给出明确错误
    let raw = b"PMBLKDB\0";
    assert_eq!(BlockDb::decompress(raw).unwrap(), raw);
}

#[test]
fn decompress_does_not_misread_plain_bytes_as_zlib() {
    // 首字节低 4 位恰为 8、但头校验不通过的数据不应被当成 zlib
    let raw = b"\x78\x00not-a-zlib-stream";
    assert_eq!(BlockDb::decompress(raw).unwrap(), raw, "头校验不通过应按未压缩处理");
}

// ---- 叠加包合并 ----

/// 造一个最小可用的库：一个基础版本 + 一张贴图 + 一个模型 + 一个方块
fn tiny_db(name: &str, dv: i32, block: &str, tex_path: &str, alpha: u8) -> BlockDb {
    let strings = vec!["1.20.1".to_string(), tex_path.to_string()];
    let model = Model {
        translucent: false,
        elements: vec![ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation: false,
            rot_kind: 0,
            origin: [0.0; 3],
            axis: 0,
            angle: 0.0,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: 0b1,
            // 贴图字段是**字符串表下标**：这里指向 tex_path
            faces: vec![ModelFace {
                texture: 1,
                uv: [0.0, 0.0, 16.0, 16.0],
                cullface: 0xFF,
                tintindex: -1,
                rotation: 0,
            }],
        }],
    };
    let version = VersionDef {
        name: name.into(),
        data_version: dv,
        lang_idx: 0,
        texture_by_path: HashMap::from([(tex_path.to_string(), 0u32)]),
        blocks: vec![BlockDef {
            name: block.into(),
            variants: vec![Variant {
                when: String::new(),
                model_idx: 0,
                rot_x_div90: 0,
                rot_y_div90: 0,
                uvlock: false,
            }],
            multipart: Vec::new(),
        }],
        langs: Vec::new(),
        is_overlay: false,
    };
    BlockDb {
        strings,
        langs: vec![vec![LangEntry {
            block_name: block.into(),
            en: "EN".into(),
            zh: "中".into(),
        }]],
        textures: vec![TextureInfo { w: 1, h: 1, offset: 0, len: 4, has_alpha: alpha != 255 }],
        models: vec![model],
        versions: vec![version],
        pixels: vec![10, 20, 30, alpha],
    }
}

#[test]
fn merge_offsets_model_texture_string_index() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    // 叠加包的模型面原本指向自己的字符串表下标 1，
    // 合并后必须指向基础库字符串表里的 "block/thing"
    let m = base.models.last().unwrap();
    let idx = m.elements[0].faces[0].texture as usize;
    assert_eq!(
        base.strings[idx], "block/thing",
        "模型面的贴图字符串下标必须平移，否则指向基础库里无关的字符串"
    );
}

/// 语言表**存在但为空**是合法状态，合并不应报错。
///
/// 运行期新建的资源包就是这样：pack::build 会放一张空语言表
/// （没有译名，但下标有效）。语言表越界校验若把「表是空的」误判成
/// 「下标越界」，这类包就再也合并不进来。
#[test]
fn merge_accepts_empty_but_valid_lang_table() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let mut pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    // 池里保留一张空表；版本仍指向下标 0
    pack.langs = vec![Vec::new()];
    pack.versions[0].lang_idx = 0;

    base.merge_from(&pack, true)
        .expect("空语言表是合法状态，不应报错");

    let merged = base.versions.last().unwrap();
    assert_eq!(merged.lang_idx as usize, base.langs.len() - 1, "下标应指向并入的那张空表");
    assert!(merged.langs.is_empty(), "空表解析出来就是空的");
}

#[test]
fn merge_offsets_texture_pool_index_and_pixels() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    assert_eq!(base.textures.len(), 2);
    // 叠加包的纹理偏移原本为 0，合并后必须落在自己的像素段上
    let t = &base.textures[1];
    assert_eq!(t.offset, 4, "贴图像素偏移必须加上原缓冲长度");
    assert_eq!(
        base.texture_pixels(1).unwrap(),
        &[10, 20, 30, 128],
        "必须取到叠加包自己的像素，而不是基础库的前 4 字节"
    );
    assert!(t.has_alpha, "alpha 应保留");
}

#[test]
fn merge_offsets_variant_model_index() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    assert_eq!(base.models.len(), 2);
    let ov = base.versions.last().unwrap();
    assert_eq!(
        ov.blocks[0].variants[0].model_idx, 1,
        "变体的模型下标必须平移，否则模组方块会用基础库的模型"
    );
}

#[test]
fn merge_marks_version_as_overlay_and_keeps_lang_index() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    assert!(base.versions[0].is_overlay == false, "基础版本不受影响");
    assert!(base.versions[1].is_overlay, "并入的版本应标记为叠加包");
    assert_eq!(base.langs.len(), 2);
    // 叠加包的语言表下标原本为 0，合并后应指向 1
    assert_eq!(base.versions[1].lang_idx, 1);
    // 版本上的语言表也要挂对
    assert_eq!(base.versions[1].langs[0].zh, "中");
}

/// 语言表下标越界时必须报错，而不是静默给一张空表。
///
/// 原先用 `unwrap_or_default()`：下标取不到时该版本会拿到空语言表，
/// 于是它的方块全都显示不出中文名，而用户无从判断是数据坏了还是
/// 本来就没有名字。多语言表下标是包里的字段，属于不可信输入。
#[test]
fn merge_reports_out_of_range_lang_index() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let mut pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    // 把下标指到语言表数量之外
    pack.versions[0].lang_idx = pack.langs.len() as u32 + 3;
    let err = base.merge_from(&pack, true).expect_err("越界下标应报错");
    let msg = err.to_string();
    assert!(
        msg.contains("超出") && msg.contains("语言表"),
        "错误信息应说明语言表下标越界，实际：{msg}"
    );
}

/// 下标越界与溢出都走同一条报错路径，这里不再单列「溢出」用例。
///
/// 加上基数后的溢出其实**不可达**：lang_idx 与语言表数量都受
/// MAX_LANGS（100_000）限制，两者之和的上界是 200_000。原先写过一个
/// 名为「溢出」的测试，但它的输入（u32::MAX）只会触发越界分支，
/// 断言又是 `超出 || 溢出` 的析取式——名字与实测内容不符，等于没测。
/// 需要覆盖的是越界分支，见 merge_reports_out_of_range_lang_index。

#[test]
fn merged_pack_block_is_findable() {        let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    let ov = base.versions.last().unwrap();
    assert!(
        crate::schematic::model::find_block(&base, ov, "mymod:thing").is_some(),
        "合并后模组方块应能查到"
    );
}

#[test]
fn overlay_attaches_to_nearest_base_version() {
    // 叠加包的 dataVersion 与基础版本不完全相同：应挂到最接近的那个
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let mut second = tiny_db("1.21", 3953, "minecraft:deepslate", "block/deepslate", 255);
    second.versions[0].blocks[0].name = "minecraft:deepslate".into();
    base.merge_from(&second, false).unwrap();

    let pack = tiny_db("mymod", 3400, "mymod:thing", "block/thing", 255);
    base.merge_from(&pack, true).unwrap();

    // 3400 离 3465 更近（差 65），离 3953 差 553
    let ov_idx = base.versions.len() - 1;
    let attached = base.overlays_of(0);
    assert!(attached.contains(&ov_idx), "应挂到 1.20.1：{attached:?}");
    assert!(!base.overlays_of(1).contains(&ov_idx), "不应挂到 1.21");
}

#[test]
fn texture_lookup_falls_back_to_overlay() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/thing", 128);
    base.merge_from(&pack, true).unwrap();

    // "block/thing" 只登记在叠加包里，基础版本查不到；
    // 查找必须能落到叠加包上，否则模组贴图永远用不上
    let sidx = base
        .strings
        .iter()
        .position(|s| s == "block/thing")
        .expect("字符串表里应有该路径") as u32;
    let idx = base.texture_pool_index(0, sidx);
    assert_ne!(idx, NO_TEXTURE, "应通过叠加包找到贴图");
    assert_eq!(base.texture_pixels(idx as usize).unwrap(), &[10, 20, 30, 128]);
}

#[test]
fn texture_lookup_prefers_own_version_over_overlay() {
    // 同名路径在本版本与叠加包里都有：必须用本版本的
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/shared", 255);
    base.pixels = vec![1, 1, 1, 255];
    let pack = tiny_db("mymod", 3465, "mymod:thing", "block/shared", 128);
    base.merge_from(&pack, true).unwrap();

    let sidx = base
        .strings
        .iter()
        .position(|s| s == "block/shared")
        .unwrap() as u32;
    let idx = base.texture_pool_index(0, sidx);
    assert_eq!(idx, 0, "应命中本版本登记的贴图");
    assert_eq!(base.texture_pixels(idx as usize).unwrap(), &[1, 1, 1, 255]);
}

#[test]
fn overlay_does_not_participate_in_version_picking() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("mymod", 9999, "mymod:thing", "block/thing", 255);
    base.merge_from(&pack, true).unwrap();

    let picked = base.pick_version(9999).expect("应选出一个基础版本");
    assert!(
        !base.versions[picked].is_overlay,
        "叠加包不应参与版本选择（与 legacy-cpp 一致）"
    );
    assert_eq!(picked, 0);
}

#[test]
fn merge_rejects_pack_without_versions() {
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let mut empty = tiny_db("x", 1, "x", "x", 255);
    empty.versions.clear();
    let e = base.merge_from(&empty, true).unwrap_err().to_string();
    assert!(e.contains("没有版本数据"), "应说明原因：{e}");
    assert_eq!(base.versions.len(), 1, "失败时不应留下部分合并的结果");
}

#[test]
fn merging_non_overlay_adds_a_base_version() {
    // 版本包（kind=1）并入的是基础版本，应参与版本选择
    let mut base = tiny_db("1.20.1", 3465, "minecraft:stone", "block/stone", 255);
    let pack = tiny_db("1.21", 3953, "minecraft:deepslate", "block/deepslate", 255);
    base.merge_from(&pack, false).unwrap();

    assert!(!base.versions[1].is_overlay, "版本包不应标记为叠加包");
    assert_eq!(base.pick_version(3953), Some(1), "新基础版本应可被选中");
    assert!(base.overlays_of(1).is_empty());
}

impl VersionDef {
    /// 按方块名查中文名，找不到时回退到英文名，再回退到方块名本身
    pub fn display_name(&self, block_name: &str) -> String {
        for e in &self.langs {
            if e.block_name == block_name {
                if !e.zh.is_empty() {
                    return e.zh.clone();
                }
                if !e.en.is_empty() {
                    return e.en.clone();
                }
                break;
            }
        }
        block_name
            .split_once(':')
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| block_name.to_string())
    }
}

impl BlockDb {
    /// 取某个贴图的像素切片
    pub fn texture_pixels(&self, idx: usize) -> Option<&[u8]> {
        let t = self.textures.get(idx)?;
        let start = t.offset as usize;
        self.pixels.get(start..start + t.len as usize)
    }
}
