import { describe, expect, it } from "vitest";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import {
  adaptOrthoAspect,
  applyPreset,
  applySavedPose,
  buildSceneGroup,
  cameraUpFor,
  exportPixelSize,
  fitCamera,
  fitDistance,
  frameBox,
  defaultViewDir,
  dirFromAngles,
  anglesFromPosition,
  isBuiltinView,
  orthoVisibleExtent,
  perspectiveViewHalfHeight,
  prepareRenderCamera,
  restoreOrthoExtent,
  presetIsOrtho,
  fitOrthoHalfExtents,
  frameOrthoBox,
  orthoZoomLimits,
  setOrthoViewExtent,
  GIZMO_AXES,
  PRESET_MENU,
  presetDir,
  syncClipPlanes,
  VIEW_PRESETS,
  zoomLimits,
} from "../render/scene3d";
import type { SceneData } from "../types";

/** 一个 1×1×1 的立方体场景，六面齐全。
 *
 * 角点顺序**照抄** geometry.rs 的 build_face_corners（from=(0,0,0)、to=(1,1,1)），
 * 不能自己随手排：这套顺序是 Minecraft 模型规范定的（它同时决定 UV 方向），
 * 而绕序正确与否恰好取决于「角点顺序 × 索引规则」两者。
 * 实测手写顺序会让 Up 面的法线变成朝内，从而把下面那条绕序断言带偏。
 */
function cubeScene(): SceneData {
  const faces: Array<[number, number, number][]> = [
    [[0, 0, 1], [1, 0, 1], [1, 0, 0], [0, 0, 0]], // Down
    [[0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1]], // Up
    [[1, 1, 0], [0, 1, 0], [0, 0, 0], [1, 0, 0]], // North
    [[0, 1, 1], [1, 1, 1], [1, 0, 1], [0, 0, 1]], // South
    [[0, 1, 0], [0, 1, 1], [0, 0, 1], [0, 0, 0]], // West
    [[1, 1, 1], [1, 1, 0], [1, 0, 0], [1, 0, 1]], // East
  ];
  const positions: number[] = [];
  const uvs: number[] = [];
  const colors: number[] = [];
  const indices: number[] = [];
  for (const f of faces) {
    const base = positions.length / 3;
    f.forEach((p, k) => {
      positions.push(p[0], p[1], p[2]);
      uvs.push(k === 0 || k === 3 ? 0 : 16, k < 2 ? 0 : 16);
      colors.push(1, 1, 1);
    });
    // 与后端 mesh::build 相同的规则：0-2-1 / 0-3-2
    indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }
  return {
    batches: [
      {
        texture: 0,
        positions,
        uvs,
        colors,
        indices,
        textureWidth: 2,
        textureHeight: 2,
        textureRgba: new Array(2 * 2 * 4).fill(255),
        alphaMode: "opaque" as const,
      },
    ],
    totalVertices: positions.length / 3,
    totalTriangles: indices.length / 3,
    enclosing: [1, 1, 1],
    origin: [0, 0, 0],
    mcVersionName: "1.20.1",
    dataVersion: 3465,
    unresolvedBlocks: 0,
  };
}

/** 直接读取 BufferAttribute 的第 i 个顶点，避免依赖内部字段名 */
function vertex(geo: THREE.BufferGeometry, i: number): [number, number, number] {
  const a = geo.getAttribute("position");
  return [a.getX(i), a.getY(i), a.getZ(i)];
}

/** 按面的 0-2-1 / 0-3-2 索引取几何法线（与外向判定同源） */
function triNormal(geo: THREE.BufferGeometry, i0: number, i1: number, i2: number) {
  const p0 = new THREE.Vector3(...vertex(geo, i0));
  const p1 = new THREE.Vector3(...vertex(geo, i1));
  const p2 = new THREE.Vector3(...vertex(geo, i2));
  return new THREE.Vector3().subVectors(p1, p0).cross(new THREE.Vector3().subVectors(p2, p0));
}


/** 透明类型必须决定材质参数，否则染色玻璃会被 alphaTest 挖出窟窿。
 *
 * 三种类型的差别不是风格问题：
 * - opaque 不透明，正常写深度；
 * - cutout（树叶/草）用 alphaTest 丢掉全透明像素，其余照常写深度；
 * - translucent（染色玻璃/水）必须混合且不写深度，**不能用 alphaTest**，
 *   否则它的半透明像素会被整片丢弃。
 */
describe("按透明类型选择材质", () => {
  /** 造一个只有单批次、指定透明类型的场景 */
  function sceneWith(alphaMode: "opaque" | "cutout" | "translucent"): SceneData {
    const s = cubeScene();
    s.batches[0].alphaMode = alphaMode;
    return s;
  }

  function materialOf(alphaMode: "opaque" | "cutout" | "translucent"): THREE.MeshBasicMaterial {
    const { group, dispose } = buildSceneGroup(sceneWith(alphaMode));
    const mesh = group.children[0] as THREE.Mesh;
    const mat = mesh.material as THREE.MeshBasicMaterial;
    // 断言后再释放，避免 WebGL 资源泄漏
    const snapshot = {
      transparent: mat.transparent,
      alphaTest: mat.alphaTest,
      depthWrite: mat.depthWrite,
    };
    dispose();
    // 用副本返回，避免调用方拿着已释放的材质
    return snapshot as unknown as THREE.MeshBasicMaterial;
  }

  it("染色玻璃等半透明贴图必须用混合，不能被 alphaTest 丢弃像素", () => {
    const m = materialOf("translucent");
    expect(m.transparent, "半透明贴图必须开启混合").toBe(true);
    expect(m.alphaTest, "半透明贴图不能用 alphaTest，否则半透明像素被丢弃").toBe(0);
    expect(m.depthWrite, "半透明面不应写深度，否则会挡掉后面的几何").toBe(false);
  });

  it("树叶等镂空贴图用 alphaTest，并保留深度写入", () => {
    const m = materialOf("cutout");
    expect(m.alphaTest, "镂空贴图应丢弃全透明像素").toBe(0.5);
    expect(m.depthWrite, "镂空部分必须写深度，否则后面的方块会透出来").toBe(true);
  });

  it("不透明贴图既不用混合也不用 alphaTest", () => {
    const m = materialOf("opaque");
    expect(m.transparent).toBe(false);
    expect(m.alphaTest).toBe(0);
    expect(m.depthWrite, "不透明方块必须写深度才能正确互相遮挡").toBe(true);
  });
});

describe("scene3d 网格构建", () => {
  it("每个方块面都朝向外部", () => {
    // 这正是「透视方块」的判据：材质用 FrontSide，法线朝内的面从外面看
    // 会被剔除，表现为看穿方块。这里对六个面逐一核对法线与朝外方向同向。
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      expect(group.children.length).toBe(1);
      const mesh = group.children[0] as THREE.Mesh;
      const geo = mesh.geometry as THREE.BufferGeometry;
      const index = geo.getIndex()!;
      // 立方体中心，用于判断法线是否背离中心
      const center = new THREE.Vector3(0.5, 0.5, 0.5);

      // 六个面 × 2 个三角形 × 3 个索引 = 36 个索引
      expect(index.count, "六个面 -> 36 个索引").toBe(36);
      for (let t = 0; t < index.count; t += 3) {
        const n = triNormal(geo, index.getX(t), index.getX(t + 1), index.getX(t + 2));
        // 三角形重心
        const c = new THREE.Vector3()
          .add(new THREE.Vector3(...vertex(geo, index.getX(t))))
          .add(new THREE.Vector3(...vertex(geo, index.getX(t + 1))))
          .add(new THREE.Vector3(...vertex(geo, index.getX(t + 2))))
          .divideScalar(3);
        const away = new THREE.Vector3().subVectors(c, center);
        // 立方体是不封闭的「空心壳」，重心到中心的连线与法线应大致同向。
        // 用点积判正负即可：反向即为「朝内」，会被 FrontSide 剔除。
        expect(
          n.dot(away),
          `第 ${t / 3} 个三角形的法线 ${n.toArray()} 指向内部（重心 ${c.toArray()}）→ 会被 FrontSide 剔除`,
        ).toBeGreaterThan(0);
      }
    } finally {
      dispose();
    }
  });

  it("顶点位置减去 origin，使模型居中", () => {
    const data = cubeScene();
    // 把原点挪到 (10, 20, 30)：顶点应相应左移，范围回到 0..1
    data.origin = [10, 20, 30];
    data.batches[0].positions = data.batches[0].positions.map((v, i) =>
      i % 3 === 0 ? v + 10 : i % 3 === 1 ? v + 20 : v + 30,
    );
    const { group, dispose } = buildSceneGroup(data);
    try {
      const geo = (group.children[0] as THREE.Mesh).geometry as THREE.BufferGeometry;
      geo.computeBoundingBox();
      const box = geo.boundingBox!;
      expect(box.min.x).toBeCloseTo(0);
      expect(box.min.y).toBeCloseTo(0);
      expect(box.min.z).toBeCloseTo(0);
      expect(box.max.x).toBeCloseTo(1);
      expect(box.max.y).toBeCloseTo(1);
      expect(box.max.z).toBeCloseTo(1);
    } finally {
      dispose();
    }
  });

  it("dispose 后不留子对象，避免反复切换投影累积显存", () => {
    const built = buildSceneGroup(cubeScene());
    expect(built.group.children.length).toBe(1);
    built.dispose();
    expect(built.group.children.length, "dispose 应清空子对象").toBe(0);
  });

  it("空场景不产生网格，也不会抛错", () => {
    const data = cubeScene();
    data.batches = [];
    const built = buildSceneGroup(data);
    try {
      expect(built.group.children.length).toBe(0);
    } finally {
      built.dispose();
    }
  });
});

describe("相机取景", () => {
  it("四个预设都能把相机放在模型外侧且看得到模型", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      for (const name of Object.keys(VIEW_PRESETS)) {
        const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
        applyPreset(cam, group, name);
        // 相机应在模型之外（避免「贴到面上」看到一片同色）
        const box = new THREE.Box3().setFromObject(group);
        const center = box.getCenter(new THREE.Vector3());
        const dist = cam.position.distanceTo(center);
        const radius = box.getSize(new THREE.Vector3()).length() * 0.5;
        expect(dist, `${name}: 相机距离 ${dist} 应大于模型半径 ${radius}`).toBeGreaterThan(radius);
        // 相机朝向应指向模型中心附近，而不是背对
        const dir = cam.getWorldDirection(new THREE.Vector3());
        const toCenter = new THREE.Vector3().subVectors(center, cam.position).normalize();
        expect(dir.dot(toCenter), `${name}: 相机应朝向模型`).toBeGreaterThan(0.99);
        // 近远平面必须包住模型，否则会被裁掉
        expect(cam.near, `${name}: near 应为正`).toBeGreaterThan(0);
        expect(cam.far, `${name}: far 应大于到模型的近端距离`).toBeGreaterThan(dist - radius);
      }
    } finally {
      dispose();
    }
  });

  it("未知视角名直接报错，不静默退回某个默认角度", () => {
    // 批量导出把这些名字一路传下来：静默兜底会让用户拿到一整批
    // 角度不对的图，却以为那就是自己选的视角
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
      expect(() => applyPreset(cam, group, "custom:我的视角")).toThrow();
      expect(() => applyPreset(cam, group, "constructor")).toThrow();
      expect(() => applyPreset(cam, group, "")).toThrow();
    } finally {
      dispose();
    }
  });

  it("原型链上的键不算内置视角", () => {
    // `name in VIEW_PRESETS` 会认下 constructor / toString，
    // 再一路算出 NaN 的相机朝向
    for (const name of ["constructor", "toString", "__proto__", "hasOwnProperty"]) {
      expect(isBuiltinView(name), `${name} 不该被当成视角`).toBe(false);
      expect(presetIsOrtho(name), `${name} 不该是正射`).toBe(false);
    }
    expect(isBuiltinView("top"), "内置视角仍应认得").toBe(true);
  });

  it("自定义位姿按存下来的位置还原视角", () => {
    const cam = new THREE.PerspectiveCamera(45, 1.6, 0.1, 10000);
    const preset = {
      name: "斜视",
      ortho: false,
      position: [30, 40, 50] as [number, number, number],
      target: [0, 0, 0] as [number, number, number],
      yaw: 45,
      pitch: 30,
    };
    applySavedPose(cam, preset);
    expect([cam.position.x, cam.position.y, cam.position.z], "位置应原样还原").toEqual([
      30, 40, 50,
    ]);
    const dir = cam.getWorldDirection(new THREE.Vector3());
    const toTarget = new THREE.Vector3(-30, -40, -50).normalize();
    expect(dir.dot(toTarget), "相机应朝向存下来的注视点").toBeGreaterThan(0.999);
  });

  it("位置与注视点重合的预设明确报错", () => {
    // 这种位姿算不出朝向：静默渲染只会得到一张角度随机的图
    const cam = new THREE.PerspectiveCamera(45, 1.6, 0.1, 10000);
    expect(() =>
      applySavedPose(cam, {
        name: "坏预设",
        ortho: false,
        position: [5, 5, 5],
        target: [5, 5, 5],
        yaw: 0,
        pitch: 0,
      }),
    ).toThrow();
  });

  it("四个预设互不相同，且 front 与 back 相对", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const pos = (name: string) => {
        const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
        applyPreset(cam, group, name);
        return cam.position.clone();
      };
      const f = pos("front");
      const b = pos("back");
      // front 与 back 应在模型两侧：各自减去中心后方向相反
      const box = new THREE.Box3().setFromObject(group);
      const c = box.getCenter(new THREE.Vector3());
      const df = f.clone().sub(c).normalize();
      const db = b.clone().sub(c).normalize();
      expect(df.dot(db), "front 与 back 应在相反方向").toBeLessThan(-0.5);
      expect(pos("left45").equals(pos("right45")), "左右 45° 视角不应重合").toBe(false);
    } finally {
      dispose();
    }
  });

  it("空场景时相机落到可用位置而不是 NaN", () => {
    const empty = new THREE.Group();
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    applyPreset(cam, empty, "front");
    // 用 Number.isFinite：chai 没有 toBeFinite
    expect(Number.isFinite(cam.position.x)).toBe(true);
    expect(Number.isFinite(cam.position.y)).toBe(true);
    expect(Number.isFinite(cam.position.z)).toBe(true);
    expect(cam.near).toBeGreaterThan(0);
  });
});

describe("导出尺寸换算", () => {
  it("横构图以高为短边，长边按比例放大", () => {
    // 短边指宽高比里较小的那一边的像素数，不是把给定的数硬当成宽
    expect(exportPixelSize("4x3", 1024)).toEqual({ w: 1365, h: 1024 });
    expect(exportPixelSize("16x9", 1080)).toEqual({ w: 1920, h: 1080 });
  });

  it("竖构图以宽为短边", () => {
    expect(exportPixelSize("3x4", 1024)).toEqual({ w: 1024, h: 1365 });
    expect(exportPixelSize("9x16", 1080)).toEqual({ w: 1080, h: 1920 });
  });

  it("正方形两条边都等于给定像素数", () => {
    expect(exportPixelSize("1x1", 2048)).toEqual({ w: 2048, h: 2048 });
  });

  it("任何比例都不会算出 0 宽或 0 高", () => {
    // 零尺寸画布会让导出直接失败，极端取值下也必须守住下限
    for (const aspect of ["1x1", "4x3", "3x4", "16x9", "9x16"] as const) {
      const px = exportPixelSize(aspect, 1);
      expect(px.w, `${aspect} 的宽应至少为 1`).toBeGreaterThanOrEqual(1);
      expect(px.h, `${aspect} 的高应至少为 1`).toBeGreaterThanOrEqual(1);
    }
  });
});

/** 相机必须对准结构中心，否则取景会偏、旋转也会绕错点。 */
describe("相机取景", () => {
  /** 造一个偏移到指定位置的立方体，用来验证取景是否跟着结构走 */
  function cubeAt(x: number, y: number, z: number): THREE.Group {
    const { group } = buildSceneGroup(cubeScene());
    group.position.set(x, y, z);
    group.updateMatrixWorld(true);
    return group;
  }

  function newCamera(): THREE.PerspectiveCamera {
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    cam.updateMatrixWorld(true);
    return cam;
  }

  it("取景后相机朝向结构中心", () => {    const group = cubeAt(100, 200, 300);
    const cam = newCamera();
    fitCamera(cam, group);

    const box = new THREE.Box3().setFromObject(group);
    const center = box.getCenter(new THREE.Vector3());
    // lookAt 已把朝向设好，但方向要在 updateMatrixWorld 之后才反映到矩阵里
    cam.updateMatrixWorld(true);
    const dir = cam.getWorldDirection(new THREE.Vector3());
    const toCenter = center.clone().sub(cam.position).normalize();
    expect(
      dir.dot(toCenter),
      "相机朝向应与「相机 -> 结构中心」几乎一致；偏差大说明取景没对准结构",
    ).toBeGreaterThan(0.999);
  });

  it("结构离原点越远，相机就跟着移到越远", () => {
    // 这条防的是「只在原点取景」：那样结构放在远处时会飞出画面
    const near = cubeAt(0, 0, 0);
    const far = cubeAt(500, 500, 500);
    const camNear = newCamera();
    const camFar = newCamera();
    fitCamera(camNear, near);
    fitCamera(camFar, far);

    expect(
      camFar.position.length(),
      "结构移到 (500,500,500) 后相机也必须跟着移过去",
    ).toBeGreaterThan(camNear.position.length() + 100);
  });

  it("四个预设视角都对准结构中心", () => {
    const group = cubeAt(20, 30, 40);
    const box = new THREE.Box3().setFromObject(group);
    const center = box.getCenter(new THREE.Vector3());

    for (const name of Object.keys(VIEW_PRESETS)) {
      const cam = newCamera();
      applyPreset(cam, group, name);
      cam.updateMatrixWorld(true);
      const dir = cam.getWorldDirection(new THREE.Vector3());
      const toCenter = center.clone().sub(cam.position).normalize();
      expect(
        dir.dot(toCenter),
        `预设视角「${name}」没有对准结构中心`,
      ).toBeGreaterThan(0.999);
    }
  });

  it("空结构不会把相机弄成 NaN", () => {
    const empty = new THREE.Group();
    const cam = newCamera();
    fitCamera(cam, empty);
    // Box3 为空时若不特判，radius/dist 会变成 NaN 并把相机弄坏
    expect(Number.isFinite(cam.position.x), "相机位置不应为 NaN").toBe(true);
    expect(Number.isFinite(cam.position.y)).toBe(true);
    expect(Number.isFinite(cam.position.z)).toBe(true);
  });
});

/**
 * 取景要把结构的**投影**装进视口，而不是把它的外接球装进视口。
 *
 * 这条守的是用户实测反馈「模型只占中间一小块、四周超大空白」：
 * 细长结构的外接球直径等于对角线，按它取景会让实际投影只占画面一小部分。
 */
describe("取景按投影包围盒拟合", () => {
  function newCamera(aspect = 1): THREE.PerspectiveCamera {
    const cam = new THREE.PerspectiveCamera(45, aspect, 0.1, 10000);
    cam.updateMatrixWorld(true);
    return cam;
  }

  /**
   * 求结构投影到屏幕上占视口的比例。
   *
   * 判据用「八个角点投影后的横向/纵向跨度 ÷ 视口尺寸」，取较大者——
   * 这正是「模型在画面里占多大」的定义，与像素无关，因此不依赖渲染。
   */
  function fillRatio(cam: THREE.PerspectiveCamera, box: THREE.Box3): number {
    cam.updateMatrixWorld(true);
    cam.updateProjectionMatrix();
    const tanY = Math.tan((cam.fov * Math.PI) / 360);
    const tanX = tanY * cam.aspect;
    const center = box.getCenter(new THREE.Vector3());
    const forward = cam.getWorldDirection(new THREE.Vector3());
    const right = new THREE.Vector3().crossVectors(forward, cam.up).normalize();
    const up = new THREE.Vector3().crossVectors(right, forward).normalize();
    const v = new THREE.Vector3();
    let maxX = 0;
    let maxY = 0;
    for (let i = 0; i < 8; i++) {
      v.set(
        i & 1 ? box.max.x : box.min.x,
        i & 2 ? box.max.y : box.min.y,
        i & 4 ? box.max.z : box.min.z,
      );
      const rel = v.clone().sub(cam.position);
      const depth = rel.dot(forward);
      if (depth <= 0) return Number.POSITIVE_INFINITY;
      maxX = Math.max(maxX, Math.abs(rel.dot(right)) / (depth * tanX));
      maxY = Math.max(maxY, Math.abs(rel.dot(up)) / (depth * tanY));
    }
    return Math.max(maxX, maxY);
  }

  it("细长结构的投影要占满大部分视口", () => {
    // 用户那个 53×9×9 的机器就是这种细长形状
    const box = new THREE.Box3(
      new THREE.Vector3(-26.5, -4.5, -4.5),
      new THREE.Vector3(26.5, 4.5, 4.5),
    );
    const cam = newCamera();
    frameBox(cam, box, defaultViewDir());
    const fill = fillRatio(cam, box);
    // 曾经按外接球取景时这里只有 0.45 左右
    expect(fill, `投影只占视口的 ${(fill * 100).toFixed(1)}%，空白过多`).toBeGreaterThan(0.8);
    expect(fill, "不该顶出视口").toBeLessThanOrEqual(1.0);
  });

  it("正方体结构也要填满，且不因形状而改变判据", () => {
    const box = new THREE.Box3(
      new THREE.Vector3(-8, -8, -8),
      new THREE.Vector3(8, 8, 8),
    );
    const cam = newCamera();
    frameBox(cam, box, defaultViewDir());
    const fill = fillRatio(cam, box);
    expect(fill).toBeGreaterThan(0.8);
    expect(fill).toBeLessThanOrEqual(1.0);
  });

  it("四个预设视角都装得下结构", () => {
    const box = new THREE.Box3(
      new THREE.Vector3(-26.5, 0, -4.5),
      new THREE.Vector3(26.5, 9, 4.5),
    );
    for (const name of Object.keys(VIEW_PRESETS)) {
      const cam = newCamera();
      frameBox(cam, box, presetDir(name));
      const fill = fillRatio(cam, box);
      expect(fill, `预设「${name}」把结构画出了视口（${fill.toFixed(2)}）`).toBeLessThanOrEqual(1.0);
      expect(fill, `预设「${name}」留白过多（${fill.toFixed(2)}）`).toBeGreaterThan(0.7);
    }
  });

  it("宽视口下按横向收紧，不会上下撑出画面", () => {
    const box = new THREE.Box3(
      new THREE.Vector3(-26.5, -4.5, -4.5),
      new THREE.Vector3(26.5, 4.5, 4.5),
    );
    for (const aspect of [0.6, 1, 2.4]) {
      const cam = newCamera(aspect);
      frameBox(cam, box, defaultViewDir());
      const fill = fillRatio(cam, box);
      expect(fill, `宽高比 ${aspect} 下越界（${fill.toFixed(2)}）`).toBeLessThanOrEqual(1.0);
      expect(fill, `宽高比 ${aspect} 下留白过多（${fill.toFixed(2)}）`).toBeGreaterThan(0.7);
    }
  });

  it("退化包围盒（单个方块）也会给非零距离", () => {
    const p = new THREE.Vector3(3, 4, 5);
    const box = new THREE.Box3(p.clone(), p.clone());
    expect(fitDistance(box, defaultViewDir(), 45, 1)).toBeGreaterThan(0);
  });

  /**
   * 默认取景的相机必须落在模型的 **+x +y +z** 一侧。
   *
   * 这一条与构图无关，是「画得出来 / 画不出来」的分界：方向写反时相机绕到
   * 另一侧，正面的三角形按背面剔除全部丢掉，界面上是一片空白，
   * 而「投影占屏幕多大」之类的几何断言照样全部通过——
   * 因为轮廓是同一块。因此只能直接验相机在哪个象限。
   */
  it("默认取景时相机在模型的 +x +y +z 一侧", () => {
    const box = new THREE.Box3(
      new THREE.Vector3(-1, -1, -1),
      new THREE.Vector3(1, 1, 1),
    );
    const cam = newCamera();
    frameBox(cam, box, defaultViewDir());
    expect(cam.position.x, "相机应在 +x 一侧").toBeGreaterThan(1);
    expect(cam.position.y, "相机应在 +y 一侧").toBeGreaterThan(1);
    expect(cam.position.z, "相机应在 +z 一侧").toBeGreaterThan(1);
  });

  /**
   * 预设视角的相机方位必须与角度表一致。
   *
   * 期望值按 VIEW_PRESETS 的角度定义**独立算出**，不调用 presetDir，
   * 否则就是拿实现验证实现。方向写反时相机绕到另一侧，正面三角形被背面
   * 剔除，界面一片空白——而「占屏幕多大」这类几何断言照样通过。
   */
  it("预设视角的相机方位与角度定义一致", () => {
    const box = new THREE.Box3(
      new THREE.Vector3(-1, -1, -1),
      new THREE.Vector3(1, 1, 1),
    );
    /** 由角度表求「相机相对中心」的单位方向 */
    function offsetDir(yawDeg: number, pitchDeg: number): THREE.Vector3 {
      const yaw = (yawDeg * Math.PI) / 180;
      const pitch = (pitchDeg * Math.PI) / 180;
      return new THREE.Vector3(
        Math.cos(pitch) * Math.sin(yaw),
        -Math.sin(pitch),
        Math.cos(pitch) * Math.cos(yaw),
      ).normalize().negate();
    }
    function offsetOf(name: string): THREE.Vector3 {
      const cam = newCamera();
      frameBox(cam, box, presetDir(name));
      return cam.position.clone().normalize();
    }

    const front = VIEW_PRESETS.front;
    expect(offsetOf("front").dot(offsetDir(front.yaw, front.pitch)), "front 方位不符").toBeGreaterThan(0.999);
    const back = VIEW_PRESETS.back;
    expect(offsetOf("back").dot(offsetDir(back.yaw, back.pitch)), "back 方位不符").toBeGreaterThan(0.999);
    const l45 = VIEW_PRESETS.left45;
    expect(offsetOf("left45").dot(offsetDir(l45.yaw, l45.pitch)), "left45 方位不符").toBeGreaterThan(0.999);
    const r45 = VIEW_PRESETS.right45;
    expect(offsetOf("right45").dot(offsetDir(r45.yaw, r45.pitch)), "right45 方位不符").toBeGreaterThan(0.999);

    // 再钉一层语义：front 与 back 在 z 上相反，left45 与 right45 在 x 上相反。
    // 这两条即使角度表的数值被改也仍然成立，能挡住「整体取反」这类错误。
    expect(offsetOf("front").z).toBeLessThan(0);
    expect(offsetOf("back").z).toBeGreaterThan(0);
    expect(offsetOf("left45").x).toBeGreaterThan(0);
    expect(offsetOf("right45").x).toBeLessThan(0);
  });
});

/** 滚轮缩放：用 OrbitControls 自带的那一套，但要能夹在限位内、并有正确的裁剪面。 */
describe("滚轮缩放", () => {
  /** OrbitControls 需要一个可量的 DOM 元素来算缩放比例 */
  function fakeElement(w: number, h: number): HTMLElement {
    const el = document.createElement("div");
    Object.defineProperty(el, "clientWidth", { value: w });
    Object.defineProperty(el, "clientHeight", { value: h });
    el.getBoundingClientRect = () =>
      ({
        left: 0,
        top: 0,
        width: w,
        height: h,
        right: w,
        bottom: h,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      }) as DOMRect;
    return el;
  }

  function setup() {
    const el = fakeElement(600, 600);
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    cam.position.set(30, 30, 30);
    const controls = new OrbitControls(cam, el);
    controls.enableZoom = true;
    controls.target.set(0, 0, 0);
    controls.update();
    return { el, cam, controls };
  }

  function wheel(el: HTMLElement, deltaY: number) {
    el.dispatchEvent(
      new WheelEvent("wheel", { deltaY, deltaMode: 0, bubbles: true, cancelable: true }),
    );
  }

  /**
   * 滚轮缩放本身要用 OrbitControls 的实现，这条钉住它确实生效。
   *
   * 之所以要专门测：它依赖内部状态机（`state` 必须为 NONE），一旦哪天
   * 被改成手写缩放或改了 enableZoom，这里会立刻失败。
   */
  it("滚轮事件能改变相机到轴心的距离", () => {
    const { el, cam, controls } = setup();
    const before = cam.position.distanceTo(controls.target);
    wheel(el, -240);
    controls.update();
    const closer = cam.position.distanceTo(controls.target);
    expect(closer, "向上滚应离模型更近").toBeLessThan(before);

    wheel(el, 600);
    controls.update();
    expect(cam.position.distanceTo(controls.target)).toBeGreaterThan(closer);
  });

  it("缩放只改距离、不改视线方向", () => {
    const { el, cam, controls } = setup();
    const dirBefore = controls.target.clone().sub(cam.position).normalize();
    wheel(el, -240);
    controls.update();
    const dirAfter = controls.target.clone().sub(cam.position).normalize();
    expect(dirAfter.dot(dirBefore), "缩放不该把视角转走").toBeGreaterThan(0.9999);
  });

  it("使劲滚动也夹在限位内，不会穿过模型", () => {
    const { el, cam, controls } = setup();
    const limits = zoomLimits(cam.position.distanceTo(controls.target));
    controls.minDistance = limits.min;
    controls.maxDistance = limits.max;
    for (let i = 0; i < 60; i++) {
      wheel(el, -400);
      controls.update();
    }
    expect(cam.position.distanceTo(controls.target)).toBeCloseTo(limits.min, 4);
    for (let i = 0; i < 60; i++) {
      wheel(el, 400);
      controls.update();
    }
    expect(cam.position.distanceTo(controls.target)).toBeCloseTo(limits.max, 4);
  });

  it("限位随模型大小变，不会大小模型一套值", () => {
    const small = zoomLimits(10);
    const big = zoomLimits(200);
    expect(big.min).toBeGreaterThan(small.min);
    expect(big.max).toBeGreaterThan(small.max);
    // 任何情况下下限都必须为正：0 会让相机贴到轴心上
    expect(zoomLimits(0).min).toBeGreaterThan(0);
  });

  /** 正交相机下的滚轮缩放：改的是 camera.zoom，不是到轴心的距离。
   *
   * 这条专门钉住正交缩放真的生效。实测在真实 WebView2 里它一度完全不起作用
   * （滚轮事件到了 canvas、OrbitControls 也 preventDefault 了，但 zoom 一动
   * 不动），而透视侧同时是好的——两套限位分属不同字段，混了就会一边失效。
   */
  it("正交相机下滚轮能改变 camera.zoom", () => {
    const el = document.createElement("div");
    const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 1000);
    cam.position.set(0, 0, 50);
    cam.zoom = 1;
    const controls = new OrbitControls(cam, el);
    controls.enableZoom = true;
    const { min, max } = orthoZoomLimits();
    controls.minZoom = min;
    controls.maxZoom = max;
    controls.target.set(0, 0, 0);
    controls.update();

    wheel(el, -240);
    controls.update();
    expect(cam.zoom, "向上滚应放大（zoom 变大）").toBeGreaterThan(1);

    const zoomed = cam.zoom;
    wheel(el, 600);
    controls.update();
    expect(cam.zoom, "向下滚应缩小").toBeLessThan(zoomed);
  });

  it("正交下使劲滚动也夹在 zoom 限位内", () => {
    const el = document.createElement("div");
    const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 1000);
    cam.position.set(0, 0, 50);
    cam.zoom = 1;
    const controls = new OrbitControls(cam, el);
    controls.enableZoom = true;
    const { min, max } = orthoZoomLimits();
    controls.minZoom = min;
    controls.maxZoom = max;
    controls.target.set(0, 0, 0);
    controls.update();
    for (let i = 0; i < 80; i++) {
      wheel(el, -400);
      controls.update();
    }
    expect(cam.zoom, "放大到顶应停在 maxZoom").toBeCloseTo(max, 4);
    for (let i = 0; i < 120; i++) {
      wheel(el, 400);
      controls.update();
    }
    expect(cam.zoom, "缩小到顶应停在 minZoom").toBeCloseTo(min, 4);
  });

  /**
   * 裁剪面必须跟着距离走。
   *
   * OrbitControls 只管位置、不管 near/far：取景后 near 是 dist/100，
   * 一直拉近就会穿进近裁剪面、模型被整片切掉（表现为「缩放几下画面就空了」）。
   */
  it("拉近后近裁剪面跟着变小，模型不会被切掉", () => {
    const { cam, controls } = setup();
    const target = controls.target;
    syncClipPlanes(cam, target);
    const nearFarAtFit = cam.near;

    // 把相机挪到离模型很近的位置（相当于滚轮拉到下限）
    cam.position.copy(target).add(new THREE.Vector3(0.5, 0.5, 0.5));
    expect(syncClipPlanes(cam, target), "距离变了应报告需要重绘").toBe(true);
    expect(cam.near, "拉近后近裁剪面必须变小，否则模型会被切掉").toBeLessThan(nearFarAtFit);
    // 模型的最近点大约在 0.7 格处，近裁剪面必须比它更小
    expect(cam.near).toBeLessThan(0.5);
    // 稳定后再调不应反复报告变化（否则每帧都会重绘）
    expect(syncClipPlanes(cam, target)).toBe(false);
  });

  it("相机贴在轴心上时不产生 NaN", () => {
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    const target = new THREE.Vector3(1, 2, 3);
    cam.position.copy(target);
    expect(syncClipPlanes(cam, target)).toBe(false);
    expect(Number.isFinite(cam.near)).toBe(true);
  });
});


describe("视角预设与方向换算", () => {
  it("六个轴向指向各自的坐标轴正负方向", () => {
    // axialDir 断言 presetDir 结果落在某个坐标轴上：
    // 轴向视角是精确核对用的，不能有一点偏，因此非零的那个分量应为 ±1
    function expectAxis(name: string, axis: "x" | "y" | "z", sign: 1 | -1) {
      const d = presetDir(name);
      const comp = { x: d.x, y: d.y, z: d.z };
      expect(comp[axis] * sign, `${name} 的 ${axis} 分量`).toBeCloseTo(1, 5);
      for (const other of ["x", "y", "z"] as const) {
        if (other === axis) continue;
        expect(comp[other], `${name} 的 ${other} 分量应为 0`).toBeCloseTo(0, 5);
      }
    }
    // 顶视：相机在上方朝下看，「相机 → 目标」的 y 为负
    expectAxis("top", "y", -1);
    expectAxis("bottom", "y", 1);
    expectAxis("south", "z", 1);
    expectAxis("north", "z", -1);
    expectAxis("east", "x", 1);
    expectAxis("west", "x", -1);
  });

  it("所有预设都给单位方向向量，没有一个退化成零向量", () => {
    // 零向量会让 frameBox 里的归一化得到 NaN，相机直接失效
    for (const name of Object.keys(VIEW_PRESETS)) {
      const d = presetDir(name);
      expect(d.length(), `${name} 应是单位向量`).toBeCloseTo(1, 5);
      expect(Number.isFinite(d.x + d.y + d.z), `${name} 不应含 NaN`).toBe(true);
    }
  });

  it("角度与方向可以互相换算", () => {
    // 保存自定义预设要用 anglesFromPosition 从相机位置反推角度，
    // 存进去后再用 dirFromAngles 还原，两者必须一致
    for (const [yaw, pitch] of [
      [0, 12],
      [45, 30],
      [-45, 30],
      [90, 0],
      [180, -20],
      [0, 89],
    ]) {
      // dirFromAngles 给「相机 → 目标」，相机因此站在它的反方向
      const cameraPos = dirFromAngles(yaw, pitch).negate().multiplyScalar(10);
      const back = anglesFromPosition(cameraPos, new THREE.Vector3(0, 0, 0));
      expect(back.pitch, `pitch 应还原（${yaw},${pitch}）`).toBeCloseTo(pitch, 3);
      // pitch 到 ±90 时水平分量消失，yaw 无意义
      if (Math.abs(pitch) < 89) {
        const yawDiff = ((back.yaw - yaw + 540) % 360) - 180;
        expect(yawDiff, `yaw 应还原（${yaw},${pitch}）`).toBeCloseTo(0, 3);
      }
    }
  });

  it("真等距预设的三个视线分量等幅", () => {
    for (const name of ["isoLeft", "isoRight"]) {
      const dir = presetDir(name);
      expect(Math.abs(dir.x), `${name} 的 x/y 分量应等幅`).toBeCloseTo(Math.abs(dir.y), 12);
      expect(Math.abs(dir.z), `${name} 的 z/y 分量应等幅`).toBeCloseTo(Math.abs(dir.y), 12);
    }
  });

  it("只有真等距预设标记为正射", () => {
    expect(presetIsOrtho("isoRight"), "真等距右应是正射").toBe(true);
    expect(presetIsOrtho("isoLeft"), "真等距左应是正射").toBe(true);
    expect(presetIsOrtho("right45"), "普通等距应是透视").toBe(false);
    expect(presetIsOrtho("top"), "轴向视角默认透视").toBe(false);
  });

  it("右键菜单列全十个预设，且方向标覆盖六个轴向", () => {
    expect(PRESET_MENU.length, "菜单应有十个预设").toBe(10);
    for (const item of PRESET_MENU) {
      expect(VIEW_PRESETS[item.key], `${item.key} 应存在于预设表`).toBeTruthy();
    }
    expect(GIZMO_AXES.length, "方向标应有六个轴向").toBe(6);
    expect(GIZMO_AXES.map((a) => a.axis).sort()).toEqual([
      "+x",
      "+y",
      "+z",
      "-x",
      "-y",
      "-z",
    ]);
  });
});
describe("正交相机的取景", () => {
  /** 造一个给定尺寸的包围盒，中心在原点 */
  function boxOf(w: number, h: number, d: number): THREE.Box3 {
    return new THREE.Box3(
      new THREE.Vector3(-w / 2, -h / 2, -d / 2),
      new THREE.Vector3(w / 2, h / 2, d / 2),
    );
  }

  /** 把包围盒八个角点投到相机空间，返回三个归一化坐标的最大绝对值 */
  function projectedExtent(cam: THREE.OrthographicCamera, box: THREE.Box3) {
    const v = new THREE.Vector3();
    let maxX = 0;
    let maxY = 0;
    let maxZ = 0;
    for (let i = 0; i < 8; i++) {
      v.set(
        i & 1 ? box.max.x : box.min.x,
        i & 2 ? box.max.y : box.min.y,
        i & 4 ? box.max.z : box.min.z,
      );
      v.project(cam);
      maxX = Math.max(maxX, Math.abs(v.x));
      maxY = Math.max(maxY, Math.abs(v.y));
      maxZ = Math.max(maxZ, Math.abs(v.z));
    }
    return { maxX, maxY, maxZ };
  }

  it("细长模型按投影后尺寸取景，不按包围盒最长边", () => {
    // 53×9×9 是实测里「画面空掉一大半」的那个形状：按最长边算会剩大片空白
    const box = boxOf(53, 9, 9);
    const dir = new THREE.Vector3(-1, -1, -1).normalize();
    const probe = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
    probe.position.set(0, 0, 0);
    probe.lookAt(dir);
    const { halfW, halfH } = fitOrthoHalfExtents(probe, box, 1);
    // 斜着看对角线会参与投影，因此可视范围必须能装下 53 那条边的一部分
    expect(halfW, "水平可视范围应覆盖投影宽度").toBeGreaterThan(53 * 0.25);
    expect(halfH, "垂直可视范围应为正").toBeGreaterThan(0);
    // 不该大到把包围盒最长边整个当半径：那会留出成片空白
    expect(halfW, "不应按最长边的一半以上给范围").toBeLessThan(53);
  });

  it("取景后整个模型都落在视锥内", () => {
    // 这是取景的唯一实质要求：所有角点投影后 |x|、|y| 都不超过 1
    for (const box of [boxOf(10, 10, 10), boxOf(53, 9, 9), boxOf(3, 40, 3)]) {
      for (const dir of [
        new THREE.Vector3(-1, -1, -1).normalize(),
        new THREE.Vector3(0, -1, 0),
        new THREE.Vector3(1, 0, 0),
      ]) {
        const cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
        frameOrthoBox(cam, box, dir, 1.5);
        const { maxX, maxY } = projectedExtent(cam, box);
        expect(maxX, `宽 ${box.max.x * 2} 方向 ${dir.toArray()}: 横向不应超出`).toBeLessThanOrEqual(1.001);
        expect(maxY, `宽 ${box.max.x * 2} 方向 ${dir.toArray()}: 纵向不应超出`).toBeLessThanOrEqual(1.001);
      }
    }
  });

  it("宽视口时补足宽度，竖视口时补足高度", () => {
    const box = boxOf(10, 10, 10);
    const probe = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
    probe.position.set(0, 0, 10);
    probe.lookAt(0, 0, 0);
    const wide = fitOrthoHalfExtents(probe, box, 2.0);
    expect(wide.halfW / wide.halfH, "宽视口下宽高比应接近 2").toBeCloseTo(2.0, 1);
    const tall = fitOrthoHalfExtents(probe, box, 0.5);
    expect(tall.halfW / tall.halfH, "竖视口下宽高比应接近 0.5").toBeCloseTo(0.5, 1);
  });

  it("正交相机的缩放上下限是 zoom 区间", () => {
    const { min, max } = orthoZoomLimits();
    expect(min, "最小 zoom 应为正").toBeGreaterThan(0);
    expect(max, "最大 zoom 应大于最小").toBeGreaterThan(min);
  });

  it("改宽高比只在基准上重算，反复 resize 不会越缩越小也不会越放越大", () => {
    // 这条钉的是「拖几次分隔条正射视图就越缩越小」：视景体不能拿当前值
    // 反复推导，必须始终以取景时记下的基准半高为准
    const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 1000);
    setOrthoViewExtent(cam, 10, 1.6);
    cam.zoom = 2.5;

    // 同一宽高比来回切：可见范围必须回到同一个值
    for (const aspect of [0.6, 3.2, 1.6, 0.9, 1.6]) {
      adaptOrthoAspect(cam, aspect);
    }
    const back = orthoVisibleExtent(cam);
    expect(back.halfH, "回到原宽高比时半高应复原").toBeCloseTo(10, 6);
    expect(back.halfW, "回到原宽高比时半宽应复原").toBeCloseTo(16, 6);
    expect(cam.zoom, "缩放值不该被 resize 改动").toBeCloseTo(2.5, 6);
  });

  it("恢复正交快照后再次 resize 仍保持快照可见半高与缩放", () => {
    const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 1000);
    cam.userData.orthoBaseHalfHeight = 40;
    restoreOrthoExtent(cam, { left: -16, right: 16, top: 12, bottom: -12 }, 2, 2);
    adaptOrthoAspect(cam, 0.75);
    const restored = orthoVisibleExtent(cam);
    expect(restored.halfH, "可见半高应来自快照而不是旧基准").toBeCloseTo(6, 6);
    expect(cam.zoom, "恢复与 resize 不应改变快照缩放").toBeCloseTo(2, 6);
    expect(cam.userData.orthoBaseHalfHeight, "后续 resize 应使用新基准").toBeCloseTo(6, 6);
  });

  it("窄画布下竖直取景保持不变，只由宽高比决定宽度", () => {
    const cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    setOrthoViewExtent(cam, 10, 1.6);
    adaptOrthoAspect(cam, 0.5);
    const extent = orthoVisibleExtent(cam);
    expect(extent.halfH, "竖直方向仍看这么多").toBeCloseTo(10, 6);
    expect(extent.halfW, "水平方向按新宽高比收窄").toBeCloseTo(5, 6);
  });

  it("设视景体时保持用户缩放", () => {
    const cam = new THREE.OrthographicCamera(-10, 10, 10, -10, 0.1, 1000);
    cam.zoom = 3;
    setOrthoViewExtent(cam, 40, 1.5);
    expect(cam.zoom, "zoom 应原样保留").toBeCloseTo(3, 6);
    const extent = orthoVisibleExtent(cam);
    expect(extent.halfH, "可见半高应为给定值").toBeCloseTo(40, 6);
    expect(extent.halfW / extent.halfH, "可见宽高比应为给定值").toBeCloseTo(1.5, 6);
  });

  it("透视与正交的可见范围可以互相换算", () => {
    // 换投影时靠这层换算保住构图：透视距离 dist、FOV 45° 下看到的半高，
    // 设成正交视景体后必须看到同样大的一块
    const cam = new THREE.PerspectiveCamera(45, 1.6, 0.1, 1000);
    const dist = 120;
    const halfH = perspectiveViewHalfHeight(dist, cam.fov);
    const ortho = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    setOrthoViewExtent(ortho, halfH, 1.6);
    expect(orthoVisibleExtent(ortho).halfH, "正交可见半高应与透视一致").toBeCloseTo(
      halfH,
      6,
    );
    // 反算：正交的范围换回透视距离，应与原距离一致
    const backDist = halfH / Math.tan((cam.fov * Math.PI) / 360);
    expect(backDist, "换回透视时距离应复原").toBeCloseTo(dist, 6);
  });

  it("离屏内置真等距选择正交相机并完整取景", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const perspective = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
      const orthographic = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
      const aspect = 1.5;
      const selected = prepareRenderCamera(perspective, orthographic, group, "isoRight", aspect);

      expect(selected).toBe(orthographic);
      const box = new THREE.Box3().setFromObject(group);
      const { maxX, maxY, maxZ } = projectedExtent(orthographic, box);
      expect(maxX, "正交取景应覆盖模型横向投影").toBeLessThanOrEqual(1.001);
      expect(maxY, "正交取景应覆盖模型纵向投影").toBeLessThanOrEqual(1.001);
      expect(maxZ, "正交取景不应裁掉模型前后部分").toBeLessThanOrEqual(1.001);
      expect(Math.max(maxX, maxY), "模型不应只占据视口中央一小块").toBeGreaterThan(0.8);
      const extent = orthoVisibleExtent(orthographic);
      expect(extent.halfW / extent.halfH).toBeCloseTo(aspect, 6);
      const dir = orthographic.getWorldDirection(new THREE.Vector3());
      expect(dir.dot(presetDir("isoRight")), "相机应采用所选预设的视线").toBeCloseTo(1, 6);
    } finally {
      dispose();
    }
  });

  it("离屏普通预设仍选择透视相机", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const perspective = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
      const orthographic = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
      const selected = prepareRenderCamera(perspective, orthographic, group, "right45", 1.5);

      expect(selected).toBe(perspective);
      expect(perspective.aspect).toBeCloseTo(1.5, 6);
      const center = new THREE.Box3().setFromObject(group).getCenter(new THREE.Vector3());
      const toCenter = center.sub(perspective.position).normalize();
      const dir = perspective.getWorldDirection(new THREE.Vector3());
      expect(dir.dot(toCenter), "透视路径仍应朝向模型中心").toBeGreaterThan(0.999);
    } finally {
      dispose();
    }
  });

  it("离屏自定义正交预设保留位姿并按该朝向取景", () => {
    const { group, dispose } = buildSceneGroup(cubeScene());
    try {
      const perspective = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
      const orthographic = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 10000);
      const preset = {
        name: "自定义正交",
        ortho: true,
        position: [4, 6, 8] as [number, number, number],
        target: [0.5, 0.5, 0.5] as [number, number, number],
        yaw: 0,
        pitch: 0,
      };
      const aspect = 1.5;
      orthographic.up.set(0, 0, -1);
      const selected = prepareRenderCamera(perspective, orthographic, group, preset, aspect);

      expect(selected).toBe(orthographic);
      expect(orthographic.position.toArray(), "位置应按存档恢复").toEqual(preset.position);
      const expectedDir = new THREE.Vector3(...preset.target)
        .sub(new THREE.Vector3(...preset.position))
        .normalize();
      expect(
        orthographic.up.distanceTo(cameraUpFor(expectedDir)),
        "up 应按存档朝向恢复",
      ).toBeCloseTo(0, 12);
      const dir = orthographic.getWorldDirection(new THREE.Vector3());
      expect(dir.dot(expectedDir), "相机应注视存档目标").toBeCloseTo(1, 6);

      orthographic.updateMatrixWorld(true);
      const box = new THREE.Box3().setFromObject(group);
      const { maxX, maxY, maxZ } = projectedExtent(orthographic, box);
      expect(maxX, "恢复位姿后横向仍应装下模型").toBeLessThanOrEqual(1.001);
      expect(maxY, "恢复位姿后纵向仍应装下模型").toBeLessThanOrEqual(1.001);
      expect(maxZ, "恢复位姿后不应裁掉模型前后部分").toBeLessThanOrEqual(1.001);
      const extent = orthoVisibleExtent(orthographic);
      expect(extent.halfW / extent.halfH).toBeCloseTo(aspect, 6);
    } finally {
      dispose();
    }
  });
});
describe("轴向视角的相机 up 向量", () => {
  /** 由相机的世界矩阵取「屏幕右方」 */
  function screenRight(cam: THREE.Camera): THREE.Vector3 {
    cam.updateMatrixWorld(true);
    return new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 0).normalize();
  }

  it("顶视角下 +X 朝屏幕右方，画面不镜像", () => {
    // 这是实际踩到的 bug：up 取 (0,0,+1) 时屏幕右方成了 -X，
    // 预览里的文字「TiPlus9100」被镜像成「-0019sulPiT」
    const cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    cam.position.set(0, 100, 0);
    cam.up.copy(cameraUpFor(new THREE.Vector3(0, -1, 0)));
    cam.lookAt(0, 0, 0);
    const right = screenRight(cam);
    expect(right.x, "顶视时屏幕右方应是 +X").toBeGreaterThan(0.99);
  });

  it("顶视角下 +Z 朝屏幕下方", () => {
    // 与俯视图的通行画法一致：从上方往下看，南（+Z）在画面下方
    const cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    cam.position.set(0, 100, 0);
    cam.up.copy(cameraUpFor(new THREE.Vector3(0, -1, 0)));
    cam.lookAt(0, 0, 0);
    cam.updateMatrixWorld(true);
    const up = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 1).normalize();
    // 屏幕上方对应世界 -Z，因此 up 的 z 应为负
    expect(up.z, "顶视时屏幕上方应是 -Z").toBeLessThan(-0.99);
  });

  it("底视角同样保持 +X 朝右", () => {
    const cam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 1000);
    cam.position.set(0, -100, 0);
    cam.up.copy(cameraUpFor(new THREE.Vector3(0, 1, 0)));
    cam.lookAt(0, 0, 0);
    const right = screenRight(cam);
    expect(right.x, "底视时屏幕右方也应是 +X").toBeGreaterThan(0.99);
  });

  it("非轴向视角沿用世界 +Y 作为 up", () => {
    for (const dir of [
      new THREE.Vector3(-1, -1, -1).normalize(),
      new THREE.Vector3(0, -1, -1).normalize(),
      new THREE.Vector3(1, 0, 0),
    ]) {
      const up = cameraUpFor(dir);
      expect(up.x, `${dir.toArray()} 的 up 应为世界 +Y`).toBeCloseTo(0, 5);
      expect(up.y).toBeCloseTo(1, 5);
      expect(up.z).toBeCloseTo(0, 5);
    }
  });

  it("透视取景在顶视角下也不镜像", () => {
    // frameBox 原本没有设置 up，顶视角时 lookAt 与默认 up 平行，朝向由
    // three 的内部兜底决定，结果不可预期。这里钉死它同样走 cameraUpFor
    const cam = new THREE.PerspectiveCamera(45, 1, 0.1, 10000);
    const box = new THREE.Box3(
      new THREE.Vector3(-5, -1, -5),
      new THREE.Vector3(5, 1, 5),
    );
    frameBox(cam, box, new THREE.Vector3(0, -1, 0));
    const right = screenRight(cam);
    expect(right.x, "顶视角经 frameBox 后屏幕右方仍是 +X").toBeGreaterThan(0.99);
  });

  it("斜视角的 up 由 cameraUpFor 给出，落地后屏幕右方保持水平", () => {
    // 直接对斜方向调用 cameraUpFor 并 lookAt：屏幕右方不应有竖直分量，
    // 有分量就说明画面被滚转了（模型看着是歪的）。
    // 相机若停在顶视，up 会是 (0,0,-1)；此时不校正 up 直接 lookAt 就会滚。
    const cam = new THREE.PerspectiveCamera(45, 1.6, 0.1, 10000);
    const dir = new THREE.Vector3(-1, -1, -1).normalize(); // 相机 → 目标
    cam.position.copy(dir.clone().multiplyScalar(-100));
    cam.up.copy(cameraUpFor(dir));
    cam.lookAt(0, 0, 0);
    const right = screenRight(cam);
    expect(Math.abs(right.y), "斜视角下屏幕右方应为水平向量").toBeLessThan(1e-6);
  });
});
