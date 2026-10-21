#!/usr/bin/env node
/**
 * 前后端命令契约的静态一致性检查。
 *
 * 为什么需要它
 *   Tauri 按名字匹配命令与参数：前端 `invoke("search_blocks", {query})`
 *   必须对应后端 `pub fn search_blocks(query: String)`。名字写错**不会**
 *   编译失败，只在运行时表现为「点了按钮没反应」——正是最难排查的一类
 *   故障。而本项目的界面无法在无图形环境下自动测试，因此这道静态检查
 *   是唯一能在 CI 里拦住这类错误的防线。
 *
 * 检查内容
 *   1. 前端调用的每个命令名都出现在 lib.rs 的 generate_handler! 列表里
 *   2. 前端传的每个参数键，都能对应到后端某个参数（按 lowerCamelCase 比对）
 *   3. 后端每个必填参数（非 Option）都出现在前端调用里
 *   4. 后端无参命令，前端不应传参数
 *
 * 依据
 *   tauri-macros/src/command/wrapper.rs:501 会把 Rust 参数名做
 *   `to_lower_camel_case()` 再与前端键名匹配。
 *
 * 明确不覆盖
 *   参数类型是否兼容、命令的运行时行为。这些由 tsc、Rust 测试与人工验证负责。
 *
 * 用法：node scripts/check-ipc-contract.mjs
 */
import { readFileSync } from "node:fs";

const COMMANDS_RS = "src-tauri/src/commands.rs";
const LIB_RS = "src-tauri/src/lib.rs";
const API_TS = "src/api.ts";

const read = (p) => readFileSync(p, "utf8");

/** Rust 参数名 -> 前端应使用的键名（与 tauri-macros 的做法一致） */
function toLowerCamel(snake) {
  const parts = snake.split("_");
  return parts[0] + parts.slice(1).map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join("");
}

/** 从 commands.rs 提取：命令名 -> { params: [{name, optional, injected}] } */
function parseRustCommands(src) {
  const out = new Map();
  // 匹配 `pub fn name(` 到配对的 `)` 之前的内容（参数表可能跨多行）
  //
  // 允许 `pub async fn`：命令写成异步是为了不在主线程上做重活（Tauri 里
  // 同步命令跑在主线程上），它的参数契约与同步命令完全一样，不能因此漏检。
  const re = /pub\s+(?:async\s+)?fn\s+([a-z0-9_]+)\s*\(([\s\S]*?)\)\s*->/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const [, name, rawParams] = m;
    const params = [];
    // 参数按逗号切分；本项目的参数类型里没有嵌套泛型含逗号的情况，
    // 若将来出现需改用更稳的解析
    for (const piece of rawParams.split(",")) {
      const p = piece.trim();
      if (!p) continue;
      const mm = p.match(/^([a-z_][a-z0-9_]*)\s*:\s*([\s\S]+)$/i);
      if (!mm) continue;
      const [, pname, ptype] = mm;
      // AppHandle 由 Tauri 注入，不出现在前端调用里
      const injected = /AppHandle/.test(ptype);
      const optional = /Option\s*</.test(ptype);
      params.push({ name: pname, optional, injected });
    }
    out.set(name, { params });
  }
  return out;
}

/** 从 lib.rs 的 generate_handler! 提取注册的命令名 */
function parseHandlers(src) {
  const block = src.match(/generate_handler!\s*\[([\s\S]*?)\]/);
  if (!block) return new Set();
  return new Set(
    [...block[1].matchAll(/([a-z_][a-z0-9_:]*)\s*,/g)].map((m) => m[1].split("::").pop()),
  );
}

/**
 * 从 api.ts 提取：命令名 -> { keys, hasBody }
 *
 * 对象字面量必须做**深度感知**的切分，否则会误判：
 *   - 简写属性 `{ path }` 里的 path 是键，但用「标识符后跟逗号」的朴素
 *     正则匹配不到（后面是 `}`），会误报成「后端必填参数未传」
 *   - `{ name: name ?? null }` 里的 null 不是键，朴素的「标识符后跟逗号」
 *     会把它当键，误报成「前端传了后端没有的参数」
 * 一个会误报的检查等于没有检查，因此这里按括号深度与字符串边界解析。
 */
function parseFrontendCalls(src) {
  const out = new Map();
  // api.ts 的命令调用统一走它自己的 logged 出口（失败时记一条日志再上抛），
  // 只有 log_ui_event 为了不递归而直接 invoke。两种写法都要认，否则这里
  // 一条命令都匹配不到，检查会退化成「全部命令未被调用」的空转。
  const callRe = /(?:invoke|logged)\s*<[^>]*>\s*\(\s*"([a-z0-9_]+)"\s*(,\s*)?/g;
  let m;
  while ((m = callRe.exec(src)) !== null) {
    const cmd = m[1];
    const hasComma = Boolean(m[2]);
    const keys = [];
    let hasBody = false;
    if (hasComma) {
      // 从逗号后开始，找到顶层对象字面量的结束位置
      const start = callRe.lastIndex;
      const braceStart = src.indexOf("{", start);
      if (braceStart !== -1 && /^\s*\{/.test(src.slice(start, braceStart + 1))) {
        const end = matchBrace(src, braceStart);
        if (end !== -1) {
          hasBody = true;
          keys.push(...topLevelKeys(src.slice(braceStart + 1, end)));
        }
      }
    }
    out.set(cmd, { keys, hasBody });
  }
  return out;
}

/** 返回与 src[openIdx] 处 `{` 配对的 `}` 下标；找不到返回 -1 */
function matchBrace(src, openIdx) {
  let depth = 0;
  let str = null;
  for (let i = openIdx; i < src.length; i++) {
    const c = src[i];
    if (str) {
      if (c === "\\") i++;
      else if (c === str) str = null;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") str = c;
    else if (c === "{") depth++;
    else if (c === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** 取对象字面量内容里的顶层键（已去掉外层花括号） */
function topLevelKeys(body) {
  const keys = [];
  let depth = 0;
  let str = null;
  let start = 0;
  const pieces = [];
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (str) {
      if (c === "\\") i++;
      else if (c === str) str = null;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") str = c;
    else if ("{([".includes(c)) depth++;
    else if ("})]".includes(c)) depth--;
    else if (c === "," && depth === 0) {
      pieces.push(body.slice(start, i));
      start = i + 1;
    }
  }
  pieces.push(body.slice(start));

  for (const raw of pieces) {
    const piece = raw.trim();
    if (!piece) continue;
    if (piece.startsWith("...")) {
      keys.push("...");
      continue;
    }
    // 顶层冒号：`key: value`；没有冒号则是简写属性 `key`
    const colon = findTopLevelColon(piece);
    if (colon === -1) {
      const id = piece.match(/^([A-Za-z_][\w$]*)$/);
      if (id) keys.push(id[1]);
      continue;
    }
    const rawKey = piece.slice(0, colon).trim().replace(/^\.\.\./, "");
    // 计算属性名（[expr]）无法静态确定，记为未知
    const lit = rawKey.match(/^["'`]?([A-Za-z_][\w$]*)["'`]?$/);
    keys.push(lit ? lit[1] : "[computed]");
  }
  return keys;
}

function findTopLevelColon(piece) {
  let depth = 0;
  let str = null;
  for (let i = 0; i < piece.length; i++) {
    const c = piece[i];
    if (str) {
      if (c === "\\") i++;
      else if (c === str) str = null;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") str = c;
    else if ("{([".includes(c)) depth++;
    else if ("})]".includes(c)) depth--;
    else if (c === ":" && depth === 0) {
      // 排除 `??`、三元里的 `:`：简单起见要求前面不是 `?`
      return i;
    }
  }
  return -1;
}

const rustSrc = read(COMMANDS_RS);
const libSrc = read(LIB_RS);
const apiSrc = read(API_TS);

const rust = parseRustCommands(rustSrc);
const handlers = parseHandlers(libSrc);
const front = parseFrontendCalls(apiSrc);

const errors = [];
const warnings = [];

// 1) 前端调用的命令必须已注册且已实现
for (const [cmd, info] of front) {
  if (!rust.has(cmd)) {
    errors.push(`前端调用 ${cmd}，但 commands.rs 里没有这个函数`);
    continue;
  }
  if (!handlers.has(cmd)) {
    errors.push(`前端调用 ${cmd}，但它没有登记在 lib.rs 的 generate_handler! 里（运行时会报命令不存在）`);
  }
  const rp = rust.get(cmd).params.filter((p) => !p.injected);
  const expected = new Map(rp.map((p) => [toLowerCamel(p.name), p]));

  // 前端多传的键
  for (const k of info.keys) {
    if (k === "...") {
      warnings.push(`${cmd}：前端用了展开语法，无法静态核对键名`);
      continue;
    }
    if (!expected.has(k)) {
      errors.push(
        `${cmd}：前端传了 ${k}，后端没有对应参数（后端参数：${[...expected.keys()].join(", ") || "无"}）`,
      );
    }
  }
  // 后端必填参数未出现在前端
  for (const [camel, p] of expected) {
    if (!info.keys.includes(camel) && !info.keys.includes("...")) {
      if (p.optional) {
        warnings.push(`${cmd}：可选参数 ${camel} 未传（后端为 Option，通常可接受）`);
      } else {
        errors.push(`${cmd}：后端必填参数 ${camel} 前端未传（运行时会缺参报错）`);
      }
    }
  }
  if (rp.length === 0 && info.hasBody && info.keys.length > 0) {
    errors.push(`${cmd}：后端无参数，前端却传了 ${info.keys.join(", ")}`);
  }
}

// 2) 已注册但前端未调用的命令：仅提示
for (const h of handlers) {
  if (!front.has(h)) warnings.push(`${h}：已注册但 api.ts 未调用（可能是内部命令）`);
}

console.log(`命令：后端实现 ${rust.size} 个，已注册 ${handlers.size} 个，前端调用 ${front.size} 个`);
console.log("");
if (warnings.length) {
  console.log("提示：");
  for (const w of warnings) console.log(`  · ${w}`);
  console.log("");
}
if (errors.length) {
  console.log("错误：");
  for (const e of errors) console.log(`  ✗ ${e}`);
  console.log("");
  console.log(`契约检查失败：${errors.length} 项`);
  process.exit(1);
}
console.log("前后端命令契约一致。");
