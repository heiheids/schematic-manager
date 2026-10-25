use super::*;

/// 六个面都带 UV，且 UV 落在 0..16 的模型空间里。
///
/// 这条最容易错：箱子贴图是 64×64，直接写像素值会让 UV 超出 0..16，
/// 采样落到贴图之外，渲染出来几乎全透明（实测不透明像素从 54% 掉到 1%）。
#[test]
fn box_faces_stay_inside_the_model_uv_space() {
    let e = box_element([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], [0.0, 0.0], [64.0, 64.0], "tex");
    assert_eq!(e.faces.len(), 6, "一个盒子应有六个面");
    for (name, f) in &e.faces {
        let uv = f.uv.expect("代码模型的面必须自带 UV");
        for v in uv {
            assert!((0.0..=16.0).contains(&v), "面 {name} 的 UV {v} 超出 0..16 空间");
        }
    }
}

/// 大箱子两半在世界坐标里必须朝邻半那一侧贴齐。
///
/// 期望值不是推出来的，而是量出来的：一份真实投影里，`facing=north` 的一对
/// 是 left 在低 x、right 在高 x；`facing=west` 的一对是 right 在低 z、left 在
/// 高 z。归纳起来，左半始终落在「朝向再转 +90 度」的方向上（north 朝 -z，
/// 转 +90 度是 west 即 -x，所以左半在 -x 侧，自身往 +x 贴）。
///
/// 这条盯住的是「两半各贴各的外侧、中间裂开」那类错误：模型正面画在 +z
/// （与原版箱子渲染代码一致）而旋转少算 180 度时，拼缝方向会整体反过来。
#[test]
fn chest_halves_meet_toward_the_neighbour() {
    use crate::render::transform::rotate_variant;
    let chest = variants_of("minecraft:chest").expect("箱子应有几何");
    // (facing, type, 贴齐的轴, 贴齐的边界值) —— 轴 0=x、2=z
    let cases = [
        ("north", "left", 0usize, 16.0f32),
        ("north", "right", 0, 0.0),
        ("south", "left", 0, 0.0),
        ("south", "right", 0, 16.0),
        ("west", "left", 2, 0.0),
        ("west", "right", 2, 16.0),
        ("east", "left", 2, 16.0),
        ("east", "right", 2, 0.0),
    ];
    for (facing, kind, axis, want_edge) in cases {
        let when = format!("facing={facing},type={kind}");
        let v = chest
            .iter()
            .find(|v| v.when == when)
            .unwrap_or_else(|| panic!("应有变体 {when}"));
        let body = &v.elements[0];
        let a = rotate_variant(body.from, v.rot_x_div90 as u8, v.rot_y_div90 as u8);
        let b = rotate_variant(body.to, v.rot_x_div90 as u8, v.rot_y_div90 as u8);
        let lo = a[axis].min(b[axis]);
        let hi = a[axis].max(b[axis]);
        let name = ["x", "y", "z"][axis];
        assert!(
            (lo - want_edge).abs() < 0.01 || (hi - want_edge).abs() < 0.01,
            "{when} 的箱体在 {name} 上是 {lo}..{hi}，应当贴到 {want_edge}（靠邻半那一侧）"
        );
    }
}

/// 箱子的锁扣要贴在正面那一层（z 14..15），和原版一样，而不是凸出到方块外。
#[test]
fn chest_lock_sits_flush_with_the_front_face() {
    for half in [ChestHalf::Single, ChestHalf::Left, ChestHalf::Right] {
        let elements = chest_elements("entity/chest/normal", half);
        let lock = elements.last().expect("应有锁扣");
        assert_eq!(
            (lock.from[2], lock.to[2]),
            (14.0, 15.0),
            "锁扣应与箱体正面（z=15）齐平"
        );
    }
}

/// 空白贴图必须正好落在拼缝那一面。
///
/// 原版四张半箱贴图各自把**拼缝面**留空（被邻半挡住）。实测：`*_left` 空的
/// 是 +x 面、`*_right` 空的是 -x 面（箱盖与箱体两行一致）。这里把这条事实
/// 写成表，再和几何算出来的拼缝方向对照——两者不一致时，朝外的那一面会被
/// 画成空的，箱子看上去就缺一面，两半也接不上。
///
/// 拼缝方向来自模型坐标系里的占位：左半 x 0..15（拼缝在 -x），右半 x 1..16
/// （拼缝在 +x）；本测试自己旋转角点，因此旋转改了也会被抓到。
#[test]
fn half_textures_leave_the_seam_face_blank() {
    use crate::render::transform::rotate_variant;
    // (贴图, 留空的面) —— 量自原版 entity/chest/*.png
    let blank_face = |tex: &str| match tex {
        "entity/chest/normal_left" | "entity/chest/trapped_left" => "east",
        "entity/chest/normal_right" | "entity/chest/trapped_right" => "west",
        other => panic!("{other} 不是半箱贴图"),
    };
    // 拼缝在世界坐标里的方向（与 chest_halves_meet_toward_the_neighbour 同一张表）
    let seam = |facing: &str, kind: &str| match (facing, kind) {
        ("north", "left") => [1.0, 0.0, 0.0],
        ("north", "right") => [-1.0, 0.0, 0.0],
        ("south", "left") => [-1.0, 0.0, 0.0],
        ("south", "right") => [1.0, 0.0, 0.0],
        ("west", "left") => [0.0, 0.0, -1.0],
        ("west", "right") => [0.0, 0.0, 1.0],
        ("east", "left") => [0.0, 0.0, 1.0],
        ("east", "right") => [0.0, 0.0, -1.0],
        _ => panic!("未覆盖 {facing}/{kind}"),
    };

    let chest = variants_of("minecraft:chest").expect("箱子应有几何");
    for facing in ["north", "east", "south", "west"] {
        for kind in ["left", "right"] {
            let when = format!("facing={facing},type={kind}");
            let v = chest.iter().find(|v| v.when == when).expect("应有变体");
            let body = &v.elements[0];
            let texture = body
                .faces
                .iter()
                .map(|(_, f)| f.texture.clone())
                .next()
                .expect("箱体应有贴图");
            let blank = blank_face(&texture);

            // 算留空的那一面转到世界坐标后朝哪边。取该面在模型坐标里的
            // **面心**（-x/+x 面分别在 from[0]/to[0]，y、z 取中点），整点过一遍
            // 旋转，再相对方块中心 (8,8,8) 看它偏在哪个轴向上。
            // 不能只比 x：转 90 度后 ±x 面会跑到 ±z 上。
            let face_center_model = match blank {
                "east" => [body.to[0], (body.from[1] + body.to[1]) / 2.0, (body.from[2] + body.to[2]) / 2.0],
                "west" => [body.from[0], (body.from[1] + body.to[1]) / 2.0, (body.from[2] + body.to[2]) / 2.0],
                other => panic!("半箱贴图只在 ±x 留空，实际是 {other}"),
            };
            let p = rotate_variant(
                face_center_model,
                v.rot_x_div90 as u8,
                v.rot_y_div90 as u8,
            );
            let d = [p[0] - 8.0, p[1] - 8.0, p[2] - 8.0];
            // 取绝对值最大的那个轴作为朝向
            let blank_dir = if d[0].abs() >= d[2].abs() {
                [if d[0] > 0.0 { 1.0 } else { -1.0 }, 0.0, 0.0]
            } else {
                [0.0, 0.0, if d[2] > 0.0 { 1.0 } else { -1.0 }]
            };
            let want = seam(facing, kind);
            assert_eq!(
                blank_dir, want,
                "{when}：留空的是 {texture} 的 {blank} 面，转到世界坐标后朝 {blank_dir:?}，\
                 但它应当朝拼缝方向 {want:?}——贴图与两半对反了"
            );
        }
    }
}

/// 箱子贴图上的区域要对得上：箱盖的顶面取木纹方块、底面取内腔深色方块。
///
/// 这条用真实取值钉住 64×64 贴图到 0..16 空间的换算关系与首行顺序：算错时
/// 箱子看起来就是一顶黑盖子。
#[test]
fn chest_lid_samples_the_expected_atlas_regions() {
    let elements = chest_elements("entity/chest/normal", ChestHalf::Single);
    let lid = elements
        .iter()
        .find(|e| e.from == [1.0, 9.0, 1.0] && e.to == [15.0, 14.0, 15.0])
        .expect("应有箱盖");
    let uv_of = |name: &str| {
        lid.faces
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, f)| f.uv)
            .unwrap_or_else(|| panic!("箱盖应有 {name} 面"))
    };
    // 木纹（顶面）在贴图 (28,0)-(42,14)，换算到 0..16 空间是 (7,0)-(10.5,3.5)
    assert_eq!(uv_of("up"), [7.0, 0.0, 10.5, 3.5], "顶面应取木纹那一格");
    // 内腔深色（底面）在 (14,0)-(28,14)
    assert_eq!(uv_of("down"), [3.5, 0.0, 7.0, 3.5], "底面应取内腔那一格");
    // 侧面高 5 像素，位于第二行
    assert_eq!(uv_of("east"), [0.0, 3.5, 3.5, 4.75]);
}

/// 箱子一族：四个朝向 × 三种形态（单箱、大箱左半、右半），贴图按方块与形态区分。
#[test]
fn chest_family_expands_facings_and_halves_with_their_own_textures() {
    for (block, want) in [
        (
            "minecraft:chest",
            vec![
                "facing=north,type=single",
                "facing=north,type=left",
                "facing=north,type=right",
                "facing=east,type=single",
                "facing=east,type=left",
                "facing=east,type=right",
                "facing=south,type=single",
                "facing=south,type=left",
                "facing=south,type=right",
                "facing=west,type=single",
                "facing=west,type=left",
                "facing=west,type=right",
            ],
        ),
        // 末影箱不能拼，也没有 `type` 属性：只有四个朝向各一条
        ("minecraft:ender_chest", vec![
            "facing=north",
            "facing=east",
            "facing=south",
            "facing=west",
        ]),
    ] {
        let vs = variants_of(block).unwrap_or_else(|| panic!("{block} 应有几何"));
        let got: Vec<String> = vs.iter().map(|v| v.when.clone()).collect();
        assert_eq!(got, want, "{block} 的变体组合不对");
        for v in &vs {
            assert!(!v.elements.is_empty(), "{block} 的几何不应为空");
        }
    }

    // 左右两半必须用各自那一半的贴图，否则并排的两个箱子接不成一只大箱子
    let chest = variants_of("minecraft:chest").unwrap();
    let tex_of = |when: &str| {
        chest
            .iter()
            .find(|v| v.when == when)
            .map(|v| v.elements[0].faces[0].1.texture.clone())
            .unwrap_or_default()
    };
    assert_eq!(tex_of("facing=north,type=single"), "entity/chest/normal");
    // 半箱贴图与 type 是交叉对应的，理由见 chest_texture 的注释：
    // 每张贴图把「拼缝那一面」留空，而左半占 x 0..15（拼缝在模型 -x 一侧）
    assert_eq!(tex_of("facing=north,type=left"), "entity/chest/normal_right");
    assert_eq!(tex_of("facing=north,type=right"), "entity/chest/normal_left");

    // 陷阱箱有自己的左右半贴图
    let trapped = variants_of("minecraft:trapped_chest").unwrap();
    let trapped_tex = |when: &str| {
        trapped
            .iter()
            .find(|v| v.when == when)
            .map(|v| v.elements[0].faces[0].1.texture.clone())
            .unwrap_or_default()
    };
    // 陷阱箱同理：左右半贴图与 type 交叉对应
    assert_eq!(trapped_tex("facing=north,type=left"), "entity/chest/trapped_right");
    assert_eq!(trapped_tex("facing=north,type=right"), "entity/chest/trapped_left");
}

/// 大箱子的两半各宽 15 格、都把靠拼缝的一侧顶到方块边界，锁扣也落在拼缝上。
///
/// 拼缝在模型坐标系里的位置：左半占 x 0..15（拼缝在 -x），右半占 x 1..16
/// （拼缝在 +x）。两半按同一套旋转摆到世界里之后，各自顶到靠邻半的那条
/// 方块边界上，接缝正好合在一条线上。
#[test]
fn chest_halves_meet_at_the_seam() {
    let chest = variants_of("minecraft:chest").unwrap();
    let bounds = |when: &str| {
        let v = chest.iter().find(|v| v.when == when).expect("应有该变体");
        (
            v.elements[0].from[0],
            v.elements[0].to[0],
            v.elements[2].from[0],
            v.elements[2].to[0],
        )
    };
    // (箱体左, 箱体右, 锁扣左, 锁扣右)
    assert_eq!(bounds("facing=north,type=single"), (1.0, 15.0, 7.0, 9.0));
    assert_eq!(bounds("facing=north,type=left"), (0.0, 15.0, 0.0, 1.0), "左半拼缝在 -x 侧");
    assert_eq!(bounds("facing=north,type=right"), (1.0, 16.0, 15.0, 16.0), "右半拼缝在 +x 侧");
}

/// 不属于代码模型的方块必须返回 None，否则会把普通方块的模型顶掉。
#[test]
fn ordinary_blocks_are_left_alone() {
    for block in ["minecraft:stone", "minecraft:oak_planks", "minecraft:bell"] {
        assert!(variants_of(block).is_none(), "{block} 不该由代码模型接管");
    }
}

/// 首行两格的实际位置：顶面在第二格，紧接在底面之后。
///
/// 次行第二格曾误写成次行的第三格起点（u+dz+dx+dz），只有在 dx == dz 时
/// 才碰巧正确；箱子恰好 dx == dz 所以没暴露，床（16×16×6）一用就错。
/// 首行的顶/底顺序则是另一处错误：写成 [顶][底] 时箱子顶面会取到内腔深色。
#[test]
fn top_and_bottom_regions_are_ordered_bottom_first() {
    let r = box_regions([0.0, 0.0], [16.0, 16.0, 6.0]);
    assert_eq!(r[1], [6.0, 0.0, 22.0, 6.0], "底面在第一格");
    assert_eq!(r[0], [22.0, 0.0, 38.0, 6.0], "顶面紧接底面，宽度同为 dx");
    // 次行依次是 dz、dx、dz、dx
    assert_eq!(r[2], [0.0, 6.0, 6.0, 22.0], "x- 面");
    assert_eq!(r[3], [6.0, 6.0, 22.0, 22.0], "z- 面");
    assert_eq!(r[4], [22.0, 6.0, 28.0, 22.0], "x+ 面");
    assert_eq!(r[5], [28.0, 6.0, 44.0, 22.0], "z+ 面");
}

/// 床：头尾两半各 4 个朝向，且枕头只出现在床头那一半的顶面。
#[test]
fn bed_puts_the_pillow_on_the_head_half_top() {
    let vs = variants_of("minecraft:red_bed").expect("床应有几何");
    assert_eq!(vs.len(), 8, "4 个朝向 × 头尾两半");
    let head = vs.iter().find(|v| v.when == "facing=north,part=head").unwrap();
    let foot = vs.iter().find(|v| v.when == "facing=north,part=foot").unwrap();
    let top_uv = |v: &CodeVariant| {
        v.elements[0]
            .faces
            .iter()
            .find(|(n, _)| n == "up")
            .and_then(|(_, f)| f.uv)
            .expect("床垫应有顶面")
    };
    // 床头板展开在 (0,0)，其 -z 面（枕头）转到顶面后是 (3.5,1.5)-(5.5,5.5)
    assert_ne!(top_uv(head), top_uv(foot), "头尾两半的顶面应取自不同区域");
    for v in [head, foot] {
        for (_, f) in &v.elements[0].faces {
            assert_eq!(f.texture, "entity/bed/red", "应使用床自己的贴图");
        }
    }
    // 床腿：头尾都有四条（总元素数 = 床垫 1 + 腿 4）
    assert_eq!(head.elements.len(), 5);
    assert_eq!(foot.elements.len(), 5, "床尾没有床头板，但同样有四条腿");
}

/// 潜影盒：六个朝向、贴图名带 shulker_ 前缀。
///
/// 贴图路径曾漏掉前缀（写成 entity/shulker/white），渲染出来是一片空白。
#[test]
fn shulker_box_covers_six_facings_and_prefixes_its_texture() {
    let vs = variants_of("minecraft:white_shulker_box").expect("潜影盒应有几何");
    assert_eq!(vs.len(), 6);
    let facings: Vec<&str> = vs.iter().map(|v| v.when.as_str()).collect();
    for want in ["facing=up", "facing=down", "facing=north", "facing=south", "facing=east", "facing=west"] {
        assert!(facings.contains(&want), "缺少 {want}");
    }
    for v in &vs {
        for e in &v.elements {
            for (_, f) in &e.faces {
                assert_eq!(f.texture, "entity/shulker/shulker_white");
            }
        }
    }
    // 未染色的那种：贴图不带颜色后缀
    let plain = variants_of("minecraft:shulker_box").expect("未染色的潜影盒应有几何");
    assert_eq!(plain[0].elements[0].faces[0].1.texture, "entity/shulker/shulker");
    // 朝上不旋转，朝下的 x 轴转两档
    let up = vs.iter().find(|v| v.when == "facing=up").unwrap();
    let down = vs.iter().find(|v| v.when == "facing=down").unwrap();
    assert_eq!((up.rot_x_div90, up.rot_y_div90), (0, 0));
    assert_eq!((down.rot_x_div90, down.rot_y_div90), (2, 0));
}

/// 告示牌：立式 16 档朝向（每档 22.5°，写在元素上），墙式 4 个朝向且没有立柱。
#[test]
fn signs_expand_floor_rotations_and_drop_the_post_on_walls() {
    let standing = variants_of("minecraft:oak_sign").expect("立式牌应有几何");
    assert_eq!(standing.len(), 16);
    assert_eq!(standing[4].when, "rotation=4");
    let angle_of = |v: &CodeVariant| v.elements[0].rotation.as_ref().map(|r| r.angle);
    assert_eq!(angle_of(&standing[0]), Some(0.0));
    assert_eq!(angle_of(&standing[4]), Some(90.0), "第 4 档是 90 度");
    assert_eq!(angle_of(&standing[1]), Some(22.5), "每档 22.5 度");
    // 立式牌有立柱（牌面 + 立柱），且牌面宽 24
    assert_eq!(standing[0].elements.len(), 2);
    let board = &standing[0].elements[0];
    assert_eq!(board.to[0] - board.from[0], 24.0, "牌面宽 24，比方块还宽");

    let wall = variants_of("minecraft:oak_wall_sign").expect("墙式牌应有几何");
    assert_eq!(wall.len(), 4);
    assert_eq!(wall[0].elements.len(), 1, "墙式牌没有立柱");
    assert!(wall[0].elements[0].rotation.is_none(), "墙式牌靠变体旋转，不需元素旋转");
}

/// 头颅：贴图像素尺寸按类型区分，同一个头方块因此对应不同的展开区域。
#[test]
fn skulls_scale_uv_by_their_own_texture_size() {
    // 8×8×8 的头方块，展开原点 (0,0)：首行两格是 (8,0)-(16,8) 的底与
    // (16,0)-(24,8) 的顶——顶面在第二格（与箱子同一个规则）
    let up_uv = |block: &str| {
        let vs = variants_of(block).expect("应有几何");
        let floor = vs.iter().find(|v| v.when == "rotation=0").unwrap();
        floor.elements[0]
            .faces
            .iter()
            .find(|(n, _)| n == "up")
            .and_then(|(_, f)| f.uv)
            .unwrap()
    };
    // 64×64 贴图：顶面 (16,0)-(24,8) 换算后是 (4,0)-(6,2)
    assert_eq!(up_uv("minecraft:player_head"), [4.0, 0.0, 6.0, 2.0]);
    // 64×32 贴图：纵向比例不同，换算后是 (4,0)-(6,4)
    assert_eq!(up_uv("minecraft:skeleton_skull"), [4.0, 0.0, 6.0, 4.0]);

    let skull = variants_of("minecraft:skeleton_skull").unwrap();
    assert_eq!(skull.len(), 20, "地面 16 档朝向 + 墙上 4 个朝向");
    let wall = skull.iter().find(|v| v.when == "facing=north").unwrap();
    assert_eq!(wall.elements[0].from[1], 4.0, "墙上的头挂在半空");
    // 龙首与猪灵头不是立方体，不接管
    assert!(variants_of("minecraft:dragon_head").is_none());
    assert!(variants_of("minecraft:piglin_head").is_none());
}
