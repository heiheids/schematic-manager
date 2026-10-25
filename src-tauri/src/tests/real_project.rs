//! 针对**任意真实投影集合**的端到端核对（按需运行）。
//!
//! 为什么单独一个模块
//!   仓库内只随源码分发一个小体积样本（见 src/tests/data/README.md，出于体积与
//!   许可证考虑）。而「真实投影 + 随包方块库」这个组合才最接近用户的实际
//!   使用：方块库的版本表或贴图池与投影对不上时，各模块自己的单元测试照样
//!   全绿，界面却显示不出方块——而界面无法在无图形环境下自动测试。
//!
//! 怎么运行
//!   SM_REAL_DIR=/path/to/projects cargo test --lib -- --ignored --nocapture
//!
//! 这些用例都标了 `#[ignore]`，而不是「读不到环境变量就返回」：后者在测试
//! 报告里会显示为通过，看不出其实一次都没跑。标了 ignore 会如实显示为
//! 「未运行」，需要跑时用 `--ignored` 显式调用。

use crate::schematic::litematic::BlockState;
use std::path::{Path, PathBuf};

/// 递归收集目录下的 `.litematic`
fn collect_litematic(dir: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().map(|x| x == "litematic").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(Path::new(dir), &mut files);
    files.sort();
    assert!(!files.is_empty(), "SM_REAL_DIR 下没有 .litematic");
    files
}

fn real_dir() -> String {
    std::env::var("SM_REAL_DIR").expect("需设置 SM_REAL_DIR 指向真实投影所在目录")
}

/// 真实数据用例共用的方块库。
///
/// 库文件不再随包分发：用 SM_BLOCK_DB 指向一份（通常是「版本库」在
/// app 数据目录下生成的单版本包），且版本应覆盖用例数据的 dataVersion。
fn env_db() -> crate::block::db::BlockDb {
    let path = std::env::var("SM_BLOCK_DB")
        .expect("需设置 SM_BLOCK_DB 指向方块库文件（如「版本库」生成的单版本包）");
    crate::block::db::BlockDb::load(&path, false).expect("方块库载入失败")
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().to_string()
}

/// 临时工作目录，测试结束即删除。
///
/// 用 `std::env::temp_dir()`（即 TEMP/TMP 指向的位置）。注意在本仓库的
/// Windows 开发环境里系统临时目录不可写，需要先把 TEMP 指到仓库内的
/// `_temp`（`tools/typecheck/run.ps1` 也是这么做的），否则这里会失败——
/// 因此把实际路径与底层错误一并写进提示，而不是只说「无法创建」。
fn work_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sm_real_{tag}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| {
        panic!(
            "无法创建临时目录 {}：{e}\n（系统临时目录不可写时，请把 TEMP/TMP 指向可写目录）",
            d.display()
        )
    });
    d
}

/// 每个真实投影：载入 → 区域结构自检 → NBT 往返零损失 → 方块统计。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_parses_and_roundtrips() {
    let files = collect_litematic(&real_dir());
    let mut failed: Vec<String> = Vec::new();

    for f in &files {
        let name = file_name(f);

        let sc = match crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        // 区域结构：归一化后尺寸全正，索引数等于体积
        for r in &sc.regions {
            if !r.size.iter().all(|&v| v > 0) {
                failed.push(format!("{name}: 尺寸非正 {:?}", r.size));
                break;
            }
            let expect = (r.size[0] as usize) * (r.size[1] as usize) * (r.size[2] as usize);
            if r.indices.len() != expect {
                failed.push(format!("{name}: 索引数 {} != 体积 {}", r.indices.len(), expect));
                break;
            }
        }

        // 写出路径必须零信息损失
        let roundtrip = (|| -> Result<(), String> {
            let bytes = std::fs::read(f).map_err(|e| e.to_string())?;
            let root = crate::format::nbt::parse_maybe_gzip(&bytes).map_err(|e| e.to_string())?;
            let out = crate::format::nbt::serialize_gzip(&root, "", 6).map_err(|e| e.to_string())?;
            let back = crate::format::nbt::parse_maybe_gzip(&out).map_err(|e| e.to_string())?;
            if root != back {
                return Err("NBT 往返前后不一致".into());
            }
            Ok(())
        })();
        if let Err(e) = roundtrip {
            failed.push(format!("{name}: {e}"));
        }

        let an = sc.analyze();
        println!(
            "  [OK]   {name}  区域 {}  非空气 {}  状态种类 {}",
            sc.regions.len(),
            an.non_air_total,
            an.blocks_by_state.len()
        );
    }

    println!("\n  共 {} 个文件，失败 {} 个", files.len(), failed.len());
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 真实投影里的染色方块必须真的被染色，且是**按通道**染色。
///
/// 这条对应界面上的「红石、树叶、藤蔓没颜色」：贴图是灰度的，
/// 靠逐顶点颜色乘上去才成形。只测 tint 函数的单元测试不足以证明
/// 「整条链路真的把颜色带到了顶点上」——中间任何一环漏掉，
/// 单元测试仍然全绿，而画面依旧灰。因此这里走真实数据：
/// 从真实投影取出这些方块产生的四边形，核对它们的颜色值。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_tinted_blocks_actually_get_colored() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = env_db();
    // 方块名 -> (四边形数, 第一个染色值)
    let mut seen: BTreeMap<String, (usize, [f32; 3])> = BTreeMap::new();
    let mut total_quads = 0usize;
    let mut tinted_quads = 0usize;

    for f in &files {
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };
        let quads = crate::render::visibility::collect_quads(&sc, &db, vi);
        // 逐四边形反查它属于哪个方块（按 block_pos 对应回调色板）
        let mut name_at: BTreeMap<[i32; 3], String> = BTreeMap::new();
        for r in &sc.regions {
            for (i, pi) in r.indices.iter().enumerate() {
                let Some(st) = r.palette.get(*pi as usize) else { continue };
                let local = r.pos_from_index(i);
                let (Some(x), Some(y), Some(z)) = (
                    r.pos[0].checked_add(local[0]),
                    r.pos[1].checked_add(local[1]),
                    r.pos[2].checked_add(local[2]),
                ) else {
                    continue;
                };
                name_at.insert([x, y, z], st.name.clone());
            }
        }
        for q in &quads {
            total_quads += 1;
            let is_tinted = q.tint != crate::render::tint::NO_TINT;
            if is_tinted {
                tinted_quads += 1;
            }
            if let Some(n) = name_at.get(&q.block_pos) {
                seen.entry(n.clone()).or_insert((0, q.tint)).0 += 1;
            }
        }
    }

    assert!(total_quads > 0, "真实投影没有产出任何四边形");
    println!("  四边形合计 {total_quads}，其中染色 {tinted_quads}");

    // 逐个方块核对**具体颜色**，而不是只核对「不是全 1」。
    // 只查「有没有变色」会漏掉「染成了错误的颜色」——樱花树叶被染成
    // 树叶绿时仍然是「变了色」，但观感明显不对。
    let expect = |name: &str, want: [f32; 3]| -> Result<(), String> {
        match seen.get(name) {
            None => Ok(()), // 这批投影里没出现该方块，不算失败
            Some((cnt, got)) => {
                if *got == want {
                    Ok(())
                } else {
                    Err(format!(
                        "{name}（{cnt} 个四边形）染色为 ({:.3},{:.3},{:.3})，\
                         应为 ({:.3},{:.3},{:.3})",
                        got[0], got[1], got[2], want[0], want[1], want[2]
                    ))
                }
            }
        }
    };

    let mut wrong: Vec<String> = Vec::new();
    let leaf = crate::render::tint::tint_color("minecraft:oak_leaves", &[], 0);
    let grass = crate::render::tint::tint_color("minecraft:grass_block", &[], 0);
    let no_tint = crate::render::tint::NO_TINT;

    let mut checked = 0usize;
    println!("  --- 出现过的染色方块 ---");
    for (name, (cnt, tint)) in &seen {
        if *tint == no_tint {
            continue;
        }
        println!("  {cnt:>6}  {name}  tint=({:.3}, {:.3}, {:.3})", tint[0], tint[1], tint[2]);
    }

    for pair in [
        ("minecraft:oak_leaves", leaf),
        ("minecraft:jungle_leaves", leaf),
        ("minecraft:acacia_leaves", leaf),
        ("minecraft:dark_oak_leaves", leaf),
        ("minecraft:mangrove_leaves", leaf),
        ("minecraft:vine", leaf),
        ("minecraft:sugar_cane", grass),
        // 樱花树叶必须保持原色（贴图自带粉色）
        ("minecraft:cherry_leaves", no_tint),
    ] {
        if seen.contains_key(pair.0) {
            checked += 1;
        }
        if let Err(e) = expect(pair.0, pair.1) {
            wrong.push(e);
        }
    }

    // 红石线单独核对：必须是红色系（红通道最高），且随信号强度变化
    if let Some((cnt, t)) = seen.get("minecraft:redstone_wire") {
        checked += 1;
        println!("  {cnt:>6}  minecraft:redstone_wire  tint=({:.3}, {:.3}, {:.3})", t[0], t[1], t[2]);
        if !(t[0] > t[1] && t[0] > t[2]) {
            wrong.push(format!("redstone_wire 染色 ({:.3},{:.3},{:.3}) 不是红色系", t[0], t[1], t[2]));
        }
    }

    assert!(
        checked >= 2,
        "这批投影里只找到 {checked} 种待验证的染色方块，样本不足；\
         请换一批含树叶/草/红石的投影再跑"
    );
    assert!(wrong.is_empty(), "以下染色方块未正确上色：\n{}", wrong.join("\n"));
}

/// 活塞的邻面剔除必须守住所依赖的**几何不变量**。
///
/// 对应界面上的「活塞有时候会缺面」：「有时候」是关键词——同一个活塞，
/// 周围有没有实心方块会改变结果，因此这里把邻居场景也当作输入维度。
///
/// 只在会**误判**的方向上断言，不自己重算一遍剔除逻辑（重算等于把被测
/// 逻辑抄一遍，抄错了两边一起错）。这里守两条与旋转无关的不变量：
///
/// 1. 邻居越少，可见面不可能更多。孤立时必须 >= 任意有邻居的场景。
/// 2. 带 cullface=none 的面在任何邻居配置下都必须保留。
///    伸出活塞的正面就是这种（活塞杆靠它显示），
///    若它被当成可剔除，活塞就会「缺一块」。
///
/// 另外验证「六面围石头」时可见面确实变少——若剔除完全失效，
/// 这条会失败，说明本用例不是空转。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_piston_culling_respects_geometry() {
    let db = env_db();
    let vi = db.pick_version(3465).expect("基础库缺少 1.20.1");

    let mut states: Vec<(String, String)> = Vec::new();
    for facing in ["north", "south", "east", "west", "up", "down"] {
        for extended in ["false", "true"] {
            for name in ["minecraft:piston", "minecraft:sticky_piston"] {
                states.push((name.to_string(), format!("extended={extended},facing={facing}")));
            }
        }
    }

    // 邻居一律用**绝对坐标**写出来，避免偏移与下标互转时出错。
    // 活塞恒在 (1,1,1)。
    let scenarios: [(&str, Vec<[i32; 3]>); 3] = [
        ("孤立", vec![]),
        (
            "六面围石头",
            vec![
                [0, 1, 1],
                [2, 1, 1],
                [1, 0, 1],
                [1, 2, 1],
                [1, 1, 0],
                [1, 1, 2],
            ],
        ),
        ("上方压石头", vec![[1, 2, 1]]),
    ];

    let mut failures: Vec<String> = Vec::new();
    let mut shrank_somewhere = false;
    let mut checked = 0usize;

    for (name, propstr) in &states {
        let props: Vec<(String, String)> = propstr
            .split(',')
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                (it.next().unwrap().into(), it.next().unwrap_or("").into())
            })
            .collect();
        let st = BlockState { name: name.clone(), props: props.clone() };

        // 该方块模型里 cullface=none 的面有几个（这些面永远不该被剔除）
        let models = crate::schematic::model::resolve_models(&db, &db.versions[vi], &st);
        let mut no_cull_faces = 0usize;
        let mut total_faces = 0usize;
        for rm in &models {
            let Some(m) = db.models.get(rm.model_idx as usize) else { continue };
            for elem in &m.elements {
                for f in 0..6u8 {
                    if elem.face_mask & (1u8 << f) == 0 {
                        continue;
                    }
                    total_faces += 1;
                    let fe = match f {
                        0 => crate::block::db::Face::Down,
                        1 => crate::block::db::Face::Up,
                        2 => crate::block::db::Face::North,
                        3 => crate::block::db::Face::South,
                        4 => crate::block::db::Face::West,
                        _ => crate::block::db::Face::East,
                    };
                    if elem.face(fe).map(|mf| mf.cullface == 0xFF).unwrap_or(false) {
                        no_cull_faces += 1;
                    }
                }
            }
        }

        let mut per_scenario: Vec<(&str, usize)> = Vec::new();
        for (sname, neighbours) in &scenarios {
            // 用 Region::index 自己算下标：索引顺序是 X 最快、Z 次之、Y 最慢，
            // 手算 x*9+y*3+z 会错位（我在写这条用例时正是这样错过一次）。
            let size = [3usize, 3, 3];
            let mut palette = vec![st.clone()];
            let mut indices = vec![0i32; 27];
            for n in neighbours {
                let stone = BlockState::new("minecraft:stone");
                let pi = match palette
                    .iter()
                    .position(|s| s.name == stone.name && s.props == stone.props)
                {
                    Some(x) => x,
                    None => {
                        palette.push(stone);
                        palette.len() - 1
                    }
                };
                let idx = n[0] as usize
                    + n[2] as usize * size[0]
                    + n[1] as usize * size[0] * size[2];
                indices[idx] = pi as i32;
            }
            let air = BlockState::new("minecraft:air");
            let air_pi = {
                palette.push(air);
                palette.len() - 1
            };
            for i in 0..indices.len() {
                let x = (i % size[0]) as i32;
                let z = ((i / size[0]) % size[2]) as i32;
                let y = (i / (size[0] * size[2])) as i32;
                if [x, y, z] == [1, 1, 1]
                    || neighbours.iter().any(|n| n[0] == x && n[1] == y && n[2] == z)
                {
                    continue;
                }
                indices[i] = air_pi as i32;
            }

            let sc = crate::schematic::litematic::Schematic {
                root: crate::format::nbt::Value::Compound(vec![]),
                regions: vec![crate::schematic::litematic::Region {
                    name: "r".into(),
                    pos: [0, 0, 0],
                    size: [3, 3, 3],
                    volume: 27,
                    bits: 5,
                    palette,
                    indices,
                    ..Default::default()
                }],
                path: String::new(),
                meta_modified: false,
                blocks_modified: false,
            };

            let quads = crate::render::visibility::collect_quads(&sc, &db, vi);
            let got = quads.iter().filter(|q| q.block_pos == [1, 1, 1]).count();
            checked += 1;
            per_scenario.push((sname, got));

            // 不变量 2：cullface=none 的面必须全都在
            if got < no_cull_faces {
                failures.push(format!(
                    "{name} [{propstr}] 场景「{sname}」：只产出 {got} 个面，\
                     但仅 cullface=none 的面就有 {no_cull_faces} 个——不可剔除的面被剔掉了"
                ));
            }
            // 可见面不可能超过模型总面数
            if got > total_faces {
                failures.push(format!(
                    "{name} [{propstr}] 场景「{sname}」：产出 {got} 个面，超过模型总面数 {total_faces}"
                ));
            }
        }

        // 不变量 1：邻居越少，可见面不可能更少
        let isolated = per_scenario[0].1;
        for (sname, got) in &per_scenario[1..] {
            if *got > isolated {
                failures.push(format!(
                    "{name} [{propstr}]：场景「{sname}」产出 {got} 个面，\
                     多于完全孤立时的 {isolated} 个——邻居变多反而看到更多面，剔除方向反了"
                ));
            }
            if *got < isolated {
                shrank_somewhere = true;
            }
        }
    }

    assert!(checked > 0, "没有验证到任何活塞状态");
    assert!(
        shrank_somewhere,
        "所有场景的可见面数都相同，说明剔除根本没生效——本用例在空转"
    );
    assert!(
        failures.is_empty(),
        "活塞剔除违反几何不变量：\n{}",
        failures.join("\n")
    );
    println!("  已核对 {checked} 个「活塞状态 × 邻居场景」组合，剔除符合几何不变量");
}

/// 真实投影里活塞与红石类方块的状态必须都能解析出模型。
///
/// 对应界面上的「活塞有时候会缺面」：活塞是**按状态切换模型**的方块
/// （extended 决定底座是完整立方体还是短了一截；facing 决定朝向；
/// piston_head 还分 short 与 type）。只要状态解析失败，
/// 这个方块就一个面都不画——表现就是「凭空少了一块」。
///
/// 这里用真实数据核对：把所有 piston / sticky_piston / piston_head /
/// redstone_* 的实际状态串拿出来，逐个确认能解析出模型，
/// 并统计覆盖到的 facing 与 extended 组合。
/// 遍历全部真实投影，核对**每个方块**都没有丢掉不可剔除的面。
///
/// 这是「活塞缺面」这类问题最宽的一张网：不限定方块种类，而是对每个方块实例
/// 独立算出「模型里 cullface=none 的面有几个」，再核对实际产出的四边形数
/// 不少于它。cullface=none 表示「任何情况下都不剔除」，因此这些面必须出现
/// 在输出里；少一个就说明该方块会缺一块。
///
/// 与只测活塞的用例互补：那一条覆盖各种朝向与邻居摆法，
/// 这一条覆盖真实世界里出现的全部方块与全部实际摆法。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_no_block_loses_a_non_cullable_face() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = env_db();

    let mut total_blocks = 0usize;
    let mut total_quads = 0usize;
    // 失败样例：(方块名, 属性串, 应有面数, 实际面数, 投影名)
    let mut failures: Vec<String> = Vec::new();
    // 出现过的方块种类数，用于说明覆盖面
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();

    for f in &files {
        let file_label = file_name(f);
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };
        let version = &db.versions[vi];

        // 每个方块状态算一次「不可剔除的面数」，同一状态只算一遍
        let mut no_cull_cache: BTreeMap<i32, usize> = BTreeMap::new();

        let quads = crate::render::visibility::collect_quads(&sc, &db, vi);
        total_quads += quads.len();

        // 按方块坐标统计实际产出的四边形数
        let mut per_block: BTreeMap<[i32; 3], usize> = BTreeMap::new();
        for q in &quads {
            *per_block.entry(q.block_pos).or_insert(0) += 1;
        }

        for r in &sc.regions {
            for (i, pi) in r.indices.iter().enumerate() {
                let Some(st) = r.palette.get(*pi as usize) else { continue };
                if crate::render::visibility::is_render_skipped(&st.name) {
                    continue;
                }
                let local = r.pos_from_index(i);
                let (Some(x), Some(y), Some(z)) = (
                    r.pos[0].checked_add(local[0]),
                    r.pos[1].checked_add(local[1]),
                    r.pos[2].checked_add(local[2]),
                ) else {
                    continue;
                };
                total_blocks += 1;
                *kinds.entry(st.name.clone()).or_insert(0) += 1;

                // 该状态在模型里有多少个不可剔除的面
                let expected = match no_cull_cache.get(pi) {
                    Some(v) => *v,
                    None => {
                        let mut n = 0usize;
                        for rm in crate::schematic::model::resolve_models(&db, version, st) {
                            let Some(m) = db.models.get(rm.model_idx as usize) else { continue };
                            for elem in &m.elements {
                                for face_idx in 0..6u8 {
                                    if elem.face_mask & (1u8 << face_idx) == 0 {
                                        continue;
                                    }
                                    let fe = match face_idx {
                                        0 => crate::block::db::Face::Down,
                                        1 => crate::block::db::Face::Up,
                                        2 => crate::block::db::Face::North,
                                        3 => crate::block::db::Face::South,
                                        4 => crate::block::db::Face::West,
                                        _ => crate::block::db::Face::East,
                                    };
                                    if elem.face(fe).map(|mf| mf.cullface == 0xFF).unwrap_or(false)
                                    {
                                        n += 1;
                                    }
                                }
                            }
                        }
                        no_cull_cache.insert(*pi, n);
                        n
                    }
                };

                let got = per_block.get(&[x, y, z]).copied().unwrap_or(0);
                if got < expected {
                    failures.push(format!(
                        "{file_label}：{} [{}] 在 {:?} 只有 {got} 个面，\
                         但模型里有 {expected} 个不可剔除的面",
                        st.name,
                        st.prop_str(),
                        [x, y, z]
                    ));
                }
            }
        }
    }

    println!(
        "  扫描 {} 个方块实例（{} 种方块）、{total_quads} 个四边形",
        total_blocks,
        kinds.len()
    );
    assert!(total_blocks > 0, "没有扫描到任何方块");
    // 失败信息可能极多，只列前 20 条，避免刷屏掩盖真正的问题
    let shown: Vec<String> = failures.iter().take(20).cloned().collect();
    assert!(
        failures.is_empty(),
        "以下方块丢了不可剔除的面（共 {} 处，列前 20）：\n{}",
        failures.len(),
        shown.join("\n")
    );
}

/// 真实投影里不应出现「贴图解不出来」的面。
///
/// mesh::build 会把贴图为 NO_TEXTURE 的四边形**直接跳过**——因为拿不到贴图
/// 就画不出正确的颜色。这本身是合理的降级，但它的表现正是「某个方块少了
/// 一块」：几何算出来了，却因为贴图没解析到而被静默丢弃。
/// 因此这里核对真实数据里一个这样的面都没有。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_has_no_unresolved_texture_faces() {
    let files = collect_litematic(&real_dir());
    let db = env_db();

    let mut total = 0usize;
    let mut unresolved = 0usize;
    // (投影名, 方块名, 属性串) -> 次数
    let mut samples: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for f in &files {
        let label = file_name(f);
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };

        // 按坐标反查方块名，便于报告是哪个方块出的问题
        let mut name_at: std::collections::BTreeMap<[i32; 3], String> =
            std::collections::BTreeMap::new();
        for r in &sc.regions {
            for (i, pi) in r.indices.iter().enumerate() {
                let Some(st) = r.palette.get(*pi as usize) else { continue };
                let local = r.pos_from_index(i);
                let (Some(x), Some(y), Some(z)) = (
                    r.pos[0].checked_add(local[0]),
                    r.pos[1].checked_add(local[1]),
                    r.pos[2].checked_add(local[2]),
                ) else {
                    continue;
                };
                name_at.insert([x, y, z], format!("{} [{}]", st.name, st.prop_str()));
            }
        }

        for q in crate::render::visibility::collect_quads(&sc, &db, vi) {
            total += 1;
            if q.texture == crate::block::db::NO_TEXTURE {
                unresolved += 1;
                let who = name_at
                    .get(&q.block_pos)
                    .cloned()
                    .unwrap_or_else(|| "<未知方块>".to_string());
                *samples.entry(format!("{label} :: {who}")).or_insert(0) += 1;
            }
        }
    }

    println!("  四边形合计 {total}，贴图解不出的 {unresolved}");
    if !samples.is_empty() {
        let mut top: Vec<_> = samples.iter().collect();
        top.sort_by(|a, b| b.1.cmp(a.1));
        for (k, v) in top.iter().take(10) {
            println!("    {v:>5}  {k}");
        }
    }
    assert!(total > 0, "真实投影没有产出任何四边形");
    assert_eq!(
        unresolved, 0,
        "有 {unresolved} 个面因贴图解不出而被静默丢弃——表现就是「方块缺面」"
    );
}

/// 真实投影里用到的贴图确实含**半透明**像素，因此前端不得用 alphaTest 丢弃它们。
///
/// 前端材质原先设了 \lphaTest: 0.5\（THREE 会把 alpha 低于阈值的像素整片
/// 丢弃）。实测白色/黄色染色玻璃的贴图各有 191 个像素的 alpha 落在 1..127，
/// 这些像素本应作为半透明画出来，却被 alphaTest 挖成了窟窿——正是
/// 「方块缺面/有洞」的一类成因。
///
/// 这条用例量出「真实数据里确实存在半透明像素」，从而说明前端不能靠
/// alphaTest 处理透明；前端的材质已相应改为只用 transparent 混合。
/// 两边合起来才是完整证据：只改前端而没有这条数据支撑，
/// 日后有人「优化」着把 alphaTest 加回去也不会被拦住。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_textures_do_contain_partial_alpha() {
    let files = collect_litematic(&real_dir());
    let db = env_db();

    let mut used: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for f in &files {
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };
        for q in crate::render::visibility::collect_quads(&sc, &db, vi) {
            if q.texture != crate::block::db::NO_TEXTURE {
                used.insert(q.texture);
            }
        }
    }
    assert!(!used.is_empty(), "没有收集到任何使用的贴图");

    let (mut zero, mut partial, mut opaque) = (0usize, 0usize, 0usize);
    let mut examples: Vec<String> = Vec::new();
    for &pool in &used {
        let Some(tex) = db.textures.get(pool as usize) else { continue };
        let start = tex.offset as usize;
        let Some(px) = db.pixels.get(start..start + tex.len as usize) else { continue };
        let n = (tex.w as usize) * (tex.h as usize);
        let mut this_partial = 0usize;
        for i in 0..n {
            let o = i * 4;
            if o + 3 >= px.len() {
                break;
            }
            match px[o + 3] {
                0 => zero += 1,
                1..=127 => {
                    partial += 1;
                    this_partial += 1;
                }
                _ => opaque += 1,
            }
        }
        if this_partial > 0 && examples.len() < 5 {
            examples.push(format!("池下标 {pool}：{this_partial} 个半透明像素"));
        }
    }

    println!(
        "  真实投影用到 {} 张贴图：全透明 {zero}、半透明 {partial}、不透明 {opaque} 像素",
        used.len()
    );
    for e in &examples {
        println!("    {e}");
    }
    // 要守的事实是「存在半透明像素」——有了它，前端就不能用 alphaTest
    // 丢弃低 alpha 的像素。
    assert!(
        partial > 0,
        "本次样本里没有半透明像素（全透明 {zero} / 半透明 {partial} / 不透明 {opaque}）；\
         样本不足以判断前端能否使用 alphaTest，请换一批含染色玻璃的投影"
    );
}

/// 活塞头（piston_head）的全部状态组合都必须解析出模型且确实有面。
///
/// 活塞头是活塞伸出后显示的部件，模型按 `short`（杆长短）与 `type`
/// （normal / sticky）切换。这些组合在真实投影采样里未必齐全，
/// 而「缺面」最可能出现在某些组合上——一旦某组合解析失败，
/// 该方块就一个面都不画。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn piston_head_all_property_combinations_resolve() {
    let db = env_db();
    let vi = db.pick_version(3465).expect("基础库缺少 1.20.1");

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut shapes: Vec<(String, usize, usize, Vec<u32>)> = Vec::new();

    for facing in ["north", "south", "east", "west", "up", "down"] {
        for short in ["true", "false"] {
            for ty in ["normal", "sticky"] {
                let propstr = format!("facing={facing},short={short},type={ty}");
                let props: Vec<(String, String)> = propstr
                    .split(',')
                    .map(|kv| {
                        let mut it = kv.splitn(2, '=');
                        (it.next().unwrap().into(), it.next().unwrap_or("").into())
                    })
                    .collect();
                let st = BlockState { name: "minecraft:piston_head".into(), props };
                let models = crate::schematic::model::resolve_models(&db, &db.versions[vi], &st);
                checked += 1;

                if models.is_empty() {
                    failures.push(format!("piston_head [{propstr}] 解析出 0 个模型"));
                    continue;
                }
                let mut elems = 0usize;
                let mut faces = 0usize;
                for rm in &models {
                    let Some(m) = db.models.get(rm.model_idx as usize) else { continue };
                    elems += m.elements.len();
                    faces += m
                        .elements
                        .iter()
                        .map(|e| (0..6u8).filter(|b| e.face_mask & (1u8 << b) != 0).count())
                        .sum::<usize>();
                }
                if faces == 0 {
                    failures.push(format!("piston_head [{propstr}] 模型没有任何面"));
                }
                // 记录 model_idx：short/type 若真的生效，不同组合应解析到
                // 不同的模型。四种组合实测为 short=true 用 568/569、
                // short=false 用 566/567，杆长分别到 z=16 与 z=20。
                let mut idxs: Vec<u32> = Vec::new();
                for rm in &models {
                    idxs.push(rm.model_idx);
                }
                shapes.push((propstr, elems, faces, idxs));
            }
        }
    }

    println!("  piston_head 状态组合共 {} 种：", shapes.len());
    for (p, e, f, idxs) in &shapes {
        println!("    {p}  元素 {e}  面 {f}  模型 {idxs:?}");
    }
    // short 必须真的换模型：长短杆若解析到同一个模型，杆长就不会变。
    let long: std::collections::BTreeSet<Vec<u32>> = shapes
        .iter()
        .filter(|(p, _, _, _)| p.contains("short=false"))
        .map(|(_, _, _, i)| i.clone())
        .collect();
    let short: std::collections::BTreeSet<Vec<u32>> = shapes
        .iter()
        .filter(|(p, _, _, _)| p.contains("short=true"))
        .map(|(_, _, _, i)| i.clone())
        .collect();
    assert!(
        long.is_disjoint(&short),
        "short=true 与 short=false 解析到了相同的模型，说明 short 属性没被用上：\
         long={long:?} short={short:?}"
    );

    assert_eq!(checked, 6 * 2 * 2, "应覆盖全部朝向 × short × type 组合");
    assert!(failures.is_empty(), "以下活塞头状态解析异常：\n{}", failures.join("\n"));
}

/// 真实投影里同时带 x 与 y 旋转的方块，其最终朝向必须与既定约定一致。
///
/// 变体旋转在 blockstate 里写作 `"x": 90, "y": 180`。约定为**先 X 后 Y**，
/// 且各轴用 16 取补（绕方块中心 8,8,8 旋转）。若顺序写反，只有**两个都非零**
/// 的方块才会转错——单独一个轴非零时两种顺序结果常常相同，因此这类缺陷容易
/// 漏掉，表现正是「有时候会不对」。
///
/// 这里不复用 rotate_variant 的结果做参照（那等于自己和自己比），
/// 而是照约定独立算一遍再比对。
///
/// `x` 的方向：blockstate 的 `x` 是绕轴 **-x·90 度**（右手系），因此 x=90
/// 把 +Y 转到 -Z。legacy-cpp/render.cpp 的 RotateBlock 在这一点上与 Minecraft
/// 相反（把 90 度与 270 度写反），早期照抄它导致所有贴墙、贴天花板的方块被
/// 镜像——墙面拉杆的底座落到墙的另一侧、把手扎进墙里。现在以 Minecraft 的
/// 约定为准；`y` 与 legacy 一致，未改动。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_two_axis_rotation_matches_reference() {
    use crate::render::transform::rotate_variant;

    /// 按既定约定独立实现的旋转（先 X 后 Y，各轴 16 取补）
    fn reference_rotate(p: [f32; 3], rot_x_div90: u8, rot_y_div90: u8) -> [f32; 3] {
        let [mut x, mut y, mut z] = p;
        match rot_x_div90 % 4 {
            1 => {
                let (ny, nz) = (z, 16.0 - y);
                y = ny;
                z = nz;
            }
            2 => {
                y = 16.0 - y;
                z = 16.0 - z;
            }
            3 => {
                let (ny, nz) = (16.0 - z, y);
                y = ny;
                z = nz;
            }
            _ => {}
        }
        match rot_y_div90 % 4 {
            1 => {
                let (nx, nz) = (16.0 - z, x);
                x = nx;
                z = nz;
            }
            2 => {
                x = 16.0 - x;
                z = 16.0 - z;
            }
            3 => {
                let (nx, nz) = (z, 16.0 - x);
                x = nx;
                z = nz;
            }
            _ => {}
        }
        [x, y, z]
    }

    let db = env_db();
    let mut checked = 0usize;
    let mut both_axis = 0usize;
    let mut examples: Vec<String> = Vec::new();
    let mut bad: Vec<String> = Vec::new();

    // 用几个不对称的点，任何顺序/符号差异都会暴露
    let probes: [[f32; 3]; 3] = [[1.0, 2.0, 3.0], [3.0, 12.0, 7.0], [15.0, 4.0, 0.5]];

    for vi in 0..db.versions.len() {
        let version = &db.versions[vi];
        for bd in &version.blocks {
            for v in bd.variants.iter().chain(bd.multipart.iter()) {
                checked += 1;
                if v.rot_x_div90 % 4 != 0 && v.rot_y_div90 % 4 != 0 {
                    both_axis += 1;
                    if examples.len() < 6 {
                        examples.push(format!(
                            "{} [{}] x={} y={}",
                            bd.name, v.when, v.rot_x_div90, v.rot_y_div90
                        ));
                    }
                }
                for p in probes {
                    let got = rotate_variant(p, v.rot_x_div90, v.rot_y_div90);
                    let want = reference_rotate(p, v.rot_x_div90, v.rot_y_div90);
                    for a in 0..3 {
                        if (got[a] - want[a]).abs() > 1e-4 {
                            bad.push(format!(
                                "{} [{}] x={} y={}：点 {p:?} 得到 {got:?}，\
                                 按参考实现应为 {want:?}",
                                bd.name, v.when, v.rot_x_div90, v.rot_y_div90
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }

    println!("  变体共 {checked} 个，其中两轴都非零的 {both_axis} 个");
    for e in &examples {
        println!("    {e}");
    }
    assert!(checked > 0, "没有扫到任何变体");
    assert!(
        both_axis > 0,
        "这批数据里没有两轴都非零的变体（只有 {both_axis} 个），\
         无法验证旋转顺序——请换一批含此类方块的方块库"
    );
    assert!(
        bad.is_empty(),
        "以下变体的旋转与参考实现不一致：\n{}",
        bad.join("\n")
    );
}

/// 红石线的亮度必须随**实际充能等级**变化，而不是一律亮红。
///
/// 用户看到「投影里很多红石粉都是亮的」，怀疑没有按充能判定。
/// 这条走真实投影：把每个红石线方块按它的 power 属性分组，
/// 核对最终落到四边形上的染色确实随 power 单调变化。
///
/// 用真实数据而不是自造区域：既验证了属性解析，也验证了染色真的
/// 传到了四边形上（中间任何一环断掉，这里都会发现）。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn redstone_wire_brightness_follows_power() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = env_db();

    // power -> 该档在四边形上出现的染色（取红通道最大者）
    let mut by_power: BTreeMap<i32, [f32; 3]> = BTreeMap::new();
    let mut wire_quads = 0usize;

    for f in &files {
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        let Some(vi) = db.pick_version(sc.summary().data_version) else { continue };

        // 坐标 -> 该处红石线的 power
        let mut power_at: BTreeMap<[i32; 3], i32> = BTreeMap::new();
        for r in &sc.regions {
            for (i, pi) in r.indices.iter().enumerate() {
                let Some(st) = r.palette.get(*pi as usize) else { continue };
                if st.name != "minecraft:redstone_wire" {
                    continue;
                }
                let power = st
                    .props
                    .iter()
                    .find(|(k, _)| k == "power")
                    .and_then(|(_, v)| v.parse::<i32>().ok())
                    .unwrap_or(0);
                let local = r.pos_from_index(i);
                let (Some(x), Some(y), Some(z)) = (
                    r.pos[0].checked_add(local[0]),
                    r.pos[1].checked_add(local[1]),
                    r.pos[2].checked_add(local[2]),
                ) else {
                    continue;
                };
                power_at.insert([x, y, z], power);
            }
        }
        if power_at.is_empty() {
            continue;
        }

        for q in crate::render::visibility::collect_quads(&sc, &db, vi) {
            let Some(&power) = power_at.get(&q.block_pos) else { continue };
            // 只认真正染色过的面（overlay 那层 tint=-1，不参与）
            if q.tint == crate::render::tint::NO_TINT {
                continue;
            }
            wire_quads += 1;
            let e = by_power.entry(power).or_insert([0.0, 0.0, 0.0]);
            if q.tint[0] > e[0] {
                *e = q.tint;
            }
        }
    }

    println!("  红石线染色四边形 {wire_quads} 个，覆盖 {} 个充能档位", by_power.len());
    for (p, c) in &by_power {
        println!("    power={p:<2} 染色 ({:.3}, {:.3}, {:.3})", c[0], c[1], c[2]);
    }

    assert!(wire_quads > 0, "真实投影里没有采集到任何红石线四边形");
    assert!(
        by_power.len() >= 2,
        "真实投影只覆盖了 {} 个充能档位，样本不足以验证「亮度随充能变化」",
        by_power.len()
    );

    // 逐档单调递增
    let mut prev: Option<(i32, f32)> = None;
    for (p, c) in &by_power {
        if let Some((pp, pv)) = prev {
            assert!(
                c[0] > pv,
                "power={p} 的红通道 {:.3} 未比 power={pp} 的 {pv:.3} 更大，\
                 说明亮度没有随充能变化",
                c[0]
            );
        }
        prev = Some((*p, c[0]));
    }

    // 未充能必须明显暗于满充能
    let lo = by_power.values().next().unwrap()[0];
    let hi = by_power.values().next_back().unwrap()[0];
    assert!(
        hi > lo + 0.3,
        "最亮档与最暗档差别太小（{lo:.3} -> {hi:.3}），红石线看起来会「都是亮的」"
    );
    println!("  亮度随充能单调变化：{lo:.3} -> {hi:.3}");
}

/// 方块与实体的中文名必须能查到，且不能因为「本版本的语言表没中文」而整列空着。
///
/// 打包时只抓到了部分版本的官方中文（`zh=none` 的 39 个，有中文的仅 10 个），
/// 而语言表里原本**没有任何 entity 条目**。因此这条同时守住两件事：
/// 1. 本版本条目只有英文时，中文要能从其它版本补齐；
/// 2. 实体名要能查到。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn display_names_resolve_chinese_for_blocks_and_entities() {
    let db = env_db();

    // 覆盖三个典型版本：有中文的、原本没中文的、最新的
    for dv in [3465, 4325, 5023] {
        let vi = db.pick_version(dv);
        let vi = vi.expect("选版本失败");
        println!("  --- {} (dv={dv}) ---", db.versions[vi].name);

        // 方块：挑几个一定存在的
        let mut got = 0usize;
        for name in [
            "minecraft:stone",
            "minecraft:redstone_wire",
            "minecraft:oak_leaves",
            "minecraft:piston",
            "minecraft:chest",
            "minecraft:white_stained_glass",
        ] {
            let (zh, en) = crate::block::names::block_display_names(&db, Some(vi), name);
            if name == "minecraft:stone" {
                assert_eq!(zh, "石头", "石头的中文名应能查到（en={en:?}）");
            }
            if !zh.is_empty() {
                got += 1;
            }
            println!("    {name:<34} {en:<22} {zh}");
        }
        assert!(
            got >= 4,
            "{} 只查到 {got} 个方块的中文名，中文支持不完整",
            db.versions[vi].name
        );

        // 实体：中文名是本轮新补的数据
        let mut ent_got = 0usize;
        for id in [
            "minecraft:zombie",
            "minecraft:creeper",
            "minecraft:villager",
            "minecraft:item",
            "minecraft:armor_stand",
        ] {
            let (zh, en) = crate::block::names::entity_display_names(&db, Some(vi), id);
            if !zh.is_empty() {
                ent_got += 1;
            }
            println!("    实体 {id:<32} {en:<20} {zh}");
        }
        assert!(
            ent_got >= 3,
            "{} 只查到 {ent_got} 个实体的中文名；语言表里 entity.* 条目可能缺失",
            db.versions[vi].name
        );
    }
}

#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn real_project_piston_states_all_resolve() {
    use std::collections::BTreeMap;

    let files = collect_litematic(&real_dir());
    let db = env_db();
    // (方块名, 属性串) -> 出现次数
    let mut states: BTreeMap<(String, String), usize> = BTreeMap::new();

    for f in &files {
        let Ok(sc) = crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) else { continue };
        for r in &sc.regions {
            for st in &r.palette {
                let n = st.name.as_str();
                let interesting = n.contains("piston") || n.contains("redstone");
                if !interesting {
                    continue;
                }
                *states.entry((st.name.clone(), st.prop_str())).or_insert(0) += 1;
            }
        }
    }

    assert!(
        !states.is_empty(),
        "这批投影里没有活塞/红石类方块，样本不足；请换一批再跑"
    );

    // 版本按每个投影自己的 data_version 选，避免用错版本比对
    let mut piston_found = 0usize;
    let mut broken: Vec<String> = Vec::new();
    let mut facings: BTreeMap<String, usize> = BTreeMap::new();
    let mut extended: BTreeMap<String, usize> = BTreeMap::new();

    for ((name, propstr), cnt) in &states {
        let props: Vec<(String, String)> = propstr
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                (it.next().unwrap().into(), it.next().unwrap_or("").into())
            })
            .collect();

        // 找一个能查到该方块定义的版本（基础库里 1.20.1 有全部这些方块）
        let vi = db
            .pick_version(3465)
            .expect("基础库缺少 1.20.1");
        if crate::schematic::model::find_block(&db, &db.versions[vi], name).is_none() {
            broken.push(format!("{name}: 方块库里查不到定义"));
            continue;
        }
        let st = BlockState { name: name.clone(), props: props.clone() };
        let models = crate::schematic::model::resolve_models(&db, &db.versions[vi], &st);
        if models.is_empty() {
            broken.push(format!("{name} [{propstr}]（{cnt} 次）解析出 0 个模型"));
            continue;
        }
        if name.contains("piston") {
            piston_found += models.len().min(*cnt);
            for (k, v) in &props {
                if k == "facing" {
                    *facings.entry(v.clone()).or_insert(0) += 1;
                }
                if k == "extended" {
                    *extended.entry(v.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    println!("  活塞/红石状态共 {} 种，活塞解析出 {piston_found} 个模型", states.len());
    println!("  facing 覆盖：{facings:?}");
    println!("  extended 覆盖：{extended:?}");

    assert!(broken.is_empty(), "以下状态解析不出模型（会导致该方块凭空消失）：\n{}", broken.join("\n"));

    // 活塞的两个关键状态都应被真实数据覆盖到，否则这条用例等于没测到
    // 「状态切换」这件事
    assert!(
        facings.len() >= 2,
        "真实数据里只出现了 {} 种 facing，样本不足以验证朝向切换",
        facings.len()
    );
    assert!(
        extended.contains_key("true") && extended.contains_key("false"),
        "真实数据里没有同时覆盖 extended 的两种取值，无法验证状态切换：{extended:?}"
    );
}

#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_builds_a_scene() {    let files = collect_litematic(&real_dir());
    let db = env_db();
    let mut failed: Vec<String> = Vec::new();

    for f in &files {
        let name = file_name(f);
        match crate::render::scene::build_with_db(&f.to_string_lossy(), &db) {
            Ok(sc) => {
                println!(
                    "  [OK]   {name}  版本 {}  批次 {}  顶点 {}  三角 {}  未解析 {}",
                    sc.mc_version_name,
                    sc.batches.len(),
                    sc.total_vertices,
                    sc.total_triangles,
                    sc.unresolved_blocks
                );
                if sc.batches.is_empty() || sc.total_vertices == 0 {
                    failed.push(format!("{name}: 没有产出任何网格"));
                }
            }
            Err(e) => failed.push(format!("{name}: 建场景失败 {e}")),
        }
    }

    println!("\n  共 {} 个文件，未通过 {} 个", files.len(), failed.len());
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 每个真实投影：把出现最多的方块替换成石头，写出后再读回核对。
///
/// 这是**会改写用户文件**的功能，出错代价最大。因此在真实数据上验证
/// 「替换 + 写出 + 读回」这条完整链路，而不只测内存里的替换函数：
/// 序列化、位打包、调色板增长等环节的问题只在写出后才暴露。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_survives_a_block_replacement() {
    let files = collect_litematic(&real_dir());
    let db = env_db();
    let dir = work_dir("replace");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for f in &files {
        let name = file_name(f);
        let mut sc = match crate::schematic::litematic::Schematic::load(&f.to_string_lossy()) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        // 取出现最多的方块作为替换源，保证一定命中
        let before = sc.analyze();
        let Some((state_key, _)) = before.blocks_by_state.iter().max_by_key(|(_, n)| **n)
        else {
            continue; // 空投影
        };
        // 去掉属性部分，只留方块名
        let from_name = state_key.split('[').next().unwrap_or(state_key).to_string();
        if from_name.is_empty() || from_name.ends_with(":air") || from_name == "minecraft:air" {
            continue;
        }

        let vi = match db.pick_version(sc.summary().data_version) {
            Some(v) => v,
            None => {
                failed.push(format!("{name}: 方块库无可用版本"));
                continue;
            }
        };
        let info = crate::schematic::model::DbBlockInfo { db: &db, version: &db.versions[vi] };

        let volume_before: u64 = sc.regions.iter().map(|r| r.indices.len() as u64).sum();
        let non_air_before = before.non_air_total;
        let air_before = before.air_total;

        let rule = crate::schematic::replace::ReplaceRule {
            from_name: from_name.clone(),
            to_name: "minecraft:stone".to_string(),
            ..Default::default()
        };
        let out = crate::schematic::replace::replace_blocks(&mut sc, &rule, &info, 0);
        if out.replaced == 0 {
            println!("  [跳过] {name}（{from_name} → stone 无变化）");
            continue;
        }

        // 写出到临时目录再读回
        let dst = dir.join(format!("replaced_{name}"));
        let dst_s = dst.to_string_lossy().to_string();
        if let Err(e) = sc.save_to(&dst_s) {
            failed.push(format!("{name}: 写出失败 {e}"));
            continue;
        }
        let back = match crate::schematic::litematic::Schematic::load(&dst_s) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 读回失败 {e}"));
                continue;
            }
        };

        // 读回后：体积不变、空气与非空气总数不变、石头确实变多
        let volume_after: u64 = back.regions.iter().map(|r| r.indices.len() as u64).sum();
        let after = back.analyze();
        let stone_after: u64 = after
            .blocks_by_state
            .iter()
            .filter(|(k, _)| k.as_str() == "minecraft:stone" || k.starts_with("minecraft:stone["))
            .map(|(_, n)| *n)
            .sum();

        if volume_after != volume_before {
            failed.push(format!("{name}: 体积 {volume_before} → {volume_after}"));
        }
        if after.non_air_total != non_air_before || after.air_total != air_before {
            failed.push(format!(
                "{name}: 方块总数变化（非空气 {non_air_before}→{}，空气 {air_before}→{}）",
                after.non_air_total, after.air_total
            ));
        }
        if stone_after == 0 {
            failed.push(format!("{name}: 替换后没有石头，写出可能没生效"));
        }

        checked += 1;
        println!(
            "  [OK]   {name}  {from_name} → stone  替换 {} 个  调色板项 +{}  石头 {}",
            out.replaced, out.palette_changed, stone_after
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("\n  实际替换 {checked} 个，未通过 {} 个", failed.len());
    assert!(checked > 0, "没有一个投影发生了替换，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 每个真实投影：转换到另一个版本，写出后再读回核对。
///
/// 与替换同样是会改写文件的功能。转换会重建方块状态与 dataVersion，
/// 因此重点核对「体积不变、方块总数不变、能再次读回」。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_survives_a_version_conversion() {
    let files = collect_litematic(&real_dir());
    let db = env_db();
    let dir = work_dir("convert");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for f in &files {
        let name = file_name(f);
        let src = f.to_string_lossy().to_string();
        let sc = match crate::schematic::litematic::Schematic::load(&src) {
            Ok(sc) => sc,
            Err(e) => {
                failed.push(format!("{name}: 载入失败 {e}"));
                continue;
            }
        };

        let summary = sc.summary();
        let targets = crate::schematic::convert::targets(&db, summary.data_version);
        let Some(t) = targets.first() else {
            println!("  [跳过] {name}（没有可转换的目标版本）");
            continue;
        };
        let source_idx = match db.pick_version(summary.data_version) {
            Some(v) => v,
            None => {
                failed.push(format!("{name}: 方块库无可用源版本"));
                continue;
            }
        };

        // 缺替代品的项填石头，避免整项被跳过而验不到东西
        let items: Vec<crate::schematic::convert::ConvertItem> = crate::schematic::convert::analyze(&sc, &db, t.index)
            .into_iter()
            .map(|mut i| {
                if i.chosen.is_empty() && i.suggested.is_empty() {
                    i.chosen = "minecraft:stone".to_string();
                }
                i
            })
            .collect();

        let volume_before: u64 = sc.regions.iter().map(|r| r.indices.len() as u64).sum();
        let non_air_before = sc.analyze().non_air_total;
        let source_name = summary.mc_version_name.clone();

        let mut work = match crate::schematic::litematic::Schematic::load(&src) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 二次载入失败 {e}"));
                continue;
            }
        };
        let out = crate::schematic::convert::convert(&mut work, &db, source_idx, t.index, &items);

        let dst = dir.join(format!("converted_{name}"));
        let dst_s = dst.to_string_lossy().to_string();
        if let Err(e) = work.save_to(&dst_s) {
            failed.push(format!("{name}: 写出失败 {e}"));
            continue;
        }
        let back = match crate::schematic::litematic::Schematic::load(&dst_s) {
            Ok(v) => v,
            Err(e) => {
                failed.push(format!("{name}: 读回失败 {e}"));
                continue;
            }
        };

        let volume_after: u64 = back.regions.iter().map(|r| r.indices.len() as u64).sum();
        let after = back.analyze();
        if volume_after != volume_before {
            failed.push(format!("{name}: 体积 {volume_before} → {volume_after}"));
        }
        if after.non_air_total != non_air_before {
            failed.push(format!(
                "{name}: 非空气方块数 {non_air_before} → {}",
                after.non_air_total
            ));
        }
        // 转换后 dataVersion 必须已改成目标版本
        if back.summary().data_version != t.data_version {
            failed.push(format!(
                "{name}: dataVersion 仍为 {}，应为 {}",
                back.summary().data_version,
                t.data_version
            ));
        }

        checked += 1;
        println!(
            "  [OK]   {name}  {source_name} → {}  dataVersion {} → {}  改写 {} 块  非空气 {}",
            t.name,
            summary.data_version,
            t.data_version,
            out.replaced_blocks,
            after.non_air_total
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
    println!("\n  实际转换 {checked} 个，未通过 {} 个", failed.len());
    assert!(checked > 0, "没有一个投影完成转换，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 查重：副本必须被判为重复，内容不同的投影必须互不相同。
///
/// 用产品自己的 `duplicate::find`，而不是另写一份比较逻辑——
/// 否则验的是测试里的那份实现。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn duplicate_scan_recognises_copies_but_not_distinct_projects() {
    let files = collect_litematic(&real_dir());
    let dir = work_dir("dup");

    // 每个真实文件复制一份到临时目录，**原件与副本都作为候选**：
    // 只放副本的话每个副本各属不同原件，两两内容不同，自然凑不出重复组
    // （第一版就是这样，报了「重复组 0 个」，其实是测试自己写错了）。
    let mut candidates: Vec<crate::schematic::duplicate::Candidate> = Vec::new();
    let mut copies: Vec<String> = Vec::new();
    for f in &files {
        let bytes = std::fs::read(f).expect("读取真实文件失败");
        candidates.push(crate::schematic::duplicate::Candidate {
            path: f.clone(),
            size: bytes.len() as u64,
        });
        // 副本：不同目录、不同文件名，内容完全相同
        let copy = dir.join(format!("copy_{}", file_name(f)));
        std::fs::write(&copy, &bytes).expect("写入副本失败");
        copies.push(file_name(&copy));
        candidates.push(crate::schematic::duplicate::Candidate {
            path: copy.clone(),
            size: bytes.len() as u64,
        });
    }
    // 干扰项：内容与任何文件都不同、但体积恰好等于第一个文件
    if let Some(first) = files.first() {
        let n = std::fs::metadata(first).expect("取大小失败").len() as usize;
        let filler = dir.join("干扰项.litematic");
        std::fs::write(&filler, vec![0xABu8; n]).expect("写入干扰项失败");
        candidates.push(crate::schematic::duplicate::Candidate { path: filler, size: n as u64 });
    }

    let scan = crate::schematic::duplicate::find(&candidates);

    // 每个副本都应与它的原件成组，且该组恰好这两个文件
    let mut failed: Vec<String> = Vec::new();
    for copy in &copies {
        let group = scan.groups.iter().find(|g| {
            g.paths.iter().any(|p| {
                p.file_name().map(|n| n.to_string_lossy() == *copy).unwrap_or(false)
            })
        });
        match group {
            None => failed.push(format!("{copy} 未被识别为某个重复组的一员")),
            Some(g) if g.paths.len() != 2 => {
                failed.push(format!("{copy} 所在组有 {} 个文件，应为 2", g.paths.len()))
            }
            Some(_) => {}
        }
    }
    // 干扰项不应与任何真实文件同组
    let filler_grouped = scan.groups.iter().any(|g| {
        g.paths.iter().any(|p| {
            p.file_name().map(|n| n.to_string_lossy() == "干扰项.litematic").unwrap_or(false)
        })
    });
    if filler_grouped {
        failed.push("干扰项（内容不同）被判为重复".to_string());
    }

    println!(
        "  候选 {} 个，重复组 {} 个，副本 {} 个",
        candidates.len(),
        scan.groups.len(),
        copies.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failed.is_empty(), "以下判定不符预期：\n{}", failed.join("\n"));
}

/// 备份与还原：改坏文件后必须能**逐字节**还原回原样。
///
/// 这是所有破坏性操作的唯一退路（替换、版本转换、改元数据都会先备份）。
/// 备份坏了不会立刻暴露——用户要等到某次替换出错、想还原时才发现回不去，
/// 那时原件已经没了。因此在真实文件上验证「备份 → 破坏 → 还原 → 逐字节比对」。
#[test]
#[ignore = "需指定 SM_REAL_DIR 指向真实投影目录"]
fn every_real_project_backs_up_and_restores_byte_for_byte() {
    let files = collect_litematic(&real_dir());
    let exe_dir = work_dir("backup_exe");
    let work = work_dir("backup_src");
    let mut failed: Vec<String> = Vec::new();
    let mut checked = 0;

    for src in &files {
        let name = file_name(src);
        // 在实际会被改写的那个文件上操作：先拷到工作目录，避免动到原始收藏
        let target = work.join(&name);
        std::fs::copy(src, &target).expect("拷贝到工作目录失败");
        let target_s = target.to_string_lossy().to_string();
        let original_bytes = std::fs::read(&target).expect("读取失败");

        // 1) 备份
        let outcome = match crate::app::backup::create(&exe_dir, &target, "test") {
            Ok(o) => o,
            Err(e) => {
                failed.push(format!("{name}: 备份失败 {e}"));
                continue;
            }
        };
        if !outcome.backup_path.is_file() {
            failed.push(format!("{name}: 备份文件不存在"));
            continue;
        }
        if std::fs::read(&outcome.backup_path).expect("读备份失败") != original_bytes {
            failed.push(format!("{name}: 备份内容与原件不一致"));
            continue;
        }

        // 2) 让它出现在列表里（备份管理的入口就是 list）
        let listed = crate::app::backup::list(&crate::app::backup::backup_root(&exe_dir));
        if !listed.iter().any(|e| e.path == outcome.backup_path) {
            failed.push(format!("{name}: 备份未出现在 list() 里，用户看不到"));
        }

        // 3) 破坏文件：真的跑一遍替换并写回，模拟用户的正常操作
        if let Ok(mut sc) = crate::schematic::litematic::Schematic::load(&target_s) {
            let db = env_db();
            if let Some(vi) = db.pick_version(sc.summary().data_version) {
                let info = crate::schematic::model::DbBlockInfo { db: &db, version: &db.versions[vi] };
                let before = sc.analyze();
                if let Some((key, _)) = before.blocks_by_state.iter().max_by_key(|(_, n)| **n) {
                    let from = key.split('[').next().unwrap_or(key).to_string();
                    if !from.is_empty() && !from.ends_with(":air") {
                        let rule = crate::schematic::replace::ReplaceRule {
                            from_name: from,
                            to_name: "minecraft:stone".into(),
                            ..Default::default()
                        };
                        let out = crate::schematic::replace::replace_blocks(&mut sc, &rule, &info, 0);
                        if out.replaced > 0 {
                            let _ = sc.save_to(&target_s);
                        }
                    }
                }
            }
        }
        let damaged = std::fs::read(&target).expect("读改写后文件失败");
        if damaged == original_bytes {
            // 没能改成也不影响还原结论，但这样就没验到「被破坏后能还原」
            println!("  [跳过] {name}（未能造成改动，无法验证还原）");
            let _ = crate::app::backup::delete(&outcome.backup_path);
            continue;
        }

        // 4) 还原，必须逐字节回到原样
        if let Err(e) = crate::app::backup::restore(&outcome.backup_path, &target) {
            failed.push(format!("{name}: 还原失败 {e}"));
            continue;
        }
        let restored = std::fs::read(&target).expect("读还原后文件失败");
        if restored != original_bytes {
            failed.push(format!(
                "{name}: 还原后与原件不一致（{} 字节 vs {} 字节）",
                restored.len(),
                original_bytes.len()
            ));
            continue;
        }
        // 还原出来的文件必须仍是合法投影，而不是「字节对了但读不了」
        if crate::schematic::litematic::Schematic::load(&target_s).is_err() {
            failed.push(format!("{name}: 还原后无法解析"));
        }

        // 5) 删除备份
        if let Err(e) = crate::app::backup::delete(&outcome.backup_path) {
            failed.push(format!("{name}: 删除备份失败 {e}"));
        }

        checked += 1;
        println!("  [OK]   {name}  备份 → 改写 → 还原，逐字节一致");
    }

    let _ = std::fs::remove_dir_all(&exe_dir);
    let _ = std::fs::remove_dir_all(&work);
    println!("\n  实际验证 {checked} 个，未通过 {}", failed.len());
    assert!(checked > 0, "没有一个文件走完备份还原，这组用例没验到东西");
    assert!(failed.is_empty(), "以下文件未通过：\n{}", failed.join("\n"));
}

/// 诊断单个投影「分析」这条路是否通：方块表为什么会是空的。
///
/// 复刻 `commands::schematic::analyze_schematic` 的全部步骤（载入 → 选版本 → 统计），
/// 但不经过 AppHandle，因此可以在命令行里直接跑真实文件。
///
/// 输入文件优先取 `SM_ANALYZE_ONE`；没设时取 `SM_REAL_DIR` 下的第一个投影，
/// 这样它跟着 `--ignored` 那套真实投影用例一起跑也不会失败。
/// 两个都没设时直接报错——这个用例本来就是「要指定文件才有意义」的诊断，
/// 不写成「读不到就返回」：那样在报告里会显示为通过，看不出一次都没跑。
#[test]
#[ignore = "需设置 SM_ANALYZE_ONE（或 SM_REAL_DIR）指向真实投影"]
fn diagnose_analyze_one_file() {
    let p = match std::env::var("SM_ANALYZE_ONE") {
        Ok(v) => v,
        Err(_) => {
            let files = collect_litematic(&real_dir());
            files
                .first()
                .map(|f| f.to_string_lossy().into_owned())
                .expect("SM_REAL_DIR 下没有 .litematic")
        }
    };
    let sc = crate::schematic::litematic::Schematic::load(&p).expect("载入失败");
    let s = sc.summary();
    println!("文件 {p}");
    println!("  dataVersion={} mc={}", s.data_version, s.mc_version_name);
    println!("  enclosing={:?} 区域数={}", s.enclosing, sc.regions.len());

    let a = sc.analyze();
    println!("  非空气={} 空气={}", a.non_air_total, a.air_total);
    println!("  方块状态种类={}", a.blocks_by_state.len());
    println!("  实体种类={} 方块实体种类={}", a.entities_by_id.len(), a.tile_entities_by_id.len());
    let mut top: Vec<(&String, &u64)> = a.blocks_by_state.iter().collect();
    top.sort_by(|x, y| y.1.cmp(x.1));
    for (k, v) in top.iter().take(8) {
        println!("    {v:>6}  {k}");
    }

    // 库文件不再随包分发：诊断命令用 SM_BLOCK_DB 指向一份方块库
    // （通常是「版本库」生成的单版本包，或其目录之外的任意 .dat.z）
    let db_path = std::env::var("SM_BLOCK_DB")
        .expect("需设置 SM_BLOCK_DB 指向方块库文件（如 app 数据目录 base/blocks/ 下的单版本包）");
    let db = crate::block::db::BlockDb::load(&db_path, false).expect("方块库载入失败");
    let vi = db.pick_version(s.data_version);
    println!("  选中版本下标={vi:?} 名称={:?}", vi.map(|i| db.versions[i].name.clone()));

    // 把与前端拿到的完全相同的形状写出来，供无头检查用真实数据复现界面
    if let Ok(out) = std::env::var("SM_ANALYZE_OUT") {
        let blocks: Vec<serde_json::Value> = a
            .blocks_by_state
            .iter()
            .map(|(state, count)| {
                let name = state.split('[').next().unwrap_or(state).to_string();
                let (zh, en) = crate::block::names::block_display_names(&db, vi, &name);
                serde_json::json!({
                    "name": name, "state": state, "count": count,
                    "zhName": zh, "enName": en
                })
            })
            .collect();
        let entities: Vec<serde_json::Value> = a
            .entities_by_id
            .iter()
            .map(|(id, count)| {
                let (zh, en) = crate::block::names::entity_display_names(&db, vi, id);
                serde_json::json!({ "id": id, "count": count, "zhName": zh, "enName": en })
            })
            .collect();
        let payload = serde_json::json!({
            "blocks": blocks,
            "entities": entities,
            "tileEntities": [],
            "nonAirTotal": a.non_air_total,
            "airTotal": a.air_total,
            // 与命令里同口径：按**名称**去重的种类数，而不是状态数
            "distinctBlockTypes": a
                .blocks_by_state
                .keys()
                .map(|s| s.split('[').next().unwrap_or(s))
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "distinctStates": blocks.len(),
        });
        std::fs::write(&out, serde_json::to_string_pretty(&payload).unwrap())
            .expect("写分析结果失败");
        println!("  已写出 {out}");
    }

    assert!(!a.blocks_by_state.is_empty(), "该投影应当有方块");
}
