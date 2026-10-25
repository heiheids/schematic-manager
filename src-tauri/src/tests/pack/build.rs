use super::*;

/// 内存中的假 jar
struct FakeJar {
    files: HashMap<String, Vec<u8>>,
}

impl FakeJar {
    fn new() -> Self {
        Self { files: HashMap::new() }
    }
    fn add(&mut self, name: &str, content: &str) -> &mut Self {
        self.files.insert(name.to_string(), content.as_bytes().to_vec());
        self
    }
    fn add_bytes(&mut self, name: &str, content: Vec<u8>) -> &mut Self {
        self.files.insert(name.to_string(), content);
        self
    }
}

impl JarSource for FakeJar {
    fn entries(&self) -> Vec<String> {
        let mut v: Vec<String> = self.files.keys().cloned().collect();
        v.sort(); // 与真实 zip 一样给出稳定顺序
        v
    }
    fn read(&self, name: &str) -> AppResult<Vec<u8>> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| AppError::Parse(format!("假 jar 里没有 {name}")))
    }
}

/// 一个自包含的最小客户端 jar
fn minimal_jar() -> FakeJar {
    let mut j = FakeJar::new();
    j.add(
        "version.json",
        r##"{"id":"1.20.1","world_version":3465}"##,
    );
    j.add(
        "assets/minecraft/blockstates/stone.json",
        r##"{"variants":{"":{"model":"block/stone"}}}"##,
    );
    j.add(
        "assets/minecraft/models/block/stone.json",
        r##"{"textures":{"all":"block/stone"},"elements":[{
            "from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#all","cullface":"up"}}
        }]}"##,
    );
    // 一张 2x2 的不透明 PNG
    j.add_bytes("assets/minecraft/textures/block/stone.png", tiny_png(false));
    j
}

/// 生成一张 NxN 的 8 位 RGBA PNG。
/// 自行构造而非依赖外部文件：测试要能在任何环境下运行。
fn tiny_png(with_alpha: bool) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        crate::format::zip::crc32(data)
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let mut crc_in = kind.to_vec();
        crc_in.extend_from_slice(body);
        out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    }

    let (w, h) = (2u32, 2u32);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.push(8); // 位深
    ihdr.push(6); // RGBA
    ihdr.extend_from_slice(&[0, 0, 0]); // 压缩/滤波/隔行
    chunk(&mut png, b"IHDR", &ihdr);

    // 每行：滤波字节 0 + 4 个像素
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&[(x * 100) as u8, (y * 100) as u8, 7, if with_alpha { 128 } else { 255 }]);
        }
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    enc.write_all(&raw).unwrap();
    let comp = enc.finish().unwrap();
    assert!(!comp.is_empty(), "zlib 编码结果不应为空（原始 {len} 字节）", len = raw.len());
    assert!(comp.len() >= 6, "zlib 结果至少应含头与校验（实际 {}）", comp.len());
    chunk(&mut png, b"IDAT", &comp);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn no_texture(_p: &str) -> Option<(u16, u16, Vec<u8>)> {
    None
}

/// 直接验证贴图登记本身（不经主流程），便于定位失败环节
#[test]
fn intern_texture_registers_for_existing_entry() {
    let mut jar = FakeJar::new();
    jar.add_bytes("assets/minecraft/textures/block/stone.png", tiny_png(false));
    let mut strings = StringPool::default();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    let mut by_path = HashMap::new();
    intern_texture(
        "minecraft:block/stone",
        &jar,
        "minecraft",
        &mut strings,
        &mut textures,
        &mut by_path,
        &mut |_p: &str| None,
    );
    assert_eq!(textures.len(), 1, "应从 jar 内解码并登记");
    assert!(by_path.contains_key("minecraft:block/stone"));
    assert!(strings.get("minecraft:block/stone").is_some());
}

/// 注入的加载器应优先于 jar 内解码
#[test]
fn intern_texture_uses_injected_loader() {
    let jar = FakeJar::new(); // jar 里什么都没有
    let mut strings = StringPool::default();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    let mut by_path = HashMap::new();
    intern_texture(
        "minecraft:block/x",
        &jar,
        "minecraft",
        &mut strings,
        &mut textures,
        &mut by_path,
        &mut |_p: &str| Some((1u16, 1u16, vec![1u8, 2, 3, 4])),
    );
    assert_eq!(textures.len(), 1, "注入的加载器应被采用");
}

/// tiny_png 生成的字节必须能被 png::decode 解出。
/// 若这条失败，说明是测试助手构造的 PNG 有问题，而不是产线代码。
#[test]
fn tiny_png_is_decodable() {
    let bytes = tiny_png(false);
    eprintln!("[诊断] PNG 总长 {}，前 40 字节 {:02x?}", bytes.len(), &bytes[..40.min(bytes.len())]);
    let img = crate::format::png::decode(&bytes).expect("自造的 PNG 应能解码");
    assert_eq!((img.width, img.height), (2, 2));
    assert_eq!(img.rgba.len(), 16);
    // 像素值应与构造时一致
    assert_eq!(&img.rgba[0..4], &[0, 0, 7, 255]);
}

#[test]
fn animated_texture_keeps_only_the_first_frame() {
    // 动画贴图是一条竖排帧带（火焰是 16×512 的 32 帧）。模型的 UV 按整张图
    // 取样，帧带被整条压进一个面就是花屏，因此只留第一帧作静态展示。
    let mut jar = FakeJar::new();
    jar.add_bytes(
        "assets/minecraft/textures/block/fire_0.png",
        crate::test_support::make_png(2, 8, 255),
    );
    jar.add(
        "assets/minecraft/textures/block/fire_0.png.mcmeta",
        r#"{"animation":{"frametime":1}}"#,
    );

    let mut strings = StringPool::default();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    let mut by_path = HashMap::new();
    intern_texture(
        "minecraft:block/fire_0",
        &jar,
        "minecraft",
        &mut strings,
        &mut textures,
        &mut by_path,
        &mut no_texture,
    );

    let items = textures.items();
    let t = items.first().expect("应登记一张贴图");
    assert_eq!((t.w, t.h), (2, 2), "16×8 的 4 帧带应只留第一帧 2×2");
    assert_eq!(t.rgba.len(), 2 * 2 * 4);
}

#[test]
fn animated_texture_honours_explicit_frame_size() {
    // 帧尺寸显式给出时以它为准（原版多数动画只写 frametime，帧为正方形）
    let mut jar = FakeJar::new();
    jar.add_bytes(
        "assets/minecraft/textures/block/lava_still.png",
        crate::test_support::make_png(4, 16, 255),
    );
    jar.add(
        "assets/minecraft/textures/block/lava_still.png.mcmeta",
        r#"{"animation":{"frametime":2,"frameWidth":4,"frameHeight":8}}"#,
    );

    let mut strings = StringPool::default();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    let mut by_path = HashMap::new();
    intern_texture(
        "minecraft:block/lava_still",
        &jar,
        "minecraft",
        &mut strings,
        &mut textures,
        &mut by_path,
        &mut no_texture,
    );

    let items = textures.items();
    let t = items.first().expect("应登记一张贴图");
    assert_eq!((t.w, t.h), (4, 8));
}

#[test]
fn static_texture_with_unrelated_mcmeta_is_untouched() {
    // 只有 villager 段的 .mcmeta 不是动画，别把静态贴图裁坏；
    // 没有 .mcmeta 的高瘦贴图（例如月相图）同样保持原样
    let mut jar = FakeJar::new();
    jar.add_bytes(
        "assets/minecraft/textures/entity/villager.png",
        crate::test_support::make_png(2, 8, 255),
    );
    jar.add(
        "assets/minecraft/textures/entity/villager.png.mcmeta",
        r#"{"villager":{"hat":"full"}}"#,
    );
    jar.add_bytes(
        "assets/minecraft/textures/block/moon_phases.png",
        crate::test_support::make_png(2, 8, 255),
    );

    let mut strings = StringPool::default();
    let mut textures: ContentPool<TexEntry> = ContentPool::new();
    let mut by_path = HashMap::new();
    for p in ["minecraft:entity/villager", "minecraft:block/moon_phases"] {
        intern_texture(
            p,
            &jar,
            "minecraft",
            &mut strings,
            &mut textures,
            &mut by_path,
            &mut no_texture,
        );
    }

    for t in textures.items() {
        assert_eq!((t.w, t.h), (2, 8), "非动画贴图不该被裁剪");
    }
}

#[test]
fn builds_pack_from_minimal_jar() {
    let jar = minimal_jar();
    let built = build(&jar, "test", 0, false, no_texture).expect("构建失败");
    assert_eq!(built.stats.namespaces, 1);
    assert_eq!(built.stats.blocks, 1);
    assert_eq!(built.stats.models, 1);
    assert_eq!(built.stats.failed_blocks, 0);
    assert!(!built.data.is_empty());
}

/// 箱子一类的方块在 jar 里只有空壳模型（没有 elements），
/// 几何改由 code_models 提供；建库结果里必须真的有几何与贴图。
#[test]
fn code_model_blocks_get_geometry_from_a_placeholder_model() {
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/blockstates/chest.json",
        r##"{"variants":{"":{"model":"block/chest"}}}"##,
    );
    // 与真实 jar 一致：只有粒子贴图声明，没有任何 elements
    jar.add(
        "assets/minecraft/models/block/chest.json",
        r##"{"textures":{"particle":"block/oak_planks"}}"##,
    );
    jar.add_bytes("assets/minecraft/textures/entity/chest/normal.png", tiny_png(false));

    let built = build(&jar, "test", 0, false, no_texture).expect("构建失败");
    let db = BlockDb::load_from_bytes(&built.data, false).expect("读取失败");
    let chest = db.versions[0]
        .blocks
        .iter()
        .find(|b| b.name == "minecraft:chest")
        .expect("箱子应进入方块库");

    assert_eq!(chest.variants.len(), 12, "四个朝向 × 单箱/左半/右半");
    for v in &chest.variants {
        let m = &db.models[v.model_idx as usize];
        assert!(!m.elements.is_empty(), "箱子必须有几何，否则预览里什么都看不到");
    }
    // 贴图确实指到了 jar 里的实体贴图
    let single = chest
        .variants
        .iter()
        .find(|v| v.when == "facing=north,type=single")
        .expect("应有单箱变体");
    let m = &db.models[single.model_idx as usize];
    let path = &db.strings[m.elements[0].faces[0].texture as usize];
    assert_eq!(path, "minecraft:entity/chest/normal");
}

/// x 轴旋转必须从代码模型一路写进方块库。
///
/// 潜影盒朝侧面时靠 x 轴旋转摆位；这一环丢失的话，六种朝向都会画成盖朝上，
/// 而每种朝向的贴图都不缺，界面看不出少东西。
#[test]
fn code_model_keeps_x_rotation_of_its_variants() {
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/blockstates/white_shulker_box.json",
        r##"{"variants":{"":{"model":"block/white_shulker_box"}}}"##,
    );
    jar.add(
        "assets/minecraft/models/block/white_shulker_box.json",
        r##"{"textures":{"particle":"block/shulker_box"}}"##,
    );
    jar.add_bytes(
        "assets/minecraft/textures/entity/shulker/shulker_white.png",
        tiny_png(false),
    );

    let built = build(&jar, "test", 0, false, no_texture).expect("构建失败");
    let db = BlockDb::load_from_bytes(&built.data, false).expect("读取失败");
    let shulker = db.versions[0]
        .blocks
        .iter()
        .find(|b| b.name == "minecraft:white_shulker_box")
        .expect("潜影盒应进入方块库");

    let rot_of = |when: &str| {
        shulker
            .variants
            .iter()
            .find(|v| v.when == when)
            .map(|v| (v.rot_x_div90, v.rot_y_div90))
    };
    assert_eq!(rot_of("facing=up"), Some((0, 0)));
    assert_eq!(rot_of("facing=down"), Some((2, 0)), "朝下要绕 x 转两档");
    assert_eq!(rot_of("facing=east"), Some((1, 1)), "朝东要 x、y 各转一档");
}

#[test]
fn built_pack_is_readable_and_preserves_blocks() {
    // 端到端：构建出的字节必须能被读取器解析，且方块与模型对上
    let jar = minimal_jar();
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).expect("读取失败");

    assert_eq!(db.versions.len(), 1);
    let v = &db.versions[0];
    assert_eq!(v.name, "1.20.1", "版本名应取自 version.json");
    assert_eq!(v.data_version, 3465);
    assert_eq!(v.blocks.len(), 1);
    assert_eq!(v.blocks[0].name, "minecraft:stone");
    assert_eq!(v.blocks[0].variants.len(), 1);
    assert_eq!(db.models.len(), 1);
    assert_eq!(db.models[0].elements.len(), 1);
    // 面应指向「字符串表下标」，且该下标处的字符串是贴图路径
    let face = &db.models[0].elements[0].faces[0];
    let path = &db.strings[face.texture as usize];
    assert_eq!(path, "minecraft:block/stone");
}

#[test]
fn language_table_is_taken_from_the_jar() {
    // 方块名来自语言文件。少了这张表，方块列表与统计表的名字整列都空着，
    // 而基础库是在用户机器上从 jar 现构建的，必须自己把语言表带上。
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/lang/en_us.json",
        r#"{"block.minecraft.stone":"Stone","block.minecraft.dirt":"Dirt",
            "item.minecraft.water_bucket":"Water Bucket",
            "block.minecraft.oak_slab":"Oak Slab","item.minecraft.oak_slab":"Oak Slab Item",
            "block.minecraft.banner.base.black":"Black Base",
            "item.minecraft.stone":"Stone Item"}"#,
    );
    jar.add(
        "assets/minecraft/lang/zh_cn.json",
        r#"{"block.minecraft.stone":"石头","item.minecraft.water_bucket":"水桶"}"#,
    );

    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    let table = db.langs.first().expect("应带一张语言表");

    let stone = table
        .iter()
        .find(|e| e.block_name == "minecraft:stone")
        .expect("石头应有语言条目");
    assert_eq!(stone.en, "Stone");
    assert_eq!(stone.zh, "石头");
    assert_eq!(
        table
            .iter()
            .filter(|e| e.block_name == "minecraft:stone")
            .count(),
        1,
        "同名时方块键应覆盖物品键，而不是留下两条"
    );

    // 统计折算后有些行代表的是物品（水折成水桶），物品名也要能查到
    let bucket = table
        .iter()
        .find(|e| e.block_name == "minecraft:water_bucket")
        .expect("物品键也要收进语言表");
    assert_eq!(bucket.zh, "水桶");

    // 同名物品与方块：以方块键的译名为准
    let slab = table
        .iter()
        .find(|e| e.block_name == "minecraft:oak_slab")
        .expect("台阶应有语言条目");
    assert_eq!(slab.en, "Oak Slab");

    // 中文文件里没有的名字仍要保留英文：查询侧会从别的版本表补中文，
    // 这里若整条丢掉，连英文名都会跟着消失
    let dirt = table
        .iter()
        .find(|e| e.block_name == "minecraft:dirt")
        .expect("只有英文的名字也要收进来");
    assert_eq!(dirt.en, "Dirt");
    assert_eq!(dirt.zh, "");

    // 余下部分带点号的是图案名
    assert!(
        !table.iter().any(|e| e.block_name.contains("banner.base")),
        "图案名不该被当成方块名"
    );
}

#[test]
fn empty_language_files_do_not_break_the_build() {
    // 语言文件缺失或坏掉时，方块与贴图仍应建出来，只是没有名字
    let jar = minimal_jar();
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert_eq!(db.langs.len(), 1, "仍应带一张（空的）语言表");
    assert!(db.langs[0].is_empty());
    assert_eq!(db.versions[0].blocks.len(), 1, "方块不受影响");
}

#[test]
fn explicit_data_version_overrides_version_json() {
    // 模组包：叠加到哪个版本由调用方指定，jar 里没有该信息
    let jar = minimal_jar();
    let built = build(&jar, "mod", 9999, true, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, true).unwrap();
    assert_eq!(db.versions[0].data_version, 9999, "显式给出的版本号应优先生效");
    assert!(db.versions[0].is_overlay, "叠加包标志应透传");
}

/// version.json 里的 world_version 超出 i32 时，
/// 必须按「没有版本号」处理，不能截断成一个看着合法的值。
///
/// 4294970761 是刻意挑的：`4294970761 as i32 == 3465`，正好是 1.20.1 的
/// 编号。用 `as i32` 时这个损坏的版本号会被当成合法版本，把资源包
/// 归到错误的游戏版本上。
#[test]
fn out_of_range_world_version_is_treated_as_unknown() {
    let mut jar = minimal_jar();
    jar.add("version.json", r##"{"id":"1.20.1","world_version":4294970761}"##);
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();

    assert_eq!(
        db.versions[0].data_version, 0,
        "超出 i32 的 world_version 应视为未知，而不是截断成 3465"
    );
    // 版本名仍照常取自 id
    assert_eq!(db.versions[0].name, "1.20.1");
}

/// 负数 world_version 应原样保留，不能被改写成某个看着合法的正数。
///
/// 本项目里 dataVersion 为负表示「打包时缺少版本号」
/// （见 block/db.rs 的说明与 legacy-cpp/tools/pack_blockdb.py），
/// 因此负值属于既定的「未知」表示，不应被替换成猜测值。
#[test]
fn negative_world_version_stays_negative() {
    let mut jar = minimal_jar();
    jar.add("version.json", r##"{"id":"1.20.1","world_version":-5}"##);
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert_eq!(
        db.versions[0].data_version, -5,
        "负值表示「未知」，应原样保留而不是改写成合法版本号"
    );
    assert_eq!(db.versions[0].name, "1.20.1");
}

/// 临界值 i32::MAX 应被接受，确认判据不是「一律拒绝大数」。
#[test]
fn world_version_at_i32_max_is_accepted() {
    let mut jar = minimal_jar();
    let max = i32::MAX;
    jar.add(
        "version.json",
        &format!(r##"{{"id":"1.20.1","world_version":{max}}}"##),
    );
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert_eq!(db.versions[0].data_version, max);
}

#[test]
fn texture_is_decoded_and_registered() {
    let jar = minimal_jar();
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.textures, 1, "应登记一张贴图");

    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert_eq!(db.textures.len(), 1);
    let t = &db.textures[0];
    assert_eq!((t.w, t.h), (2, 2));
    assert_eq!(t.len, 16, "2x2 RGBA 应为 16 字节");
    // 像素池里应有该贴图的数据
    assert_eq!(db.pixels.len(), 16);
    // 路径映射应指向该贴图
    assert_eq!(
        db.versions[0].texture_by_path.get("minecraft:block/stone"),
        Some(&0)
    );
}

#[test]
fn identical_textures_are_deduplicated() {
    // 两个方块引用内容相同的贴图：应只登记一张
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/blockstates/copy.json",
        r##"{"variants":{"":{"model":"block/copy"}}}"##,
    );
    // copy 的模型引用另一条路径，但 PNG 内容完全相同
    jar.add(
        "assets/minecraft/models/block/copy.json",
        r##"{"textures":{"all":"block/copy"},"elements":[{
            "from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#all"}}
        }]}"##,
    );
    jar.add_bytes("assets/minecraft/textures/block/copy.png", tiny_png(false));

    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.blocks, 2);
    assert_eq!(
        built.stats.textures, 1,
        "内容相同的两张贴图应去重为一张"
    );
}

#[test]
fn identical_models_are_deduplicated() {
    // 两个方块的模型内容完全相同：应只登记一个模型
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/blockstates/twin.json",
        r##"{"variants":{"":{"model":"block/stone"}}}"##,
    );
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.blocks, 2);
    assert_eq!(built.stats.models, 1, "内容相同的模型应去重");
}

#[test]
fn parent_chain_is_resolved() {
    // 子模型只声明 parent 与 textures，elements 继承自父模型
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/models/block/base.json",
        r##"{"textures":{"all":"block/stone"},"elements":[{
            "from":[0,0,0],"to":[16,16,16],
            "faces":{"up":{"texture":"#all"}}
        }]}"##,
    );
    jar.add(
        "assets/minecraft/blockstates/child.json",
        r##"{"variants":{"":{"model":"block/child"}}}"##,
    );
    jar.add(
        "assets/minecraft/models/block/child.json",
        r##"{"parent":"block/base"}"##,
    );
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    // child 应通过 parent 拿到 elements
    let child = db
        .versions[0]
        .blocks
        .iter()
        .find(|b| b.name == "minecraft:child")
        .expect("应有 child 方块");
    let mi = child.variants[0].model_idx as usize;
    assert_eq!(
        db.models[mi].elements.len(),
        1,
        "父模型的 elements 应被继承"
    );
}

#[test]
fn multipart_is_preserved() {
    let mut jar = minimal_jar();
    jar.add(
        "assets/minecraft/blockstates/fence.json",
        r##"{"multipart":[
            {"apply":{"model":"block/stone"}},
            {"when":{"north":"true"},"apply":{"model":"block/stone"}}
        ]}"##,
    );
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    let fence = db
        .versions[0]
        .blocks
        .iter()
        .find(|b| b.name == "minecraft:fence")
        .expect("应有 fence 方块");
    assert_eq!(fence.multipart.len(), 2);
    assert_eq!(fence.multipart[0].when, "");
    assert_eq!(fence.multipart[1].when, "north=true");
}

#[test]
fn jar_without_assets_is_rejected() {
    let mut jar = FakeJar::new();
    jar.add("META-INF/MANIFEST.MF", "x");
    let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
    assert!(e.contains("没有找到任何资源"), "应说明 jar 内容不符：{e}");
}

#[test]
fn jar_with_assets_but_no_blockstates_is_rejected() {
    let mut jar = FakeJar::new();
    jar.add("assets/minecraft/textures/block/x.png", "not really png");
    let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
    assert!(e.contains("blockstates"), "应说明缺少 blockstates：{e}");
}

#[test]
fn broken_blockstates_are_counted_not_fatal() {
    // 一个能解析、一个坏掉：坏掉的计入统计，整体仍然成功
    let mut jar = minimal_jar();
    jar.add("assets/minecraft/blockstates/broken.json", "{ not json");
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.failed_blockstates, 1);
    assert_eq!(built.stats.blocks, 1, "好的那个仍应产出");
}

#[test]
fn all_blockstates_broken_is_an_error() {
    let mut jar = FakeJar::new();
    jar.add("assets/minecraft/blockstates/broken.json", "{ not json");
    let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
    assert!(e.contains("没有解析出任何方块"), "应说明无方块产出：{e}");
}

#[test]
fn missing_model_marks_block_failed() {
    // blockstate 引用了不存在的模型
    let mut jar = FakeJar::new();
    jar.add(
        "assets/minecraft/blockstates/ghost.json",
        r##"{"variants":{"":{"model":"block/ghost"}}}"##,
    );
    let e = build(&jar, "test", 0, false, no_texture).unwrap_err().to_string();
    assert!(e.contains("没有解析出任何方块"), "模型缺失应导致该方块被跳过：{e}");
}

#[test]
fn injected_texture_loader_is_used() {
    // 调用方提供的加载器应被采用；这里给一张 1x1 的红色贴图
    let mut jar = minimal_jar();
    // 删掉 jar 内的贴图，只靠注入的加载器
    jar.files.remove("assets/minecraft/textures/block/stone.png");
    let built = build(&jar, "test", 0, false, |_p: &str| {
        Some((1u16, 1u16, vec![255u8, 0, 0, 255]))
    })
    .unwrap();
    assert_eq!(built.stats.textures, 1);
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert_eq!((db.textures[0].w, db.textures[0].h), (1, 1));
    assert_eq!(db.pixels, vec![255, 0, 0, 255]);
}

#[test]
fn missing_texture_does_not_fail_the_model() {
    // 贴图缺失时仍应产出模型（面会指向一个无贴图的下标），
    // 只是贴图池里没有对应记录
    let mut jar = minimal_jar();
    jar.files.remove("assets/minecraft/textures/block/stone.png");
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.textures, 0, "没有贴图数据时不应登记贴图");
    assert_eq!(built.stats.blocks, 1, "方块仍应产出");
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    assert!(db.textures.is_empty());
    assert_eq!(db.models[0].elements.len(), 1);
}

/// 变体旋转必须按磁盘格式（90 度倍数）写出，读回后与 blockstate 一致。
///
/// blockstate 的 `x` / `y` 是角度，磁盘格式是「90 度倍数」。
/// 曾经写出端用 `角度 % 4`：y=90 写成 2（读回 180 度）、
/// y=180 写成 0（读回 0 度），导入资源包后楼梯、门、告示牌全转错方向。
#[test]
fn variant_rotation_roundtrips_as_multiples_of_90() {
    for (deg, want) in [(0u64, 0u8), (90, 1), (180, 2), (270, 3)] {
        let json = format!(
            r##"{{"variants":{{"":{{"model":"block/stone","x":{deg},"y":{deg}}}}}}}"##
        );
        let mut jar = minimal_jar();
        jar.add("assets/minecraft/blockstates/spin.json", &json);
        let built = build(&jar, "test", 0, false, no_texture).unwrap();
        let db = BlockDb::load_from_bytes(&built.data, false).expect("读取失败");
        let blk = db.versions[0]
            .blocks
            .iter()
            .find(|b| b.name == "minecraft:spin")
            .expect("应有 spin 方块");
        let v = &blk.variants[0];
        assert_eq!(
            (v.rot_x_div90, v.rot_y_div90),
            (want, want),
            "blockstate 里的 {deg} 度应写出并读回 {want} 个 90 度"
        );
    }
}

#[test]
fn block_name_comes_from_filename() {        assert_eq!(
        block_name_of("assets/minecraft/blockstates/oak_stairs.json").unwrap(),
        "oak_stairs"
    );
    assert_eq!(
        block_name_of("assets/mymod/blockstates/deep/x.json").unwrap(),
        "deep/x"
    );
    assert!(block_name_of("assets/minecraft/models/block/x.json").is_none());
    assert!(block_name_of("assets/minecraft/blockstates/.json").is_none());
}

#[test]
fn multiple_namespaces_are_merged() {
    let mut jar = minimal_jar();
    jar.add(
        "assets/mymod/blockstates/thing.json",
        r##"{"variants":{"":{"model":"mymod:block/thing"}}}"##,
    );
    jar.add(
        "assets/mymod/models/block/thing.json",
        r##"{"textures":{"all":"minecraft:block/stone"},"elements":[{
            "from":[0,0,0],"to":[8,8,8],
            "faces":{"up":{"texture":"#all"}}
        }]}"##,
    );
    let built = build(&jar, "test", 0, false, no_texture).unwrap();
    assert_eq!(built.stats.namespaces, 2);
    assert_eq!(built.stats.blocks, 2, "两个命名空间的方块应合并到一个版本里");
    let db = BlockDb::load_from_bytes(&built.data, false).unwrap();
    let names: Vec<&str> = db.versions[0].blocks.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"minecraft:stone"));
    assert!(names.contains(&"mymod:thing"));
}

#[test]
fn deterministic_output_for_same_jar() {
    let a = build(&minimal_jar(), "test", 0, false, no_texture).unwrap();
    let b = build(&minimal_jar(), "test", 0, false, no_texture).unwrap();
    assert_eq!(a.data, b.data, "同一 jar 两次构建应产出完全相同的字节");
}
