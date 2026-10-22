import { describe, expect, it } from "vitest";
import {
  isPresetNameUsable,
  sanitizeViewPreset,
  sanitizeViewPresets,
  type SavedViewPreset,
} from "../render/scene3d";

/** 一条合法预设，用例里按需改字段 */
function valid(over: Partial<SavedViewPreset> = {}): SavedViewPreset {
  return {
    name: "正面核对",
    ortho: false,
    position: [10, 20, 30],
    target: [0, 0, 0],
    yaw: -45,
    pitch: 30,
    ...over,
  };
}

describe("自定义视角预设的校验", () => {
  it("合法预设原样通过", () => {
    const out = sanitizeViewPreset(valid());
    expect(out, "合法预设不应被丢弃").toBeTruthy();
    expect(out!.name).toBe("正面核对");
    expect(out!.position).toEqual([10, 20, 30]);
    expect(out!.ortho).toBe(false);
  });

  it("缺少字段或有非有限数值的一律丢弃", () => {
    // 磁盘上的数据可能是旧版本写的或被手工改坏。脏值套进相机会得到
    // 一个指向 NaN 的画面，而用户看不出是数据坏了
    expect(sanitizeViewPreset(null), "null 应丢弃").toBeNull();
    expect(sanitizeViewPreset("x"), "字符串应丢弃").toBeNull();
    expect(sanitizeViewPreset({}), "空对象应丢弃").toBeNull();
    expect(sanitizeViewPreset(valid({ name: "" })), "空名称应丢弃").toBeNull();
    expect(
      sanitizeViewPreset(valid({ name: "这名字实在是太长了长到超过二十四个字符的上限肯定不行" })),
      "超长名称应丢弃",
    ).toBeNull();
    expect(
      sanitizeViewPreset(valid({ position: [1, 2] as never })),
      "位置不足三项应丢弃",
    ).toBeNull();
    expect(
      sanitizeViewPreset(valid({ position: [1, NaN, 3] as never })),
      "含 NaN 的位置应丢弃",
    ).toBeNull();
    expect(
      sanitizeViewPreset(valid({ yaw: Number.POSITIVE_INFINITY })),
      "无限大的角度应丢弃",
    ).toBeNull();
  });

  it("位置与目标重合的预设被丢弃", () => {
    // 相机与注视点重合时没有朝向可言，取景会得到 NaN
    expect(
      sanitizeViewPreset(valid({ position: [0, 0, 0], target: [0, 0, 0] })),
      "位置与目标重合应丢弃",
    ).toBeNull();
  });

  it("名称首尾空白会被去掉", () => {
    const out = sanitizeViewPreset(valid({ name: "  空白名  " }));
    expect(out!.name, "应去掉首尾空白").toBe("空白名");
  });

  it("一批预设里重名的只留第一条", () => {
    const out = sanitizeViewPresets([
      valid({ name: "甲", position: [1, 0, 0] }),
      valid({ name: "甲", position: [2, 0, 0] }),
      valid({ name: "乙", position: [3, 0, 0] }),
      null,
      "bad",
    ]);
    expect(out.length, "应留下两条有效且去重后的预设").toBe(2);
    expect(out[0].position, "重名时留第一条").toEqual([1, 0, 0]);
    expect(out[1].name).toBe("乙");
  });

  it("非数组输入得到空列表而不是抛错", () => {
    // 旧版本的状态文件里根本没这个字段，读回来就是 undefined
    expect(sanitizeViewPresets(undefined)).toEqual([]);
    expect(sanitizeViewPresets(null)).toEqual([]);
    expect(sanitizeViewPresets({ a: 1 })).toEqual([]);
  });

  it("名称可用性：非空、不超长、不与已有重名", () => {
    const existing = [valid({ name: "甲" })];
    expect(isPresetNameUsable("乙", existing), "新名字应可用").toBe(true);
    expect(isPresetNameUsable("  ", existing), "空名字不可用").toBe(false);
    expect(isPresetNameUsable("甲", existing), "重名不可用").toBe(false);
    expect(
      isPresetNameUsable("这名字实在是太长了长到超过二十四个字符的上限肯定不行", existing),
      "超长不可用",
    ).toBe(false);
  });
});
