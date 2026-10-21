/**
 * 运行日志行的解析。
 *
 * 存储格式由后端的日志写入决定：`纳秒时间戳 \t 等级 \t 说明`，等级是大写的
 * ERROR / WARN / INFO / DEBUG。这里钉住的是「界面据此分行展示」所依赖的
 * 前提：时间戳能单独取出来（且不是数字时不当成时间），等级能认出来（列不全的
 * 行不能把说明的第一个词误当成等级），说明里的动作词与细节能分开。
 */
import { describe, expect, it } from "vitest";
import { isLogLevel, parseLogLine } from "../utils/run_log";

describe("运行日志行解析", () => {
  it("拆出纳秒时间戳、等级、动作与细节", () => {
    const e = parseLogLine(
      "17921400658821747800\tINFO\t删除到回收站 24 个文件：a.litematic、b.litematic",
    );
    expect(e.timeNs).toBe("17921400658821747800");
    expect(e.level).toBe("info");
    expect(e.action).toBe("删除到回收站");
    expect(e.detail).toBe("24 个文件：a.litematic、b.litematic");
  });

  it("四档等级都能认出来", () => {
    for (const [token, level] of [
      ["ERROR", "error"],
      ["WARN", "warn"],
      ["INFO", "info"],
      ["DEBUG", "debug"],
    ] as const) {
      expect(parseLogLine(`1\t${token}\t替换 a.litematic`).level, token).toBe(level);
    }
  });

  it("等级大小写不敏感", () => {
    // 后端统一写大写；手工编辑过的行可能是小写，仍然要认出来而不是当成动作
    const e = parseLogLine("17921400658821747800\twarn\t替换 a.litematic：石头 -> 泥土");
    expect(e.level).toBe("warn");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });

  it("说明里的换行与制表符是转义后的字面量，不该被当成列分隔", () => {
    // 后端把说明里的换行写成 \n、制表符写成 \t（各两个字符），一行仍是一条
    const e = parseLogLine("17921400658821747800\tERROR\t替换 a.litematic\\n失败\\t已回滚");
    expect(e.level).toBe("error");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic\\n失败\\t已回滚");
  });

  it("列不全（只有两列）时整段当作「动作 细节」", () => {
    // 写一行分两次 write_all，进程在中间被杀就会留下这种半行；手工编辑也会
    const e = parseLogLine("17921400658821747800\t替换 a.litematic：石头 -> 泥土");
    expect(e.timeNs).toBe("17921400658821747800");
    expect(e.level).toBe("");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });

  it("第二列不是已知等级时不当成等级", () => {
    // 缺列或手工编辑的行里第二列可能是任意词；只有四个等级 token 才算等级列
    const e = parseLogLine("17921400658821747800\tINFOX 替换 a.litematic");
    expect(e.level).toBe("");
    expect(e.action).toBe("INFOX");
    expect(e.detail).toBe("替换 a.litematic");
  });

  it("动作只有一个词时细节为空", () => {
    const e = parseLogLine("17921400658821747800\tWARN\t还原");
    expect(e.level).toBe("warn");
    expect(e.action).toBe("还原");
    expect(e.detail).toBe("");
  });

  it("没有制表符时整行当作说明，时间与等级留空", () => {
    // 写入被截断或手工编辑过、连制表符都没有的行
    const e = parseLogLine("替换 a.litematic：石头 -> 泥土");
    expect(e.timeNs).toBe("");
    expect(e.level).toBe("");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });

  it("时间戳不是数字时不当成时间，但其余部分照常拆开", () => {
    // 测试替身与手工编辑都可能写出这种行；若把「2026-10-15 12:00:00」
    // 当成纳秒时间戳，会显示成 1970 年
    const e = parseLogLine("2026-10-15 12:00:00\tWARN\t替换 a.litematic：石头 -> 泥土");
    expect(e.timeNs).toBe("");
    expect(e.level).toBe("warn");
    expect(e.action).toBe("替换");
    expect(e.detail).toBe("a.litematic：石头 -> 泥土");
  });
});

describe("日志等级取值校验", () => {
  it("只认约定里的四档", () => {
    for (const level of ["error", "warn", "info", "debug"]) {
      expect(isLogLevel(level), level).toBe(true);
    }
    for (const bad of ["ERROR", "trace", "", undefined, null, 1, {}]) {
      expect(isLogLevel(bad), String(bad)).toBe(false);
    }
  });
});
