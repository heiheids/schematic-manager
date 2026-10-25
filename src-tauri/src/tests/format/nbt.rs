use super::*;

fn sample() -> Value {
    Value::Compound(vec![
        ("name".into(), Value::String("测试".into())),
        ("count".into(), Value::Int(-42)),
        ("flag".into(), Value::Byte(1)),
        ("ratio".into(), Value::Double(0.5)),
        (
            "list".into(),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)], TAG_INT),
        ),
        (
            "nested".into(),
            Value::Compound(vec![("x".into(), Value::Int(3)), ("y".into(), Value::Int(4))]),
        ),
        ("longs".into(), Value::LongArray(vec![i64::MIN, -1, 0, 1, i64::MAX])),
    ])
}

#[test]
fn modified_utf8_roundtrip_covers_specials() {
    // NUL 与补充平面字符是 modified UTF-8 与普通 UTF-8 的分歧点
    for t in ["", "abc", "中文测试", "a\u{0}b", "emoji\u{1F600}end", "\u{10FFFF}"] {
        let enc = encode_modified_utf8(t);
        assert_eq!(decode_modified_utf8(&enc).expect("应能解码"), t, "往返必须一致");
    }
    // NUL 必须写成 C0 80，而不是原始 0x00
    assert_eq!(encode_modified_utf8("\u{0}"), vec![0xC0, 0x80]);
    // 补充平面必须写成 CESU-8 代理对（6 字节）
    assert_eq!(encode_modified_utf8("\u{1F600}").len(), 6);
}

#[test]
fn modified_utf8_string_survives_nbt_roundtrip() {
    let v = Value::Compound(vec![
        ("emoji".into(), Value::String("笑脸\u{1F600}".into())),
        ("nul".into(), Value::String("a\u{0}b".into())),
    ]);
    assert_eq!(parse(&serialize(&v, "").expect("序列化失败")).unwrap(), v);
}

#[test]
fn invalid_modified_utf8_is_rejected() {
    // 孤立低代理、非法首字节、截断的多字节序列都应显式报错，
    // 而不是替换成 U+FFFD 后继续
    assert!(decode_modified_utf8(&[0xED, 0xB0, 0x80]).is_err());
    assert!(decode_modified_utf8(&[0xF8, 0x80]).is_err());
    assert!(decode_modified_utf8(&[0xE0, 0x80]).is_err());
}

#[test]
fn empty_list_keeps_declared_element_type() {
    // 「元素类型 Int、长度 0」的列表往返后必须仍是 Int。
    // 若改用 items.first() 推断元素类型，空列表会退化成 End。
    let v = Value::Compound(vec![
        ("emptyInts".into(), Value::List(Vec::new(), TAG_INT)),
        ("emptyStrs".into(), Value::List(Vec::new(), TAG_STRING)),
    ]);
    let bytes = serialize(&v, "").expect("序列化失败");
    let back = parse(&bytes).unwrap();
    assert_eq!(back, v);
    if let Value::Compound(f) = &back {
        assert_eq!(f[0].1, Value::List(Vec::new(), TAG_INT));
        assert_eq!(f[1].1, Value::List(Vec::new(), TAG_STRING));
    } else {
        panic!("根应为 Compound");
    }
}

#[test]
fn roundtrip_keeps_content() {
    let v = sample();
    let bytes = serialize(&v, "").expect("序列化失败");
    let back = parse(&bytes).unwrap();
    assert_eq!(v, back);
}

#[test]
fn field_order_is_preserved() {
    let bytes = serialize(&sample(), "").expect("序列化失败");
    let back = parse(&bytes).unwrap();
    if let Value::Compound(f) = &back {
        let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["name", "count", "flag", "ratio", "list", "nested", "longs"]);
    } else {
        panic!("根应为 Compound");
    }
}

#[test]
fn gzip_roundtrip() {
    let v = sample();
    let z = serialize_gzip(&v, "", 6).unwrap();
    assert_eq!(&z[0..2], &[0x1f, 0x8b]);
    assert_eq!(parse_maybe_gzip(&z).unwrap(), v);
}

#[test]
fn uncompressed_input_parses() {
    let v = sample();
    let raw = serialize(&v, "").expect("序列化失败");
    assert_eq!(parse_maybe_gzip(&raw).unwrap(), v);
}

#[test]
fn xyz_accessor_reads_triplet() {
    let v = Value::Compound(vec![
        ("x".into(), Value::Int(1)),
        ("y".into(), Value::Int(-2)),
        ("z".into(), Value::Int(3)),
    ]);
    assert_eq!(v.as_xyz(), Some([1, -2, 3]));
}

/// 超出 i32 范围的坐标必须判为无效，而不是截断成一个看似正常的数。
///
/// Position 在 NBT 里可能是 TAG_Long。原先用 `as i32` 截断，
/// 例如 2^32 会变成 0、2^32+5 会变成 5——后续按它算出的方块位置
/// 全是错的，且从数值上看不出异常。
///
/// 已验证有效性：把实现改回 `as i32` 后这条会失败。
#[test]
fn xyz_accessor_rejects_out_of_range_values() {
    for bad in [
        i64::from(i32::MAX) + 1,
        i64::from(i32::MIN) - 1,
        1i64 << 32,
        (1i64 << 32) + 5,
        i64::MAX,
        i64::MIN,
    ] {
        let v = Value::Compound(vec![
            ("x".into(), Value::Long(bad)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
        ]);
        assert!(
            v.as_xyz().is_none(),
            "x={bad} 超出 i32 范围，应判为无效而不是截断"
        );
    }
}

/// 边界值本身必须仍然有效，避免把范围判错。
#[test]
fn xyz_accessor_accepts_i32_boundaries() {
    for ok in [i64::from(i32::MAX), i64::from(i32::MIN), 0] {
        let v = Value::Compound(vec![
            ("x".into(), Value::Long(ok)),
            ("y".into(), Value::Int(0)),
            ("z".into(), Value::Int(0)),
        ]);
        assert_eq!(
            v.as_xyz(),
            Some([ok as i32, 0, 0]),
            "x={ok} 恰在 i32 范围内，应被接受"
        );
    }
}

/// 截断的数据必须返回错误，而不是 panic，也不能「解析成功」。
///
/// 名字承诺了 errors，但原实现是 `let _ = parse(...)`——返回值被丢弃，
/// 只验证了「不 panic」。若 parse 回归成「截断时也返回 Ok」，旧写法
/// 照样通过。这里对每个截断点断言结果是 Err。
#[test]
fn truncated_data_errors_without_panic() {
    let bytes = serialize(&sample(), "").expect("序列化失败");
    for cut in [1usize, 3, 10, 25] {
        let cut = cut.min(bytes.len());
        let r = parse(&bytes[..cut]);
        assert!(
            r.is_err(),
            "截断到 {cut} 字节时应报错，实际得到 {r:?}"
        );
    }
}

#[test]
fn set_replaces_in_place() {
    let mut v = Value::Compound(vec![
        ("a".into(), Value::Int(1)),
        ("b".into(), Value::Int(2)),
    ]);
    v.set("a", Value::Int(9));
    v.set("c", Value::Int(3));
    if let Value::Compound(f) = &v {
        let keys: Vec<&str> = f.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);
        assert_eq!(f[0].1, Value::Int(9));
    }
}

#[test]
fn zlib_roundtrip_recovers_content() {
    // 用压缩器造真实输入，避免手写字节序列带来的假设
    let src: Vec<u8> = (0..1024).map(|i| (i * 7 % 251) as u8).collect();
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(&src).unwrap();
    let comp = e.finish().unwrap();
    assert_ne!(comp, src, "压缩结果不应与原文相同");
    assert_eq!(inflate_zlib(&comp).unwrap(), src);
}

#[test]
fn zlib_rejects_gzip_input() {
    // 用 gzip 容器喂给 zlib 解压必须报错，而不是静默出错数据
    let src = b"hello".to_vec();
    let comp = gzip(&src, 6).unwrap();
    assert!(inflate_zlib(&comp).is_err(), "gzip 数据不应被 zlib 解压接受");
}

// ---- 字符串编码的对称性与长度上限 ----

/// 键名必须与字符串值走同一套编码。
///
/// 曾经键名用原始 UTF-8 写、读取端却按 modified UTF-8 解，导致键名含
/// 补充平面字符（emoji）时写得出、读不回：报「不是合法的 modified
/// UTF-8」，整个投影文件再也打不开。区域名由用户命名，是可达路径。
#[test]
fn compound_keys_roundtrip_with_supplementary_plane_characters() {
    let v = Value::Compound(vec![
        ("区域😀".to_string(), Value::Int(1)),
        ("普通".to_string(), Value::Int(2)),
        ("emoji🔧tail".to_string(), Value::Int(3)),
    ]);
    let bytes = serialize(&v, "").expect("序列化失败");
    let back = parse(&bytes).expect("键名含 emoji 时必须能读回");
    assert_eq!(back, v, "键名往返必须逐字节一致");
}

/// 键名与值里的 NUL 字符都要往返。modified UTF-8 用 0xC0 0x80 表示 NUL，
/// 与 C 风格字符串不同，这里确认两侧都按这个规则处理。
#[test]
fn strings_with_nul_character_roundtrip() {
    let v = Value::Compound(vec![("a\u{0}b".to_string(), Value::String("x\u{0}y".into()))]);
    let bytes = serialize(&v, "").expect("序列化失败");
    let back = parse(&bytes).expect("含 NUL 的字符串必须能读回");
    assert_eq!(back, v);
}

/// 超出 u16 长度上限的字符串必须报错，而不是截断长度字段。
///
/// 截断会让文件里记录的长度小于实际写入的字节数，后续字段全部错位，
/// 而写入路径的终点是覆盖用户的 .litematic —— 宁可失败也不能写出
/// 悄悄损坏的文件。
#[test]
fn oversized_string_fails_instead_of_truncating() {
    // 中文在 modified UTF-8 下每字 3 字节，30000 字即 90000 字节
    let s = "啊".repeat(30000);
    assert!(s.len() * 3 > MAX_STRING_BYTES, "用例本身必须真的超限");
    let err = serialize(&Value::String(s), "").expect_err("超长字符串应报错");
    assert!(
        err.to_string().contains("过长"),
        "错误信息应说明是长度问题，实际：{err}"
    );
}

/// 恰好等于上限时应能写出，避免把边界判错成超限。
#[test]
fn string_at_exact_limit_is_accepted() {
    // ASCII 一字节一字符，正好取到上限
    let s = "a".repeat(MAX_STRING_BYTES);
    let bytes = serialize(&Value::String(s.clone()), "").expect("恰好达上限应可写出");
    let back = parse(&bytes).expect("应能读回");
    assert_eq!(back, Value::String(s));
}

/// 超长键名同样必须报错。
#[test]
fn oversized_key_fails_instead_of_truncating() {
    let v = Value::Compound(vec![("k".repeat(MAX_STRING_BYTES + 1), Value::Byte(1))]);
    // 断言错误提到长度上限：只断言「出错了」时，任何其它原因
    // （例如序列化中途别处失败）都会让这条通过
    let e = serialize(&v, "").unwrap_err().to_string();
    assert!(
        e.contains("过长") && e.contains("上限"),
        "超长键名应报长度超限，实际：{e}"
    );
}

/// 超长根名同样必须报错。
#[test]
fn oversized_root_name_fails_instead_of_truncating() {
    let e = serialize(&Value::Byte(1), &"r".repeat(MAX_STRING_BYTES + 1))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("过长") && e.contains("上限"),
        "超长根名应报长度超限，实际：{e}"
    );
}
