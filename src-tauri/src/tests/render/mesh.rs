use super::*;

fn quad(texture: u32, shade: f32) -> Quad {
    quad_tinted(texture, shade, [1.0, 1.0, 1.0])
}

/// 带染色的四边形，用于验证「明暗 × 染色」的合成
fn quad_tinted(texture: u32, shade: f32, tint: [f32; 3]) -> Quad {
    Quad {
        corners: [
            [0.0, 0.0, 0.0],
            [16.0, 0.0, 0.0],
            [16.0, 16.0, 0.0],
            [0.0, 16.0, 0.0],
        ],
        uv: [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]],
        texture,
        shade,
        tint,
        block_pos: [0, 0, 0],
    }
}

#[test]
fn one_quad_becomes_four_vertices_and_two_triangles() {
    let m = build(&[quad(3, 1.0)]);
    assert_eq!(m.batches.len(), 1);
    let b = &m.batches[0];
    assert_eq!(b.vertex_count(), 4);
    assert_eq!(b.triangle_count(), 2);
    assert_eq!(b.positions.len(), 12);
    assert_eq!(b.uvs.len(), 8);
    // 颜色为逐顶点 3 通道
    assert_eq!(b.colors.len(), 4 * 3);
    assert_eq!(b.indices, vec![0, 2, 1, 0, 3, 2]);
    assert_eq!(m.total_vertices, 4);
    assert_eq!(m.total_triangles, 2);
}

/// 每个面的三角形绕序必须使其几何法线**朝外**。
///
/// 这条直接对应用户看到的「透视方块」：前端材质是 THREE.FrontSide，
/// 只画逆时针（法线朝观察者）的面。若绕序反了，六个面从外面看全是
/// 背面而被剔除，画面变成看得见对面墙的内侧。
///
/// 因此这里不只看索引字面量，而是真正算出法线并与「该面朝外方向」
/// 比较——索引写对但角点顺序被改动时，也能发现。
#[test]
fn face_winding_makes_normals_point_outward() {
    use crate::block::db::Face;
    use crate::render::geometry::build_face_corners;

    // 一个 16³ 的元素（模型空间 0..16），六个面各生成一个四边形
    let from = [0.0f32, 0.0, 0.0];
    let to = [16.0f32, 16.0, 16.0];
    let uv = [0.0f32, 0.0, 16.0, 16.0];
    let outward: [(Face, [f32; 3]); 6] = [
        (Face::Down, [0.0, -1.0, 0.0]),
        (Face::Up, [0.0, 1.0, 0.0]),
        (Face::North, [0.0, 0.0, -1.0]),
        (Face::South, [0.0, 0.0, 1.0]),
        (Face::West, [-1.0, 0.0, 0.0]),
        (Face::East, [1.0, 0.0, 0.0]),
    ];

    let quads: Vec<Quad> = outward
        .iter()
        .map(|(f, _)| {
            let c = build_face_corners(*f, from, to, uv);
            Quad {
                corners: c.pos,
                uv: c.uv,
                texture: 1,
                shade: 1.0,
                tint: [1.0, 1.0, 1.0],
                block_pos: [0, 0, 0],
            }
        })
        .collect();

    let m = build(&quads);
    assert_eq!(m.batches.len(), 1);
    let b = &m.batches[0];
    let v = |i: u32| -> [f32; 3] {
        let i = i as usize * 3;
        [b.positions[i], b.positions[i + 1], b.positions[i + 2]]
    };
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

    // 每个四边形两个三角形，绕序必须一致
    for (qi, (face, out)) in outward.iter().enumerate() {
        let t0 = &b.indices[qi * 6..qi * 6 + 3];
        let t1 = &b.indices[qi * 6 + 3..qi * 6 + 6];
        for t in [t0, t1] {
            let n = cross(sub(v(t[1]), v(t[0])), sub(v(t[2]), v(t[0])));
            assert!(
                dot(n, *out) > 0.0,
                "{face:?} 的三角形 {t:?} 法线朝内（应为 {out:?}），\
                 前端会用 FrontSide 把它剔除，表现为看穿方块。\
                 法线 = {n:?}"
            );
        }
    }
}

/// 绕序朝外这件事**在所有变体旋转下**都必须成立。
///
/// 上一条只覆盖未旋转的方块，而用户报告的症状恰恰是「旋转过的方块」出问题
/// （活塞 facing=up/down 等）。剔除方向会随旋转改，绕序若也跟着被镜像，
/// 就会看到「有时候缺面」——取决于方块转到哪个朝向。
///
/// 这里对全部 4×4 种 X/Y 旋转组合逐面核对：把旋转后的角点交给 mesh，
/// 算出各三角形的实际法线，再与「该面旋转后应有的朝向」比较。
/// 这条能发现「某个旋转角引入了镜像（行列式为 -1）」这类错误——
/// 那会让所有面由朝外变成朝内。
#[test]
fn face_winding_survives_every_variant_rotation() {
    use crate::block::db::Face;
    use crate::render::geometry::build_face_corners;
    use crate::render::transform::rotate_variant;

    let from = [0.0f32, 0.0, 0.0];
    let to = [16.0f32, 16.0, 16.0];
    let uv = [0.0f32, 0.0, 16.0, 16.0];
    let faces: [(Face, [f32; 3]); 6] = [
        (Face::Down, [0.0, -1.0, 0.0]),
        (Face::Up, [0.0, 1.0, 0.0]),
        (Face::North, [0.0, 0.0, -1.0]),
        (Face::South, [0.0, 0.0, 1.0]),
        (Face::West, [-1.0, 0.0, 0.0]),
        (Face::East, [1.0, 0.0, 0.0]),
    ];

    let mut checked = 0usize;
    for rx in 0..4u8 {
        for ry in 0..4u8 {
            let quads: Vec<Quad> = faces
                .iter()
                .map(|(f, _)| {
                    let c = build_face_corners(*f, from, to, uv);
                    // 与产品同样的方式旋转角点（含 0..16 取补）
                    let rotated: [[f32; 3]; 4] =
                        std::array::from_fn(|k| rotate_variant(c.pos[k], rx, ry));
                    Quad {
                        corners: rotated,
                        uv: c.uv,
                        texture: 1,
                        shade: 1.0,
                        tint: [1.0, 1.0, 1.0],
                        block_pos: [0, 0, 0],
                    }
                })
                .collect();

            let m = build(&quads);
            assert_eq!(m.batches.len(), 1, "rx={rx} ry={ry} 应只有一个批次");
            let b = &m.batches[0];
            let v = |i: u32| -> [f32; 3] {
                let i = i as usize * 3;
                [b.positions[i], b.positions[i + 1], b.positions[i + 2]]
            };
            let sub = |a: [f32; 3], c: [f32; 3]| [a[0] - c[0], a[1] - c[1], a[2] - c[2]];
            let cross = |a: [f32; 3], c: [f32; 3]| {
                [
                    a[1] * c[2] - a[2] * c[1],
                    a[2] * c[0] - a[0] * c[2],
                    a[0] * c[1] - a[1] * c[0],
                ]
            };
            let dot = |a: [f32; 3], c: [f32; 3]| a[0] * c[0] + a[1] * c[1] + a[2] * c[2];

            // 该面旋转**之后**应朝的方向：对朝外法线做同样的旋转。
            // 旋转是绕方块中心、坐标 0..16，因此法线（方向量）无需平移，
            // 直接按同一角度转动方向即可。
            let rotate_dir = |d: [f32; 3]| -> [f32; 3] {
                // 方向量在 0..16 空间下按中心 8 的补变换；等价于把
                // 向量绕中心旋转，用与角点同一套公式（先 X 再 Y）
                let [mut x, mut y, mut z] = [d[0], d[1], d[2]];
                match rx % 4 {
                    1 => {
                        let (ny, nz) = (z, -y);
                        y = ny;
                        z = nz;
                    }
                    2 => {
                        y = -y;
                        z = -z;
                    }
                    3 => {
                        let (ny, nz) = (-z, y);
                        y = ny;
                        z = nz;
                    }
                    _ => {}
                }
                match ry % 4 {
                    1 => {
                        let (nx, nz) = (-z, x);
                        x = nx;
                        z = nz;
                    }
                    2 => {
                        x = -x;
                        z = -z;
                    }
                    3 => {
                        let (nx, nz) = (z, -x);
                        x = nx;
                        z = nz;
                    }
                    _ => {}
                }
                [x, y, z]
            };

            for (qi, (_face, out)) in faces.iter().enumerate() {
                let want = rotate_dir(*out);
                for t in [
                    &b.indices[qi * 6..qi * 6 + 3],
                    &b.indices[qi * 6 + 3..qi * 6 + 6],
                ] {
                    let n = cross(sub(v(t[1]), v(t[0])), sub(v(t[2]), v(t[0])));
                    assert!(
                        dot(n, want) > 0.0,
                        "rx={rx} ry={ry} 的第 {qi} 个面法线朝内：\
                         法线 {n:?} 与应有朝向 {want:?} 点积为 {}。\
                         旋转引入了镜像，所有面会被 FrontSide 剔除",
                        dot(n, want)
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 16 * 6 * 2, "应覆盖 4×4 种旋转 × 6 个面 × 2 个三角形");
}

#[test]
fn same_texture_shares_one_batch() {
    let m = build(&[quad(7, 0.8), quad(7, 0.8), quad(7, 0.8)]);
    assert_eq!(m.batches.len(), 1, "相同贴图应合并到同一批次");
    assert_eq!(m.batches[0].vertex_count(), 12);
    assert_eq!(m.batches[0].triangle_count(), 6);
    // 第二、三个四边形的索引应各自偏移
    assert_eq!(&m.batches[0].indices[6..12], &[4, 6, 5, 4, 7, 6]);
}

#[test]
fn different_textures_split_into_batches() {
    let m = build(&[quad(5, 1.0), quad(2, 1.0), quad(9, 1.0)]);
    assert_eq!(m.batches.len(), 3);
    // 批次按下标排序，结果稳定
    let ids: Vec<u32> = m.batches.iter().map(|b| b.texture).collect();
    assert_eq!(ids, vec![2, 5, 9]);
}

#[test]
fn missing_texture_quads_are_skipped() {
    let m = build(&[quad(crate::block::db::NO_TEXTURE, 1.0), quad(4, 1.0)]);
    assert_eq!(m.batches.len(), 1, "哨兵贴图不应产生批次");
    assert_eq!(m.batches[0].texture, 4);
}

#[test]
fn color_is_per_vertex_and_combines_shade_with_tint() {
    let m = build(&[quad(1, 0.55), quad(1, 0.85)]);
    let c = &m.batches[0].colors;
    // 4 个顶点 × 3 通道
    assert_eq!(c.len(), 8 * 3);
    // 无染色时三个通道都等于明暗系数
    assert!(c[..12].iter().all(|v| (*v - 0.55).abs() < 1e-6));
    assert!(c[12..].iter().all(|v| (*v - 0.85).abs() < 1e-6));
}

/// 染色必须按通道乘进去：树叶/红石这类灰度贴图靠它上色，
/// 若只乘明暗（三通道相同），贴图就永远是灰的。
#[test]
fn tint_is_applied_per_channel() {
    let m = build(&[quad_tinted(1, 0.5, [0.25, 0.5, 1.0])]);
    let c = &m.batches[0].colors;
    assert_eq!(c.len(), 12);
    for v in c.chunks(3) {
        assert!((v[0] - 0.125).abs() < 1e-6, "红通道 {v:?}");
        assert!((v[1] - 0.25).abs() < 1e-6, "绿通道 {v:?}");
        assert!((v[2] - 0.5).abs() < 1e-6, "蓝通道 {v:?}");
    }
    // 三通道必须不同，否则染色没生效
    assert!(c[0] != c[2], "染色未按通道生效");
}

#[test]
fn empty_input_yields_empty_mesh() {
    let m = build(&[]);
    assert!(m.batches.is_empty());
    assert_eq!(m.total_vertices, 0);
    assert_eq!(m.total_triangles, 0);
}

#[test]
fn indices_stay_within_vertex_range() {
    let quads: Vec<Quad> = (0..50).map(|i| quad((i % 3) as u32, 0.7)).collect();
    let m = build(&quads);
    for b in &m.batches {
        let n = b.vertex_count() as u32;
        for idx in &b.indices {
            assert!(*idx < n, "索引 {idx} 超出顶点数 {n}");
        }
    }
}
