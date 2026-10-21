/**
 * 界面侧问题的上报通道：把界面自己遇到的问题写进运行日志。
 *
 * 为什么需要它：界面状态读写失败、订阅事件失败、未捕获异常这些只有界面知道。
 * 只 console.warn 的话，用户报「界面不对劲」时运行日志里一行线索都没有，
 * 而控制台是关掉窗口就没了的东西。因此日志是附加通道，控制台那份照旧输出，
 * 开发时仍然看得见。
 *
 * 上报失败不能再抛：一个诊断动作把主流程带崩是本末倒置。失败只补一条
 * console.warn，而且不再走本模块（否则失败会自我递归）。
 */
import { logUiEvent } from "../api";
import type { LogLevel } from "./run_log";

/** 记一条界面侧的警告：功能能继续，但需要留线索的问题 */
export function reportUiWarning(message: string, detail?: unknown): void {
  report("warn", message, detail);
}

/** 记一条界面侧的错误：已经没人接住、需要排查的问题 */
export function reportUiError(message: string, detail?: unknown): void {
  report("error", message, detail);
}

function report(level: LogLevel, message: string, detail: unknown): void {
  // 控制台这一份保持调用处的原样：带详情时作为第二个参数传，
  // 这样在开发者工具里看到的是对象本身而不是 String() 之后的一行文本
  const write = level === "error" ? console.error : console.warn;
  if (detail === undefined) write.call(console, message);
  else write.call(console, message, detail);

  const line = detail === undefined ? message : `${message} ${String(detail)}`;
  void logUiEvent(level, line).catch((e) => {
    console.warn("写入运行日志失败：", e);
  });
}
