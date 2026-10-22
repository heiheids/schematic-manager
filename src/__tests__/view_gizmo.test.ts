import { describe, expect, it } from "vitest";
import * as THREE from "three";
import {
  GIZMO_DOT_RADIUS,
  GIZMO_RADIUS,
  gizmoDots,
  hitGizmo,
} from "../render/view_gizmo";

/** 取某根轴的小球 */
function dotOf(orientation: THREE.Quaternion | null, axis: string) {
  const dots = gizmoDots(orientation, 100);
  const found = dots.find((d) => d.axis === axis);
  if (!found) throw new Error(`没有 ${axis} 这枚小球`);
  return found;
}

describe("方向标的坐标换算", () => {
  it("没有朝向时按单位基投影", () => {
    // 相机未就绪时不该抛错，也不该给出 NaN 坐标
    const dots = gizmoDots(null, 100);
    expect(dots.length, "应有六个轴向").toBe(6);
    for (const d of dots) {
      expect(Number.isFinite(d.x + d.y + d.depth), `${d.axis} 坐标应有限`).toBe(true);
    }
  });

  it("相机未旋转时 +x 在右、+y 在上、+z 朝向观察者", () => {
    // 单位四元数表示相机没有转动，此时世界轴就是相机轴
    const q = new THREE.Quaternion();
    const px = dotOf(q, "+x");
    expect(px.x, "+x 应在屏幕右侧").toBeGreaterThan(0);
    expect(px.y, "+x 纵坐标应为 0").toBeCloseTo(0, 5);

    const py = dotOf(q, "+y");
    // SVG 的 y 向下，屏幕「上方」对应负值
    expect(py.y, "+y 应在屏幕上方（y 为负）").toBeLessThan(0);
    expect(py.x, "+y 横坐标应为 0").toBeCloseTo(0, 5);

    const pz = dotOf(q, "+z");
    expect(pz.depth, "+z 应朝向观察者").toBeGreaterThan(0);
  });

  it("相机绕 Y 轴转 90° 后，+x 转到朝向观察者那一侧", () => {
    // 这是「方向标会不会跟着转」的核心：转动相机后，屏幕上的轴必须跟着换
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(0, 1, 0),
      Math.PI / 2,
    );
    const px = dotOf(q, "+x");
    expect(px.depth, "转 90° 后 +x 应朝向观察者").toBeGreaterThan(0.9);
    expect(px.x, "+x 此时横向分量应接近 0").toBeCloseTo(0, 3);
    // -z 会转到屏幕右侧
    const nz = dotOf(q, "-z");
    expect(nz.x, "-z 应转到屏幕右侧").toBeGreaterThan(90);
  });

  it("俯视 90° 时 +y 朝向观察者、+z 落在屏幕下方", () => {
    // 顶视角是常用视角，轴的位置必须符合直觉：从上往下看时，+z 指向屏幕下方
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(1, 0, 0),
      -Math.PI / 2,
    );
    const py = dotOf(q, "+y");
    expect(py.depth, "俯视时 +y 应朝向观察者").toBeGreaterThan(0.9);
    const pz = dotOf(q, "+z");
    expect(pz.y, "俯视时 +z 应落在屏幕下方（y 为正）").toBeGreaterThan(90);
  });

  it("按远近排序，越远的越靠前绘制", () => {
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(1, 0, 0),
      -Math.PI / 2,
    );
    const dots = gizmoDots(q, 100);
    for (let i = 1; i < dots.length; i++) {
      expect(
        dots[i].depth,
        `第 ${i} 个的 depth 不应小于前一个`,
      ).toBeGreaterThanOrEqual(dots[i - 1].depth);
    }
  });

  it("同一根轴的正负两端落在相反位置、深度也相反", () => {
    const q = new THREE.Quaternion().setFromAxisAngle(
      new THREE.Vector3(0, 1, 0),
      Math.PI / 4,
    );
    for (const [pos, neg] of [
      ["+x", "-x"],
      ["+y", "-y"],
      ["+z", "-z"],
    ]) {
      const a = dotOf(q, pos);
      const b = dotOf(q, neg);
      expect(a.x, `${pos} 与 ${neg} 的 x 应相反`).toBeCloseTo(-b.x, 5);
      expect(a.y, `${pos} 与 ${neg} 的 y 应相反`).toBeCloseTo(-b.y, 5);
      expect(a.depth, `${pos} 与 ${neg} 的深度应相反`).toBeCloseTo(-b.depth, 5);
    }
  });
});

describe("方向标的点击命中", () => {
  /** 等距视角下的方向标：六枚小球围在四周，中心留给复位方块 */
  const iso = (() => {
    const dir = new THREE.Vector3(-1, -1, -1).normalize();
    return new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 0, 1), dir);
  })();
  const dots = gizmoDots(iso, GIZMO_RADIUS);

  it("点在小球中心切到该轴", () => {
    const px = dots.find((d) => d.axis === "+x")!;
    const hit = hitGizmo(dots, px.x, px.y);
    expect(hit, "小球中心应命中").toEqual({ kind: "axis", key: "east" });
  });

  it("点在中心方块上触发复位", () => {
    expect(hitGizmo(dots, 0, 0), "中心应触发复位").toEqual({ kind: "center" });
  });

  it("轴向视角下正对视线的轴落在中心，点它切该轴而非复位", () => {
    // 正南视角（+Z 朝观察者）里 +Z 球的投影就在中心，与复位方块重叠。
    // 这时它压住中心是必然的：小球半径比方块大，用户看到的就是那枚球。
    const south = new THREE.Quaternion();
    const dotsSouth = gizmoDots(south, GIZMO_RADIUS);
    const pz = dotsSouth.find((d) => d.axis === "+z")!;
    expect(Math.hypot(pz.x, pz.y), "+Z 球应落在中心").toBeLessThan(1e-6);
    expect(hitGizmo(dotsSouth, pz.x, pz.y), "应切到该轴").toEqual({
      kind: "axis",
      key: "south",
    });
  });

  it("点在小球边缘之内仍命中，超出半径则不命中", () => {
    const py = dots.find((d) => d.axis === "+y")!;
    const inside = hitGizmo(dots, py.x, py.y - GIZMO_DOT_RADIUS + 1);
    expect(inside, "半径内应命中").toEqual({ kind: "axis", key: "top" });
    const outside = hitGizmo(dots, py.x, py.y - GIZMO_DOT_RADIUS - 1);
    expect(outside, "半径外不应命中").toBeNull();
  });

  it("点在空白处不命中", () => {
    // 对角线方向离六枚小球都远，那里应当什么都不触发，
    // 使拖拽按在空白处时不会误切视角
    expect(hitGizmo(dots, GIZMO_RADIUS * 0.7, GIZMO_RADIUS * 0.7)).toBeNull();
  });

  it("俯角很大时 +Y 小球压在中心上，仍应切顶视而不是复位", () => {
    // 俯角 ≥72° 时 +Y 小球会落进中心方块的范围里。若中心判定优先，
    // 用户越接近顶视越点不动顶视按钮，只能被复位
    const almostTop = new THREE.Quaternion().setFromEuler(
      new THREE.Euler(-Math.PI / 2 + 0.08, 0, 0),
    );
    const dots = gizmoDots(almostTop, GIZMO_RADIUS);
    const py = dots.find((d) => d.axis === "+y")!;
    expect(py.y, "此时 +Y 小球应落在中心附近").toBeLessThan(9);
    expect(hitGizmo(dots, py.x, py.y), "应切顶视而非复位").toEqual({
      kind: "axis",
      key: "top",
    });
  });

  it("两枚小球重叠时取更近的那一枚", () => {
    // 斜视角下前后轴在屏幕上会重在一起，此时必须认最近的一枚，
    // 否则会出现「点在 A 上却切到 B」。
    // 位置取在中心复位方块之外，免得被中心命中抢先
    const overlapping = [
      { key: "east", axis: "+x", x: 20, y: 20, depth: 0 },
      { key: "west", axis: "-x", x: 22, y: 20, depth: 0 },
    ];
    expect(hitGizmo(overlapping, 20.8, 20), "应命中更近的 +x").toEqual({
      kind: "axis",
      key: "east",
    });
    expect(hitGizmo(overlapping, 21.2, 20), "应命中更近的 -x").toEqual({
      kind: "axis",
      key: "west",
    });
  });
});
