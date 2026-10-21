import { beforeEach, describe, expect, it, vi } from "vitest";

// 把 Tauri 的 invoke 换成可观察的替身。
// 这里不 mock 被测模块本身——测的是真实的 api.ts，只是把它的出口拦下来，
// 看它究竟发出了什么命令、带了什么参数。
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import * as api from "../api";

const mockInvoke = vi.mocked(invoke);

/** 取上一次调用发出的 (命令名, 参数对象) */
function lastCall(): { cmd: string; args: Record<string, unknown> } {
  const call = mockInvoke.mock.calls.at(-1);
  if (!call) throw new Error("没有发出任何命令");
  return { cmd: call[0] as string, args: (call[1] ?? {}) as Record<string, unknown> };
}

/**
 * 断言一次调用。
 *
 * `args` 只列出该命令**必须**携带的键；额外的键不判失败，因为
 * 一个命令多传一个后端忽略的参数不会导致功能失效，而少传或传错名会。
 */
function expectCall(
  fn: () => Promise<unknown>,
  cmd: string,
  args: Record<string, unknown> = {},
): Promise<void> {
  return fn().then(() => {
    const c = lastCall();
    expect(c.cmd, "命令名必须与后端注册的一致").toBe(cmd);
    for (const [k, v] of Object.entries(args)) {
      expect(c.args, `参数 ${k} 必须存在且取值正确`).toHaveProperty(k, v);
    }
  });
}

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(undefined as never);
});

describe("投影与扫描", () => {
  it("peekSchematic 传 path", async () => {
    await expectCall(() => api.peekSchematic("/a/b.litematic"), "peek_schematic", {
      path: "/a/b.litematic",
    });
  });

  it("scanLitematics 传 dir 与 maxDepth", async () => {
    await expectCall(() => api.scanLitematics("/dir"), "scan_litematics", {
      dir: "/dir",
      maxDepth: 6,
    });
  });

  it("scanLitematics 的自定义深度生效", async () => {
    await expectCall(() => api.scanLitematics("/dir", 2), "scan_litematics", {
      maxDepth: 2,
    });
  });

  it("analyzeSchematic 传 path", async () => {
    await expectCall(() => api.analyzeSchematic("/a.litematic"), "analyze_schematic", {
      path: "/a.litematic",
    });
  });

  it("buildScene 传 schematicPath（前端驼峰，后端 schematic_path）", async () => {
    // Tauri 按 camelCase 匹配参数名；这条断言防的是有人改成 snake_case
    await expectCall(() => api.buildScene("/a.litematic"), "build_scene", {
      schematicPath: "/a.litematic",
    });
  });

  it("saveMetadata 把三个字段平铺，未给的字段不出现在参数里", async () => {
    await expectCall(
      () => api.saveMetadata("/a.litematic", { name: "新名字" }),
      "save_metadata",
      { path: "/a.litematic", name: "新名字" },
    );
    // 未提供的作者/描述不应以 undefined 形式发出：后端把 Option 当缺失处理，
    // 传 undefined 与不传等价，但显式断言可避免将来被改成空串覆盖原值
    const { args } = lastCall();
    expect(args).not.toHaveProperty("author");
    expect(args).not.toHaveProperty("description");
  });

  it("mcVersionName 传 dataVersion", async () => {
    await expectCall(() => api.mcVersionName(3465), "mc_version_name", {
      dataVersion: 3465,
    });
  });
});

describe("方块库", () => {
  it("searchBlocks 传 query 与 limit", async () => {
    await expectCall(() => api.searchBlocks("石头", 50), "search_blocks", {
      query: "石头",
      limit: 50,
    });
  });

  it("searchBlocks 有默认 limit", async () => {
    await expectCall(() => api.searchBlocks("x"), "search_blocks", { limit: 200 });
  });

  it("listBlockVersions 只发命令名、不带参数", async () => {
    await expectCall(() => api.listBlockVersions(), "list_block_versions");
    // 无参命令应当只发一个参数（命令名），不发空的参数对象：
    // Tauri 对两者都能处理，但只发命令名更贴合后端的无参签名
    expect(mockInvoke.mock.calls.at(-1)?.length, "只应传命令名").toBe(1);
  });
});

describe("替换与转换", () => {
  it("previewReplace 传 schematicPath 与 rule", async () => {
    const rule = {
      fromName: "a",
      fromProps: [],
      toName: "b",
      toProps: [],
      inheritProps: true,
    };
    await expectCall(() => api.previewReplace("/a.litematic", rule), "preview_replace", {
      schematicPath: "/a.litematic",
      rule,
    });
  });

  it("applyReplace 传 schematicPath 与 rule", async () => {
    const rule = {
      fromName: "a",
      fromProps: [],
      toName: "b",
      toProps: [],
      inheritProps: false,
    };
    await expectCall(() => api.applyReplace("/a.litematic", rule), "apply_replace", {
      schematicPath: "/a.litematic",
      rule,
    });
  });

  it("convertTargets 传 schematicPath", async () => {
    await expectCall(() => api.convertTargets("/a.litematic"), "convert_targets", {
      schematicPath: "/a.litematic",
    });
  });

  it("analyzeConversion 传 targetIndex（非 target_index）", async () => {
    await expectCall(() => api.analyzeConversion("/a.litematic", 3), "analyze_conversion", {
      schematicPath: "/a.litematic",
      targetIndex: 3,
    });
  });

  it("applyConversion 传 targetIndex 与 items", async () => {
    const items = [
      {
        name: "a",
        nameZh: "甲",
        nameEn: "Alpha",
        count: 1,
        suggested: "b",
        suggestedZh: "乙",
        suggestedEn: "Beta",
        chosen: "b",
      },
    ];
    await expectCall(
      () => api.applyConversion("/a.litematic", 1, items),
      "apply_conversion",
      { schematicPath: "/a.litematic", targetIndex: 1, items },
    );
  });
});

describe("重复筛查与备份", () => {
  it("scanDuplicates 传 dirs（一次交全部目录）", async () => {
    await expectCall(() => api.scanDuplicates(["/dir", "/dir2"]), "scan_duplicates", {
      dirs: ["/dir", "/dir2"],
    });
  });

  it("listBackups 不传 stem 时发 null", async () => {
    // 后端是 Option<String>：发 null 表示「不筛选」，与省略不同
    await expectCall(() => api.listBackups(), "list_backups", { stem: null });
  });

  it("listBackups 传 stem", async () => {
    await expectCall(() => api.listBackups("x"), "list_backups", { stem: "x" });
  });

  it("restoreBackup 传 backupPath 与 targetPath", async () => {
    await expectCall(() => api.restoreBackup("/b/x", "/t/x"), "restore_backup", {
      backupPath: "/b/x",
      targetPath: "/t/x",
    });
  });

  it("deleteBackup 传 backupPath", async () => {
    await expectCall(() => api.deleteBackup("/b/x"), "delete_backup", {
      backupPath: "/b/x",
    });
  });

  it("readRunLog 传 maxLines", async () => {
    await expectCall(() => api.readRunLog(10), "read_run_log", {
      maxLines: 10,
    });
  });

  it("configureRunLog 传 level 与 localDate", async () => {
    // 日期是命令的一部分：后端按它决定日志写进哪一天的文件
    await expectCall(() => api.configureRunLog("debug", "2026-10-19"), "configure_run_log", {
      level: "debug",
      localDate: "2026-10-19",
    });
  });

  it("clearRunLog 不带参数", async () => {
    await expectCall(() => api.clearRunLog(), "clear_run_log");
  });

  it("openRunLogDir 不带参数", async () => {
    await expectCall(() => api.openRunLogDir(), "open_run_log_dir");
  });

  it("logUiEvent 传 level 与 message", async () => {
    await expectCall(
      () => api.logUiEvent("warn", "保存界面状态失败：磁盘只读"),
      "log_ui_event",
      { level: "warn", message: "保存界面状态失败：磁盘只读" },
    );
  });

  it("runLogDir 只发命令名、不带参数", async () => {
    await expectCall(() => api.runLogDir(), "run_log_dir");
    expect(mockInvoke.mock.calls.at(-1)?.length, "只应传命令名").toBe(1);
  });
});

describe("契约整体性", () => {
  it("所有导出函数发出的命令名都是小写下划线形式", async () => {
    // 后端命令名一律 snake_case；出现驼峰或大写必然匹配不上
    const calls: Array<() => Promise<unknown>> = [
      () => api.peekSchematic("a"),
      () => api.scanLitematics("d"),
      () => api.analyzeSchematic("a"),
      () => api.saveMetadata("a", {}),
      () => api.buildScene("a"),
      () => api.searchBlocks("q"),
      () => api.listBlockVersions(),
      () => api.previewReplace("a", {
        fromName: "x", fromProps: [], toName: "y", toProps: [], inheritProps: true,
      }),
      () => api.applyReplace("a", {
        fromName: "x", fromProps: [], toName: "y", toProps: [], inheritProps: true,
      }),
      () => api.convertTargets("a"),
      () => api.analyzeConversion("a", 0),
      () => api.applyConversion("a", 0, []),
      () => api.scanDuplicates(["d"]),
      () => api.listBackups(),
      () => api.restoreBackup("b", "t"),
      () => api.deleteBackup("b"),
      () => api.readRunLog(),
      () => api.configureRunLog("info", "2026-10-19"),
      () => api.clearRunLog(),
      () => api.openRunLogDir(),
      () => api.logUiEvent("warn", "x"),
      () => api.runLogDir(),
      () => api.mcVersionName(1),
    ];
    const names: string[] = [];
    for (const c of calls) {
      mockInvoke.mockClear();
      await c();
      names.push(lastCall().cmd);
    }
    for (const n of names) {
      expect(n, `命令名 ${n} 应为 snake_case`).toMatch(/^[a-z][a-z0-9_]*$/);
    }
    // 不允许重名：两个功能共用一个命令说明其中一个实现错了
    expect(new Set(names).size, "命令名不应重复").toBe(names.length);
  });

  it("参数名一律是驼峰（后端靠 lowerCamelCase 匹配）", async () => {
    // 依据 tauri-macros/src/command/wrapper.rs:501：
    // Rust 侧参数名会被 `to_lower_camel_case()` 转换后再去匹配前端键名。
    // 因此前端必须发 lowerCamelCase；发 snake_case 会匹配不上。
    // 单词参数（dir、path、rule）转换后仍是自身，所以这里的判据是
    // 「不含下划线」，而不是「必含大写字母」。
    const cases: Array<() => Promise<unknown>> = [
      () => api.scanLitematics("d"),
      () => api.buildScene("a"),
      () => api.analyzeConversion("a", 0),
      () => api.applyConversion("a", 0, []),
      () => api.analyzeSchematic("a"),
      () => api.saveMetadata("a", { name: "n", author: "au" }),
      // localDate 尤其要盯着：写成 local_date 后端就匹配不上了
      () => api.configureRunLog("info", "2026-10-19"),
    ];
    for (const c of cases) {
      mockInvoke.mockClear();
      await c();
      const { args } = lastCall();
      expect(Object.keys(args).length, "至少应有一个参数").toBeGreaterThan(0);
      for (const k of Object.keys(args)) {
        expect(k, `参数名 ${k} 不应含下划线（后端按 lowerCamelCase 匹配）`).not.toContain("_");
      }
    }
  });

  it("appVersion 与 checkUpdate 都是无参命令", async () => {
    await expectCall(() => api.appVersion(), "app_version");
    await expectCall(() => api.checkUpdate(), "check_update");
  });
});

describe("命令失败的上报", () => {
  it("命令失败时既抛给调用方，也记一条带命令名的 error 日志", async () => {
    // 用户报「点了报错」时，日志里得能查到是哪条命令、报了什么。
    // 界面弹的提示关掉就没了，因此这条链路要单独钉住。
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "delete_backup") throw new Error("权限不足");
      return undefined;
    }) as never);

    // (a) 原始错误照旧抛给调用方，不能被上报动作替换或吞掉
    await expect(api.deleteBackup("/b/x")).rejects.toThrow("权限不足");

    // (b) 上报一条 error 级日志，说明里带上命令名
    const events = mockInvoke.mock.calls.filter((c) => c[0] === "log_ui_event");
    expect(events.length, "失败应上报一条界面日志").toBe(1);
    const args = events[0][1] as { level: string; message: string };
    expect(args.level, "命令失败属于错误").toBe("error");
    expect(args.message, "说明里要带上命令名才好查").toContain("delete_backup");
  });

  it("上报本身失败时仍然把原始错误抛给调用方", async () => {
    // 写日志的通道不能把主流程带崩，也不能盖掉真正的原因
    mockInvoke.mockImplementation((async (cmd: string) => {
      if (cmd === "delete_backup") throw new Error("权限不足");
      if (cmd === "log_ui_event") throw new Error("日志目录不可写");
      return undefined;
    }) as never);

    await expect(api.deleteBackup("/b/x")).rejects.toThrow("权限不足");
  });
});
