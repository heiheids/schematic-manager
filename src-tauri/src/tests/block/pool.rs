use super::*;

#[test]
fn identical_content_is_stored_once() {
    let mut p: ContentPool<String> = ContentPool::new();
    let a = p.intern(b"same", "first".to_string());
    let b = p.intern(b"same", "second".to_string());
    assert_eq!(a, b, "内容相同应复用同一下标");
    assert_eq!(p.len(), 1);
    assert_eq!(p.get(a).unwrap(), "first", "应保留先加入的那份");
}

#[test]
fn different_content_gets_separate_slots() {
    let mut p: ContentPool<String> = ContentPool::new();
    let a = p.intern(b"one", "one".to_string());
    let b = p.intern(b"two", "two".to_string());
    assert_ne!(a, b);
    assert_eq!(p.len(), 2);
}

#[test]
fn same_length_but_different_content_is_not_merged() {
    // 关键：长度相同、仅个别字节不同的内容绝不能被当成同一份。
    // 若只比长度或指纹，这里会错误复用，进而让模型用错贴图。
    let mut p: ContentPool<u32> = ContentPool::new();
    let a = p.intern(b"abcd", 1);
    let b = p.intern(b"abce", 2);
    assert_ne!(a, b, "仅末字节不同也必须是两项");
    assert_eq!(p.len(), 2);
}

#[test]
fn intern_by_key_constructs_lazily() {
    let mut p: ContentPool<Vec<u8>> = ContentPool::new();
    let mut built = 0;
    for _ in 0..3 {
        p.intern_by_key(b"k", || {
            built += 1;
            vec![1, 2, 3]
        });
    }
    assert_eq!(built, 1, "内容相同只应构造一次");
    assert_eq!(p.len(), 1);
}

#[test]
fn indices_are_stable_across_insertions() {
    let mut p: ContentPool<u32> = ContentPool::new();
    let first = p.intern(b"a", 1);
    let _ = p.intern(b"b", 2);
    let _ = p.intern(b"c", 3);
    assert_eq!(p.intern(b"a", 99), first);
    assert_eq!(p.get(first), Some(&1));
}

#[test]
fn find_does_not_insert() {
    let mut p: ContentPool<u32> = ContentPool::new();
    p.intern(b"a", 1);
    assert_eq!(p.find(b"a"), Some(0));
    assert_eq!(p.find(b"missing"), None);
    assert_eq!(p.len(), 1, "find 不应改变池");
}

#[test]
fn empty_pool_is_empty() {
    let p: ContentPool<u32> = ContentPool::new();
    assert!(p.is_empty());
    assert_eq!(p.len(), 0);
    assert!(p.items().is_empty());
    assert!(p.get(0).is_none());
    assert_eq!(p.total_key_bytes(), 0);
}

#[test]
fn empty_key_is_still_deduplicated() {
    let mut p: ContentPool<u32> = ContentPool::new();
    let a = p.intern(b"", 1);
    let b = p.intern(b"", 2);
    assert_eq!(a, b);
    assert_eq!(p.len(), 1);
}

#[test]
fn fingerprint_is_stable_and_distinguishes_content() {
    assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
    assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
    // 与 FNV-1a 64 位的标准值一致
    assert_eq!(fingerprint(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fingerprint(b"a"), 0xaf63_dc4c_8601_ec8c);
}

#[test]
fn many_distinct_items_keep_correct_mapping() {
    let mut p: ContentPool<usize> = ContentPool::new();
    let mut expect = Vec::new();
    for i in 0..200usize {
        let key = format!("key-{i}");
        expect.push(p.intern(key.as_bytes(), i));
    }
    assert_eq!(p.len(), 200);
    for i in 0..200usize {
        let key = format!("key-{i}");
        assert_eq!(p.intern(key.as_bytes(), 9999), expect[i], "第 {i} 项未命中");
    }
    assert_eq!(p.len(), 200, "重复查询不应增长池");
}

#[test]
fn items_preserve_insertion_order() {
    let mut p: ContentPool<&str> = ContentPool::new();
    p.intern(b"1", "one");
    p.intern(b"2", "two");
    p.intern(b"3", "three");
    assert_eq!(p.items(), &["one", "two", "three"]);
}

#[test]
fn total_key_bytes_sums_contents() {
    let mut p: ContentPool<u32> = ContentPool::new();
    p.intern(b"abc", 1);
    p.intern(b"de", 2);
    assert_eq!(p.total_key_bytes(), 5);
}

// ---- 模型去重键 ----

fn face(texture: u32, cullface: u8) -> crate::block::db::ModelFace {
    crate::block::db::ModelFace {
        texture,
        uv: [0.0, 0.0, 16.0, 16.0],
        cullface,
        tintindex: -1,
        rotation: 0,
    }
}

fn model(translucent: bool, faces: Vec<crate::block::db::ModelFace>, mask: u8) -> crate::block::db::Model {
    crate::block::db::Model {
        translucent,
        elements: vec![crate::block::db::ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            has_rotation: false,
            rot_kind: 0,
            origin: [0.0; 3],
            axis: 0,
            angle: 0.0,
            rescale: false,
            rot_vec: [0.0; 3],
            face_mask: mask,
            faces,
        }],
    }
}

#[test]
fn model_key_is_equal_for_equal_models() {
    let a = model(false, vec![face(1, 0xFF)], 0b1);
    let b = model(false, vec![face(1, 0xFF)], 0b1);
    assert_eq!(model_key(&a), model_key(&b));
}

#[test]
fn model_key_distinguishes_every_rendered_field() {
    let base = model(false, vec![face(1, 0xFF)], 0b1);
    let base_key = model_key(&base);

    // 半透明标志
    let mut m = base.clone();
    m.translucent = true;
    assert_ne!(model_key(&m), base_key, "translucent 应影响去重键");

    // 贴图下标
    let mut m = base.clone();
    m.elements[0].faces[0].texture = 2;
    assert_ne!(model_key(&m), base_key, "贴图下标应影响去重键");

    // cullface
    let mut m = base.clone();
    m.elements[0].faces[0].cullface = 2;
    assert_ne!(model_key(&m), base_key, "cullface 应影响去重键");

    // UV
    let mut m = base.clone();
    m.elements[0].faces[0].uv[2] = 8.0;
    assert_ne!(model_key(&m), base_key, "UV 应影响去重键");

    // tintindex
    let mut m = base.clone();
    m.elements[0].faces[0].tintindex = 0;
    assert_ne!(model_key(&m), base_key, "tintindex 应影响去重键");

    // 面旋转
    let mut m = base.clone();
    m.elements[0].faces[0].rotation = 90;
    assert_ne!(model_key(&m), base_key, "面旋转应影响去重键");

    // faceMask
    let mut m = base.clone();
    m.elements[0].face_mask = 0b11;
    assert_ne!(model_key(&m), base_key, "faceMask 应影响去重键");

    // 几何
    let mut m = base.clone();
    m.elements[0].to = [8.0, 16.0, 16.0];
    assert_ne!(model_key(&m), base_key, "几何尺寸应影响去重键");
}

#[test]
fn model_key_distinguishes_element_rotation() {
    let mut a = model(false, vec![face(1, 0xFF)], 0b1);
    a.elements[0].has_rotation = true;
    a.elements[0].axis = 1;
    a.elements[0].angle = 45.0;
    let ka = model_key(&a);

    let mut b = a.clone();
    b.elements[0].angle = 90.0;
    assert_ne!(model_key(&b), ka, "旋转角度应影响去重键");

    let mut c = a.clone();
    c.elements[0].rescale = true;
    assert_ne!(model_key(&c), ka, "rescale 应影响去重键");

    let mut d = a.clone();
    d.elements[0].rot_kind = 1;
    d.elements[0].rot_vec = [90.0, 0.0, 0.0];
    assert_ne!(model_key(&d), ka, "多轴旋转应影响去重键");
}

#[test]
fn model_key_is_usable_as_pool_key() {
    // 两个内容相同的模型应合并，不同的应分开
    let m1 = model(false, vec![face(1, 0xFF)], 0b1);
    let m2 = model(false, vec![face(1, 0xFF)], 0b1);
    let m3 = model(false, vec![face(2, 0xFF)], 0b1);

    let mut pool: ContentPool<usize> = ContentPool::new();
    let i1 = pool.intern(&model_key(&m1), 1);
    let i2 = pool.intern(&model_key(&m2), 2);
    let i3 = pool.intern(&model_key(&m3), 3);
    assert_eq!(i1, i2, "相同模型应合并");
    assert_ne!(i1, i3, "不同模型应分开");
    assert_eq!(pool.len(), 2);
}

impl<T> ContentPool<T> {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, i: usize) -> Option<&T> {
        self.items.get(i)
    }

    /// 内容对应的下标（只查不建）
    pub fn find(&self, key: &[u8]) -> Option<usize> {
        let h = fingerprint(key);
        let cands = self.by_hash.get(&h)?;
        cands.iter().copied().find(|&i| self.keys[i] == key)
    }

    /// 池中全部内容的总字节数，供调用方判断体积
    pub fn total_key_bytes(&self) -> usize {
        self.keys.iter().map(|k| k.len()).sum()
    }
}
