/** 界面语言：语言清单、文案实例，以及显示名称的取用规则。
 *
 * 各语言在这里成对定义。`locale` 给 vue-i18n 用，`lang` 是 Minecraft 语言
 * 文件的键名——客户端 jar 里名字表的路径就是 `assets/minecraft/lang/<lang>.json`，
 * 方块与实体的显示名称要按这个键去取，两者必须成对维护。
 */
import { createI18n } from "vue-i18n";
import arcoEnUS from "@arco-design/web-vue/es/locale/lang/en-us";
import arcoZhCN from "@arco-design/web-vue/es/locale/lang/zh-cn";
import arcoZhTW from "@arco-design/web-vue/es/locale/lang/zh-tw";
import enUS from "./en-US";
import zhCN from "./zh-CN";
import zhTW from "./zh-TW";

export const LOCALES = [
  { locale: "zh-CN", lang: "zh_cn", labelKey: "lang.zhCN" },
  { locale: "zh-TW", lang: "zh_tw", labelKey: "lang.zhTW" },
  { locale: "en-US", lang: "en_us", labelKey: "lang.enUS" },
] as const;

export type Locale = (typeof LOCALES)[number]["locale"];

/** 认不出来的语言一律回落到简体中文：文案缺了，比界面变成半截英文要好 */
export const FALLBACK_LOCALE: Locale = "zh-CN";

export function isLocale(value: unknown): value is Locale {
  return LOCALES.some((item) => item.locale === value);
}

const i18n = createI18n({
  legacy: false,
  locale: FALLBACK_LOCALE,
  fallbackLocale: FALLBACK_LOCALE,
  messages: { "zh-CN": zhCN, "zh-TW": zhTW, "en-US": enUS },
});

/** 组件里用 `useI18n()`；store 和工具函数不在组件上下文里，用这个 */
export const t = i18n.global.t;

/** 当前语言。响应式的：切换后用到它的模板会重绘 */
export const locale = i18n.global.locale;

export function currentLocale(): Locale {
  return isLocale(locale.value) ? locale.value : FALLBACK_LOCALE;
}

export function setLocale(next: Locale): void {
  locale.value = next;
  // index.html 上写死了 zh-CN；它影响断行、字体选择与朗读，切语言时要一起改
  document.documentElement.lang = next;
  // 窗口标题与任务栏/Alt-Tab 上显示的就是它，同样要跟着语言走
  document.title = t("titleBar.appName");
}

/** Arco 自己那些内置文案（分页、空数据、弹窗按钮）的语言包 */
const ARCO_LOCALES = { "zh-CN": arcoZhCN, "zh-TW": arcoZhTW, "en-US": arcoEnUS } as const;

export function arcoLocaleOf(current: Locale) {
  return ARCO_LOCALES[current];
}

/** 一条记录上带的显示名称：中英两套，由后端从 Minecraft 语言表里取出 */
export interface NamedRecord {
  zhName?: string;
  enName?: string;
}

/** 当前语言该优先看哪个名字，另一个是备选。后端只取 zh_cn 与 en_us 两张语言表，
 * 繁体中文界面没有对应的繁体名字表，按中文名优先，比退到英文名更贴近使用者 */
function namesByPreference(record: NamedRecord): [string, string] {
  return currentLocale() !== "en-US"
    ? [record.zhName ?? "", record.enName ?? ""]
    : [record.enName ?? "", record.zhName ?? ""];
}

/** 按当前界面语言取显示名称；这套缺了就退另一套，两套都缺返回空串 */
export function displayName(record: NamedRecord): string {
  const [preferred, fallback] = namesByPreference(record);
  return preferred || fallback;
}

/** 取到的是不是退而求其次的那套名字——界面上要标灰并说明语言表里没有 */
export function isFallbackName(record: NamedRecord): boolean {
  const [preferred, fallback] = namesByPreference(record);
  return !preferred && Boolean(fallback);
}

export default i18n;
