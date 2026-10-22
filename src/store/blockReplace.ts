/**
 * 方块替换域：替换弹窗的全部状态与流程（规则表单、方块检索、
 * 中文名联想、预览、执行替换）。
 *
 * 这是一个自包含的向导式流程：打开弹窗 → 填规则 → 预览 → 执行，
 * 状态只在弹窗生命周期内有意义，与文件列表、3D 预览彼此独立。
 *
 * 与其他域的协作：执行成功后经 BlockReplaceDeps 让文件列表域重扫并
 * 刷新选中项，再让 3D 预览域重建场景，使界面立刻反映替换结果。
 * 依赖只声明本域用到的成员，避免类型上的循环引用（见 store/fileList.ts）。
 */
import { onBeforeUnmount, ref, watch } from "vue";
import type { ComputedRef, Ref } from "vue";
import { Message } from "@arco-design/web-vue";
import type { TableData } from "@arco-design/web-vue";
import { displayName, t } from "../i18n";
import type { NamedRecord } from "../i18n";
import {
  applyReplace,
  listBlockVersions,
  previewReplace,
  searchBlocks,
} from "../api";
import type { BlockHit, BlockRow, ReplaceRule, SchematicSummary, VersionOption } from "../types";

/** 方块替换域用到的其他域成员（循环引用回避见 fileList.ts） */
export interface BlockReplaceDeps {
  /** 文件列表域的当前选中；替换的作用对象 */
  selected: Ref<SchematicSummary | null>;
  /** 文件列表域的文件清单；替换后按路径找回同名文件 */
  files: ComputedRef<SchematicSummary[]>;
  /** 文件列表域的重扫；替换生成新文件后刷新清单 */
  rescanAll: () => Promise<void>;
  /** 文件列表域的草稿复位；替换后重新载入元数据 */
  resetDraft: (rec: SchematicSummary) => void;
  /** 3D 预览域的场景加载；替换后重建场景 */
  loadScene: (record: SchematicSummary) => void;
  /** 把替换规则记入待保存的草稿；保存时才写回文件 */
  stageReplace: (rule: ReplaceRule) => void;
}

export function createBlockReplaceSlice(s: BlockReplaceDeps) {
  const activeBlock = ref<{ name: string } | null>(null);

  // 方块替换
  const replaceOpen = ref(false);
  const replaceBusy = ref(false);
  const blockVersions = ref<VersionOption[]>([]);
  const replaceForm = ref({
    fromName: "",
    fromPropsText: "",
    toName: "",
    toPropsText: "",
    inheritProps: true,
  });
  const replacePreview = ref<number | null>(null);

  // 右侧方块列表：搜索并点选
  const blockPickQuery = ref("");
  const blockHits = ref<BlockHit[]>([]);
  const blockSearching = ref(false);
  /** 搜索请求序号：只接受最后一次查询的结果（逐字符搜会并发多个请求） */
  let blockSearchSeq = 0;

  /** 右侧列表点选后填到哪一个输入框。
   *
   * 弹窗里有两个方块输入框（源、目标），右侧列表必须知道该填哪个，
   * 否则点下去不知道改的是哪一栏。跟随用户最后点过/聚焦的那个输入框，
   * 默认目标是替换目标——替换时更常选的是「换成什么」。
   */
  const replacePickField = ref<"from" | "to">("to");

  /** 方块 ID -> 显示名称的两套名字（中文、英文，取自 Minecraft 语言表）。
   *
   * 源方块与目标方块共用一份：两个输入框都要显示名字，而来源有三处
   * （手输、右侧列表点选、从「方块」页签点行填入），汇总到一处才不会
   * 出现同一个方块在两个框里显示不同名字。
   *
   * 两套都留着、而不是只存当前语言那一套：切语言后输入框要立刻改用另一套，
   * 只存一套会让它继续显示旧语言的名字。
   */
  const blockNames = ref<Record<string, NamedRecord>>({});

  /** 已经查过的方块 ID。
   *
   * 与 blockNames 分开是为了区分「还没查」与「库里没有这个名字」：
   * 只看有没有名字的话，两种情况的界面表现一样，用户分不清是自己
   * 还没查还是这个方块本来就没有名字。
   */
  const blockNamesQueried = ref<Set<string>>(new Set());

  /** 待触发的查询，按方块 ID 分开存。
   *
   * 不能共用一个计时器：在两个输入框之间快速输入时，后一次输入会把前一个
   * 还没落地的查询取消掉，那一栏的名字就再也不显示了。
   */
  const blockNameTimers = new Map<string, ReturnType<typeof setTimeout>>();

  /** 从「方块 ID 或 带属性的状态串」里取出纯 ID。
   *
   * 输入框是自由文本，用户可能把手输/粘贴的 `minecraft:chest[facing=east]`
   * 这种状态串直接留在框里。严格按整串去方块库比对必然匹配不上，于是明明
   * 能找到的方块也会被报成「未找到」。方块库里的名字不含属性，因此先剥掉
   * 方括号部分。
   */
  function bareBlockId(raw: string): string {
    const i = raw.indexOf("[");
    return (i >= 0 ? raw.slice(0, i) : raw).trim();
  }

  /** 把「方块名[属性]」拆成方块名与属性串；不是这种写法时返回 null。
   *
   * 后端契约要求方块名不含属性（见 replace.rs 的 ReplaceRule）：带属性的名字
   * 会被原样写进投影的调色板，游戏读不出来。旧界面用下拉框选方块，取值域
   * 天然挡住了这条路；改成自由文本后必须自己拆。拆法与从「方块」页签点行
   * 填入一致——名字进方块栏，方括号里的进属性栏，用户看得见拆成了什么。
   */
  function splitStateString(raw: string): { name: string; props: string } | null {
    const m = /^([^[\]]+)\[([^\]]*)\]\s*$/.exec(raw.trim());
    if (!m) return null;
    return { name: m[1].trim(), props: m[2].trim() };
  }

  function rememberBlockName(id: string, names: NamedRecord) {
    const checked = new Set(blockNamesQueried.value);
    checked.add(id);
    blockNames.value = { ...blockNames.value, [id]: names };
    blockNamesQueried.value = checked;
  }

  /** 查一个方块的显示名称。查不到时记为空（界面据此说明库里没有） */
  async function lookupBlockName(raw: string) {
    const id = bareBlockId(raw);
    if (!id || blockNameQueried(id)) return;
    try {
      const hits = await searchBlocks(id, 50);
      // 只认名字完全一致的命中：搜索是模糊匹配，取自第一项会把
      // minecraft:oak_stairs 显示成 minecraft:oak_planks 这类近邻的名字
      const exact = hits.find((h) => h.name === id);
      rememberBlockName(id, { zhName: exact?.zh, enName: exact?.en });
    } catch (e) {
      // 查不到名字不该打断替换流程，但也不能悄悄当成「库里没有」：
      // 记为未查过，下次改动会再试一遍
      Message.error(t("replace.lookupFailed", { error: String(e) }));
    }
  }

  /** 输入框每敲一个字符都会触发，用防抖；逐字符查库既无必要也拖慢输入 */
  function scheduleBlockName(raw: string) {
    const id = bareBlockId(raw);
    if (!id) return;
    const pending = blockNameTimers.get(id);
    if (pending !== undefined) clearTimeout(pending);
    blockNameTimers.set(
      id,
      setTimeout(() => {
        blockNameTimers.delete(id);
        void lookupBlockName(id);
      }, 200),
    );
  }

  /** 方块的显示名称（按当前界面语言取）；没有记录时返回空串 */
  function blockNameOf(raw: string): string {
    const id = bareBlockId(raw);
    return id ? displayName(blockNames.value[id] ?? {}) : "";
  }

  /** 这个方块是否已经查过名字（用于区分「没查」与「库里没有」） */
  function blockNameQueried(raw: string): boolean {
    return blockNamesQueried.value.has(bareBlockId(raw));
  }

  /** 让右侧列表接下来填到指定的输入框 */
  function setPickField(field: "from" | "to") {
    replacePickField.value = field;
  }

  /** 方块栏的值变了：先拆掉可能粘贴进来的属性串，再查中文名。
   *
   * 拆完会再触发一次本函数（值已不含方括号，第二次直接走到查询），因此
   * 不会反复。
   */
  function onNameChanged(raw: string, field: "from" | "to") {
    const split = splitStateString(raw);
    if (split) {
      if (field === "from") {
        replaceForm.value.fromName = split.name;
        replaceForm.value.fromPropsText = split.props;
      } else {
        replaceForm.value.toName = split.name;
        replaceForm.value.toPropsText = split.props;
      }
      return;
    }
    scheduleBlockName(raw);
  }

  watch(() => replaceForm.value.fromName, (v) => onNameChanged(v, "from"));
  watch(() => replaceForm.value.toName, (v) => onNameChanged(v, "to"));

  /** 预览请求序号：规则改过之后，早先发出去的预览结果不能再写回界面 */
  let previewSeq = 0;

  // 规则一改，之前算出的替换数量就不再对应当前规则了：留着那个数字会让
  // 用户以为「按现在填的规则只有这么多个」。同时在途的预览请求也一并作废。
  watch(
    replaceForm,
    () => {
      replacePreview.value = null;
      previewSeq += 1;
    },
    { deep: true },
  );

  // 卸载时清掉待触发的查询：否则组件已销毁仍会发一次无意义的请求
  onBeforeUnmount(() => {
    for (const timer of blockNameTimers.values()) clearTimeout(timer);
    blockNameTimers.clear();
  });

  /** 解析 "key=value,key2=value2" 形式的属性限定；空串表示不限定 */
  function parseProps(text: string): [string, string][] {
    const out: [string, string][] = [];
    for (const seg of text.split(",")) {
      const t = seg.trim();
      if (!t) continue;
      const i = t.indexOf("=");
      if (i <= 0) continue;
      out.push([t.slice(0, i).trim(), t.slice(i + 1).trim()]);
    }
    return out;
  }

  function currentRule(): ReplaceRule {
    const f = replaceForm.value;
    return {
      // 再剥一次属性：契约要求方块名不含属性，而这里是最后一次把关，
      // 带属性的名字会被原样写进投影的调色板
      fromName: bareBlockId(f.fromName),
      fromProps: parseProps(f.fromPropsText),
      toName: bareBlockId(f.toName),
      toProps: parseProps(f.toPropsText),
      inheritProps: f.inheritProps,
    };
  }

  async function onBlockSearch(text: string) {
    const q = text.trim();
    // 记下这次查询的序号：逐字符搜索会并发多个请求，慢的旧响应回来时
    // 不能覆盖新结果，否则列表内容与输入框里的关键字对不上
    const seq = ++blockSearchSeq;
    if (!q) {
      blockHits.value = [];
      // 空查询自己就是最新的一次查询，而它在途的前一次查询已经因为序号
      // 不匹配不再复位这个标志；这里必须复位，否则「输入后立刻清空」会把
      // 转圈状态永久留在界面上
      blockSearching.value = false;
      return;
    }
    blockSearching.value = true;
    try {
      const hits = await searchBlocks(q, 200);
      if (seq !== blockSearchSeq) return; // 已有更新的查询，丢弃这份过期结果
      blockHits.value = hits;
      // 顺手把结果里的名字记下来（并标记为已查过），点选时就不必再查
      const names = { ...blockNames.value };
      const checked = new Set(blockNamesQueried.value);
      for (const h of hits) {
        names[h.name] = { zhName: h.zh, enName: h.en };
        checked.add(h.name);
      }
      blockNames.value = names;
      blockNamesQueried.value = checked;
    } catch (e) {
      if (seq !== blockSearchSeq) return;
      Message.error(t("replace.searchFailed", { error: String(e) }));
    } finally {
      if (seq === blockSearchSeq) blockSearching.value = false;
    }
  }

  /** 点选右侧列表里的方块，填到当前选定的那个输入框 */
  function pickBlock(hit: BlockHit) {
    const f = replaceForm.value;
    rememberBlockName(hit.name, { zhName: hit.zh, enName: hit.en });
    // 提示里报出用户当前语言看得懂的名字，而不是方块 ID
    const shown = displayName({ zhName: hit.zh, enName: hit.en }) || hit.name;
    if (replacePickField.value === "from") {
      // 换源方块必须同时清掉属性限定：属性是上一个方块特有的，留着它
      // 会让这条规则一个都匹配不上，而界面看上去「明明填好了」
      const hadProps = f.fromPropsText.trim().length > 0;
      f.fromName = hit.name;
      f.fromPropsText = "";
      Message.success(
        t("replace.setSource", { name: shown }) +
          (hadProps ? t("replace.sourcePropsCleared") : ""),
      );
    } else {
      // 目标属性覆盖同理：它是无条件写上去的（replace.rs 里 to_props 最后
      // 强制 set_prop），留着上一个目标方块的属性会把新目标也带上那个属性
      const hadProps = f.toPropsText.trim().length > 0;
      f.toName = hit.name;
      f.toPropsText = "";
      Message.success(
        t("replace.setTarget", { name: shown }) +
          (hadProps ? t("replace.targetPropsCleared") : ""),
      );
    }
  }

  /** 点方块统计表的一行，把它设为替换源。
   *
   * 表里的行是**物品**行（水折成水桶、门只算下半），而替换规则在后端按方块名
   * 精确相等匹配（见 replace.rs 的 matches）。因此替换源取这一行的代表状态
   * 所属的方块名，而不是行名：行名可能是这个投影里根本不存在的物品——库里
   * 只有墙上的告示牌时，行名是告示牌物品，填进去一条都匹配不上。
   *
   * 两种情况必须说清覆盖范围，否则用户以为「替换整行」而实际只替换了一部分：
   * 跨多个方块名时（火把与墙上的火把）只覆盖其中一个；合计了多条状态记录时
   * 不带属性限定。
   */
  function onBlockRowClick(raw: TableData) {
    const rec = raw as unknown as BlockRow;
    // 状态串形如 minecraft:oak_stairs[facing=east,half=top]
    const m = /^([^[\]]+)\[([^\]]*)\]\s*$/.exec(rec.state ?? "");
    const blockName = m ? m[1] : rec.state;
    activeBlock.value = { name: blockName };
    replaceForm.value.fromName = blockName;
    const single = rec.forms.length === 1 && rec.stateCount === 1;
    replaceForm.value.fromPropsText = single && m ? m[2] : "";
    if (rec.forms.length > 1) {
      const others = rec.forms.filter((f) => f !== blockName);
      Message.warning(
        t("replace.rowMergedForms", {
          name: blockName,
          forms: rec.forms.length,
          others: others.join(t("common.listSeparator")),
        }),
      );
      return;
    }
    Message.info(
      t("replace.setSource", { name: blockName }) + (single ? "" : t("replace.rowMultiState")),
    );
  }

  async function openReplace() {
    replaceOpen.value = true;
    replacePreview.value = null;
    blockPickQuery.value = "";
    blockHits.value = [];
    // 列表默认填「替换目标」：替换时更常选的是「换成什么」，
    // 而源方块多半是从「方块」页签点行带过来的
    replacePickField.value = "to";
    // 名字缓存每次打开都作废：方块库可能在本程序运行期间被换掉
    // （到「版本库」里补齐或重建版本后，后端每条命令都会重新读库），
    // 留着旧结果会让明明能查到的方块一直显示「未找到」。
    for (const timer of blockNameTimers.values()) clearTimeout(timer);
    blockNameTimers.clear();
    blockNames.value = {};
    blockNamesQueried.value = new Set();
    // 源方块默认取当前选中的方块，省去用户重打一遍
    if (!replaceForm.value.fromName && activeBlock.value) {
      replaceForm.value.fromName = activeBlock.value.name;
    }
    // 本次打开时就查一次名字：值可能来自上次的填写而没有发生变化，
    // 只靠 watch 不会触发，那一栏就会空着
    void lookupBlockName(replaceForm.value.fromName);
    void lookupBlockName(replaceForm.value.toName);
    if (blockVersions.value.length === 0) {
      try {
        blockVersions.value = await listBlockVersions();
      } catch (e) {
        Message.error(t("replace.loadVersionsFailed", { error: String(e) }));
      }
    }
  }

  async function doPreview() {
    const cur = s.selected.value;
    if (!cur) return;
    const rule = currentRule();
    if (!rule.fromName || !rule.toName) {
      Message.warning(t("replace.emptyFromTo"));
      return;
    }
    // 记下这次预览的序号：等待期间用户可能改了规则（那时结果已经不作数），
    // 也可能连点了两次，早先的结果不能盖住后来的
    const seq = ++previewSeq;
    replaceBusy.value = true;
    try {
      const r = await previewReplace(cur.path, rule);
      if (seq !== previewSeq) return;
      replacePreview.value = r.replaced;
    } catch (e) {
      if (seq !== previewSeq) return;
      Message.error(t("replace.previewFailed", { error: String(e) }));
    } finally {
      if (seq === previewSeq) replaceBusy.value = false;
    }
  }

  /** 把当前规则记入草稿，等用户点「保存」时才真正写进文件。
   *
   * 不在这里直接写盘：方块替换会大量改写调色板，一旦执行就不可撤销。
   * 让它与其它改动一起进入未保存状态后，用户可以先看一眼再决定存不存，
   * 关窗口或切文件时也会有明确的提醒。
   */
  async function doReplace() {
    const cur = s.selected.value;
    if (!cur) return;
    const rule = currentRule();
    if (!rule.fromName || !rule.toName) {
      Message.warning(t("replace.emptyFromTo"));
      return;
    }
    s.stageReplace(rule);
    Message.success(t("replace.staged"));
    replaceOpen.value = false;
  }

  return {
    activeBlock,
    replaceOpen,
    replaceBusy,
    blockPickQuery,
    blockHits,
    blockSearching,
    blockVersions,
    replacePickField,
    replaceForm,
    replacePreview,
    blockNameOf,
    blockNameQueried,
    onBlockSearch,
    onBlockRowClick,
    setPickField,
    pickBlock,
    openReplace,
    doPreview,
    doReplace,
  };
}

export type BlockReplaceSlice = ReturnType<typeof createBlockReplaceSlice>;
