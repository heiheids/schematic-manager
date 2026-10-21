/**
 * 运行日志的一行解析成四段，供界面分行展示。
 *
 * 存储格式由后端的日志写入决定：`纳秒时间戳 \t 等级 \t 说明`。等级是
 * 大写的 ERROR / WARN / INFO / DEBUG；说明本身又以一个动词开头（「替换 …」
 * 「删除到回收站 …」「转换 …」），后面是文件名一类的细节。
 *
 * 为什么要在界面这一层拆：日志行是给文件存的，一整行读起来没有结构——
 * 时间戳是 20 位数字，说明里可能塞进几十个文件名，挤在一起无法逐条阅读。
 * 拆成「时间 / 等级 / 动作 / 细节」后，时间可以格式化成日期、等级与动作
 * 能各自成标签，每条之间的边界也就看得见了。
 */

/** 日志等级。取值与后端 `configure_run_log` 的 level 一致（小写） */
export const LOG_LEVELS = ["error", "warn", "info", "debug"] as const;

export type LogLevel = (typeof LOG_LEVELS)[number];

/** 等级在界面上的文案键（i18n 的 log 分区）。选择器与等级标签都要用，因此集中在这里 */
export const LOG_LEVEL_KEYS: Record<LogLevel, string> = {
  error: "log.levelError",
  warn: "log.levelWarn",
  info: "log.levelInfo",
  debug: "log.levelDebug",
};

/** 日志行里的等级列写成大写，识别时大小写不敏感 */
const LEVEL_TOKEN = new Map(LOG_LEVELS.map((level) => [level.toUpperCase(), level]));

/** 是不是一个合法的日志等级（用于校验界面状态里记下来的取值） */
export function isLogLevel(value: unknown): value is LogLevel {
  return typeof value === "string" && (LOG_LEVELS as readonly string[]).includes(value);
}

/** 一条运行日志 */
export interface LogEntry {
  /** 纳秒时间戳（纯数字串）；解析不出来时为空串 */
  timeNs: string;
  /** 等级（小写，与 LOG_LEVELS 一致）；列不全、取不到等级时为空串 */
  level: LogLevel | "";
  /** 开头的动作词；取不到时为空串 */
  action: string;
  /** 动作之后的细节（文件名等） */
  detail: string;
}

/** 解析一行日志 */
export function parseLogLine(line: string): LogEntry {
  const tab = line.indexOf("\t");
  const rawTime = tab >= 0 ? line.slice(0, tab) : "";
  const rest = tab >= 0 ? line.slice(tab + 1) : line;

  // 列可能不全：后端写一行要分两次 write_all，进程在两次之间被杀就会留下半行；
  // 手工编辑日志文件同样会造成缺列。这种行没有任何「这里是等级」的标记，
  // 只能靠取值判断：认得出等级 token 才切走这一列，否则整段都当作「动作 细节」。
  const nextTab = rest.indexOf("\t");
  const head = (nextTab >= 0 ? rest.slice(0, nextTab) : rest).trim();
  const level = LEVEL_TOKEN.get(head.toUpperCase()) ?? "";
  const text = level ? (nextTab >= 0 ? rest.slice(nextTab + 1) : "") : rest;

  const space = text.indexOf(" ");
  const action = space >= 0 ? text.slice(0, space) : text;
  const detail = space >= 0 ? text.slice(space + 1) : "";
  return {
    // 时间戳必须是纯数字，否则「时间」这一列会显示成一个词
    timeNs: /^\d+$/.test(rawTime) ? rawTime : "",
    level,
    action: action.trim(),
    detail: detail.trim(),
  };
}
