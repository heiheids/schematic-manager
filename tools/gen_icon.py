"""绘制图标候选图：六个以立方体为主图案的设计方案，各画一张大图看效果。

这个脚本**不产出应用图标**，只用来比较候选设计。当前应用图标由
`tools/gen_app_icons.py` 从 `docs/` 下两张母版按尺寸分档生成（见 README 的
「应用图标」一节）。留在这里的候选里，C（选中框）是早期采用过的那一版。

为什么用脚本画而不是直接放成品 PNG：图标是纯几何图形，用脚本画才能改
（线宽、颜色、括号长度、立方体比例），也才能保证每一档尺寸都是按该尺寸
单独绘制并超采样缩小的，而不是拿一张大图硬缩——16px 若按比例给线宽会糊成
一团，脚本里对线宽设了与尺寸相关的下限。

用法：
    python tools/gen_icon.py            # 写出主图（1024）与各档预览
    python tools/gen_icon.py --all      # 另外写出 A~F 六个候选与对照图

颜色全部是自己配的纯色，不含任何游戏贴图素材。
"""
import argparse
import os

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT_DIR = os.path.join(ROOT, "tools", "icons")
SS = 8  # 超采样倍数

# ---------------------------------------------------------------- 配色

BLUE = (47, 111, 237, 255)
BLUE_HI = (82, 140, 246, 255)
WHITE = (255, 255, 255, 255)
PALE = (219, 231, 255, 255)
MID = (168, 195, 246, 255)
DARK = (30, 41, 59, 255)
DARK_HI = (45, 58, 80, 255)
CYAN = (126, 200, 255, 255)


def canvas(size):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)


def rounded_bg(d, S, base=BLUE, hi=BLUE_HI, radius_frac=0.2, top_frac=0.5):
    r = int(S * radius_frac)
    d.rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=base)
    if hi is not None:
        d.rounded_rectangle([0, 0, S - 1, int(S * top_frac)], radius=r, fill=hi)
        d.rectangle([0, int(S * (top_frac - 0.22)), S - 1, int(S * top_frac)], fill=hi)


def cube_geometry(cx, ty, w, ht):
    """等轴测立方体的三个面（顶面为 2:1 菱形）。"""
    hh = w * 0.5
    top = [(cx, ty - hh), (cx + w, ty), (cx, ty + hh), (cx - w, ty)]
    front = (cx, ty + hh)
    left = [(cx - w, ty), front, (cx, ty + hh + ht), (cx - w, ty + ht)]
    right = [front, (cx + w, ty), (cx + w, ty + ht), (cx, ty + hh + ht)]
    return top, left, right


def draw_cube(d, cx, ty, w, ht, top_c, left_c, right_c, edge=None, ew=0):
    top, left, right = cube_geometry(cx, ty, w, ht)
    d.polygon(left, fill=left_c)
    d.polygon(right, fill=right_c)
    d.polygon(top, fill=top_c)
    if edge and ew > 0:
        for poly in (top, left, right):
            d.line(list(poly) + [poly[0]], fill=edge, width=int(ew), joint="curve")


def _stamp_with_mask(img, layer, mask):
    """按掩版把图层合成到图上。

    不能用 paste(layer, (0,0), mask)：掩版为 255 处会连图层里的透明像素一起
    覆盖上去，等于把底层擦掉。正确做法是把掩版乘进图层 alpha 再 alpha 合成。
    """
    from PIL import ImageChops

    layer = layer.copy()
    layer.putalpha(ImageChops.multiply(layer.split()[3], mask))
    img.alpha_composite(layer)


def iso_grid_on_top(img, cx, ty, w, n, color, width):
    """顶面 n×n 网格：画在临时层上，再用顶面菱形做掩版合成回去。"""
    hh = w * 0.5
    size = img.size[0]
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    dl = ImageDraw.Draw(layer)
    for i in range(1, n):
        t = i / n
        x0, y0 = cx - w + 2 * w * t, ty - hh + 2 * hh * t
        dl.line([(x0, y0), (x0 - w, y0 + hh)], fill=color, width=width)
        x1, y1 = cx - w + 2 * w * t, ty + hh - 2 * hh * t
        dl.line([(x1, y1), (x1 + w, y1 + hh)], fill=color, width=width)
    mask = Image.new("L", (size, size), 0)
    top, _, _ = cube_geometry(cx, ty, w, 0)
    ImageDraw.Draw(mask).polygon(top, fill=255)
    _stamp_with_mask(img, layer, mask)


def side_grid(img, cx, ty, w, ht, n, color, width):
    """左右侧面的分割线（同样用面做掩版）。"""
    hh = w * 0.5
    size = img.size[0]
    layer = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    dl = ImageDraw.Draw(layer)
    for i in range(1, n):
        t = i / n
        lx, ly = cx - w + w * t, ty + hh * t
        dl.line([(lx, ly), (lx, ly + ht)], fill=color, width=width)
        rx, ry = cx + w - w * t, ty + hh * t
        dl.line([(rx, ry), (rx, ry + ht)], fill=color, width=width)
    mask = Image.new("L", (size, size), 0)
    _, left, right = cube_geometry(cx, ty, w, ht)
    dm = ImageDraw.Draw(mask)
    dm.polygon(left, fill=255)
    dm.polygon(right, fill=255)
    _stamp_with_mask(img, layer, mask)


# ---------------------------------------------------------------- 候选

def icon_solid(size):
    """A：蓝底 + 实心等轴测立方体"""
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S)
    draw_cube(d, S * 0.5, S * 0.40, S * 0.30, S * 0.30,
              WHITE, PALE, MID, edge=BLUE, ew=max(2, S // 90))
    return img.resize((size, size), Image.LANCZOS)


def icon_grid_top(size):
    """B：蓝底 + 立方体，顶面 3×3 网格 + 侧面分层"""
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S)
    w, ht = S * 0.30, S * 0.30
    cx, ty = S * 0.5, S * 0.40
    draw_cube(d, cx, ty, w, ht, WHITE, PALE, MID, edge=BLUE, ew=max(2, S // 90))
    iso_grid_on_top(img, cx, ty, w, 3, (47, 111, 237, 160), max(2, S // 210))
    side_grid(img, cx, ty, w, ht, 3, (47, 111, 237, 120), max(1, S // 230))
    return img.resize((size, size), Image.LANCZOS)


def icon_selection(size):
    """C：深色底 + 线稿立方体 + 四角选中括号

    这是早期用过的设计，留作候选与历史参考。当前应用图标是「双子立方体 +
    RGB 三轴」，由 `tools/gen_app_icons.py` 从 `docs/` 母版生成，与本脚本无关。

    线宽与括号长度按 size 的比例给，但设了下限：16px 那种小图标若线太细，
    缩小后括号与立方体会挤成一团，几乎认不出。
    """
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S, base=DARK, hi=DARK_HI)
    w, ht = S * 0.285, S * 0.285
    cx, ty = S * 0.5, S * 0.375
    ew = max(S * 0.0135, S / size * 1.35)
    top, left, right = cube_geometry(cx, ty, w, ht)
    for poly in (top, left, right):
        d.line(list(poly) + [poly[0]], fill=CYAN, width=int(ew), joint="curve")
    m = S * 0.105
    L = S * 0.155
    bw = max(S * 0.017, S / size * 1.6)
    for (x, y, dx, dy) in ((m, m, 1, 1), (S - m, m, -1, 1), (m, S - m, 1, -1), (S - m, S - m, -1, -1)):
        d.line([(x, y), (x + dx * L, y)], fill=WHITE, width=int(bw))
        d.line([(x, y), (x, y + dy * L)], fill=WHITE, width=int(bw))
    return img.resize((size, size), Image.LANCZOS)


def icon_layers(size):
    """D：蓝底 + 分层的立方体（切成三片、略微错开）"""
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S)
    w = S * 0.29
    ht = S * 0.115
    gap = S * 0.016
    cx = S * 0.5
    base_ty = S * 0.255
    ew = max(2, S // 110)
    tones = [
        (WHITE, (232, 240, 253, 255), (203, 219, 250, 255)),
        ((243, 247, 255, 255), (220, 232, 251, 255), (188, 208, 246, 255)),
        ((232, 240, 253, 255), (206, 221, 249, 255), (170, 196, 246, 255)),
    ]
    for i in range(3):
        ty = base_ty + i * (ht + gap)
        draw_cube(d, cx, ty, w, ht, *tones[i], edge=BLUE, ew=ew)
        if i < 2:
            y = ty + ht + gap * 0.5
            d.line([(cx - w, y), (cx, y + w * 0.5)], fill=(255, 255, 255, 150), width=max(1, S // 170))
            d.line([(cx + w, y), (cx, y + w * 0.5)], fill=(255, 255, 255, 150), width=max(1, S // 170))
    return img.resize((size, size), Image.LANCZOS)


def icon_grass(size):
    """E：深色底 + 草方块配色的立方体（绿顶、棕侧）"""
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S, base=(38, 46, 58, 255), hi=(52, 62, 76, 255))
    w, ht = S * 0.30, S * 0.30
    cx, ty = S * 0.5, S * 0.38
    top, left, right = cube_geometry(cx, ty, w, ht)
    grass, grass_d = (110, 190, 78, 255), (86, 158, 60, 255)
    dirt_r, dirt_l = (150, 104, 64, 255), (120, 80, 48, 255)
    d.polygon(left, fill=dirt_l)
    d.polygon(right, fill=dirt_r)
    d.polygon(top, fill=grass)
    hh, lip = w * 0.5, ht * 0.28
    d.polygon([(cx - w, ty), (cx, ty + hh), (cx, ty + hh + lip), (cx - w, ty + lip)], fill=grass_d)
    d.polygon([(cx, ty + hh), (cx + w, ty), (cx + w, ty + lip), (cx, ty + hh + lip)], fill=grass)
    for poly in (top, left, right):
        d.line(list(poly) + [poly[0]], fill=(24, 30, 38, 255), width=max(2, S // 100), joint="curve")
    return img.resize((size, size), Image.LANCZOS)


def icon_voxel(size):
    """F：蓝底 + 2×2×2 体素簇"""
    S = size * SS
    img, d = canvas(S)
    rounded_bg(d, S)
    w = S * 0.165
    ht = S * 0.165
    hh = w * 0.5
    cx, ty = S * 0.5, S * 0.245
    cells = [(i, j, k) for k in (0, 1) for j in (0, 1) for i in (0, 1)]
    cells.sort(key=lambda p: (p[2], p[1] + p[0]))
    ew = max(2, S // 150)
    for (i, j, k) in cells:
        x = cx + (i - j) * w
        y = ty + (i + j) * hh - k * ht
        if k:
            cols = (WHITE, (226, 236, 252, 255), (196, 214, 249, 255))
        else:
            cols = ((243, 247, 255, 255), (206, 221, 249, 255), (176, 199, 246, 255))
        draw_cube(d, x, y, w, ht, *cols, edge=BLUE, ew=ew)
    return img.resize((size, size), Image.LANCZOS)


CANDIDATES = [
    ("A", "实心立方体", icon_solid),
    ("B", "顶面网格", icon_grid_top),
    ("C", "选中框", icon_selection),
    ("D", "分层投影", icon_layers),
    ("E", "草方块配色", icon_grass),
    ("F", "体素簇", icon_voxel),
]

# 画主图时用的那一个候选。只是给这六个候选做一张 1024 主图看看效果，
# 与应用图标无关——真正进程序的是 `tools/gen_app_icons.py` 的产物。
CHOSEN = icon_selection
CHOSEN_NAME = "icon_master"


def write_master(size=1024):
    os.makedirs(OUT_DIR, exist_ok=True)
    img = CHOSEN(size)
    path = os.path.join(OUT_DIR, f"{CHOSEN_NAME}.png")
    img.save(path)
    for s in (16, 24, 32, 48, 64, 128):
        CHOSEN(s).save(os.path.join(OUT_DIR, f"preview_{s}.png"))
    print("主图：", path)
    print("各档预览：", OUT_DIR)
    print("这只是候选图。应用图标请用： python tools/gen_app_icons.py")


def write_candidates(cell=256, pad=24):
    """写出 A~F 六个候选与一张对照图（上排 256、下排 32 放大 4 倍）。"""
    os.makedirs(OUT_DIR, exist_ok=True)
    for key, name, fn in CANDIDATES:
        fn(512).save(os.path.join(OUT_DIR, f"{key}_{name}.png"))
    cols = len(CANDIDATES)
    W = cols * cell + (cols + 1) * pad
    H = pad + cell + 40 + 128 + pad * 2
    sheet = Image.new("RGBA", (W, H), (245, 246, 248, 255))
    ds = ImageDraw.Draw(sheet)
    try:
        from PIL import ImageFont
        font = ImageFont.truetype(r"C:\Windows\Fonts\arialbd.ttf", 26)
    except Exception:
        font = None
    for n, (key, name, fn) in enumerate(CANDIDATES):
        x = pad + n * (cell + pad)
        big = fn(512).resize((cell, cell), Image.LANCZOS)
        sheet.paste(big, (x, pad), big)
        ds.text((x + cell // 2, pad + cell + 14), f"{key}  {name}", fill=(30, 35, 45, 255), font=font, anchor="ma")
        small = fn(512).resize((32, 32), Image.LANCZOS).resize((128, 128), Image.NEAREST)
        sheet.paste(small, (x + cell // 2 - 64, pad + cell + 52), small)
    path = os.path.join(OUT_DIR, "candidates.png")
    sheet.save(path)
    print("候选对照图：", path)


def main():
    ap = argparse.ArgumentParser(description="绘制程序图标")
    ap.add_argument("--all", action="store_true", help="同时写出 A~F 六个候选与对照图")
    args = ap.parse_args()
    write_master()
    if args.all:
        write_candidates()


if __name__ == "__main__":
    main()
