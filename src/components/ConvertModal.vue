/**
 * 版本转换弹窗。
 *
 * 向导式流程：选目标版本 → 分析差异 → 指定替代品 → 执行转换。
 * 状态与流程都在 store/versionConvert.ts，这里只做展示与触发。
 *
 * 布局分左右两栏：左栏是要处理的方块清单，右栏是方块列表（点选即指定）。
 * 用户必须同时看到「缺了哪些方块」与「有哪些方块可选」，挤在一个窄弹窗里
 * 就只能来回滚动，因此弹窗明显加宽、两栏并排。右栏用 Arco 的无轨道滚动条。
 *
 * 凡是出现方块 ID 的地方都一并给出显示名称：只给 ID 时用户无法确认自己
 * 要替换的到底是哪一个方块。名字分中英两套（后端从 Minecraft 语言表取出，
 * 见 dto.rs），按当前界面语言取一套。
 */
<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import type { TableData } from "@arco-design/web-vue";
import { Message } from "@arco-design/web-vue";
import { displayName, isFallbackName } from "../i18n";
import { useAppStore } from "../store";
import { useListKeyboard } from "../composables/useListKeyboard";
import type { ConvertItem } from "../types";

const { t } = useI18n();

const {
  selected,
  convOpen,
  convBusy,
  convTargets,
  convTargetIdx,
  convItems,
  convAnalyzed,
  convColumns,
  convActiveItem,
  convPickQuery,
  convPickHits,
  convPickBusy,
  convReplacementOf,
  convIsAuto,
  convReplState,
  convHitInTarget,
  convNameOf,
  convNameQueried,
  selectConvRow,
  onConvPickSearch,
  pickConvBlock,
  scheduleConvNameLookup,
  reloadConvTargets,
  selectConvTarget,
  runConvertAnalyze,
  doConvert,
  // 版本库域：已支持但尚未加载的版本也要列出来，点一下现场补齐
  supported,
  ready,
  progress,
  running,
  prepareVersion,
} = useAppStore();

/** 目标版本下拉的候选项。
 *
 * 已加载的版本（后端给了下标，可直接用来转换）排在最前；其余支持的版本
 * 以灰字列在后面，点一下就下载/构建，补完再选它。两者分开是必要的：
 * 只有加载过的版本有下标，没加载的连不上后端。
 */
interface TargetOption {
  /** 已加载版本在方块库里的下标；未加载的没有 */
  index: number | null;
  name: string;
  dataVersion: number;
  /** 是否已经加载，可以直接转换 */
  loaded: boolean;
  /** 该版本是否正在准备中（下载或构建） */
  preparing: boolean;
  /** 准备进度说明；未在准备时为空 */
  progressText: string;
}

/** 正在准备中的版本名。
 *
 * 补库过程是异步的，按钮上的转圈与进度文字都靠它；
 * 与 running 配合：整批补库在跑时不允许重复点。
 */
const preparingName = ref("");
let prepareSequence = 0;

/** 左栏表格的当前页（从 1 起）。
 *
 * 由这里管而不是交给 Arco 内部：方向键导航要用翻页键切页，
 * 就必须能读到当前页、也能主动改它。 */
const convPage = ref(1);
const CONV_PAGE_SIZE = 12;
/** 当前页可见的行。键盘导航的「行数」与「下标」都按这一份算。 */
const convPageRows = computed(() => {
  const start = (convPage.value - 1) * CONV_PAGE_SIZE;
  return convItems.value.slice(start, start + CONV_PAGE_SIZE);
});
/** 键盘导航的挂载点 */
const convTableHost = ref<HTMLElement | null>(null);

/** 当前选中项在本页里的下标 */
function convActiveIndex(): number {
  const cur = convActiveItem.value;
  if (!cur) return -1;
  return convPageRows.value.findIndex((r) => r.name === cur.name);
}

useListKeyboard({
  host: convTableHost,
  rowCount: () => convPageRows.value.length,
  activeIndex: convActiveIndex,
  onSelect: (i) => {
    const target = convPageRows.value[i];
    if (target) selectConvRow(target);
  },
  paged: () => true,
  page: () => convPage.value,
  pageCount: () => Math.max(1, Math.ceil(convItems.value.length / CONV_PAGE_SIZE)),
  onPage: (p) => {
    convPage.value = p;
  },
});

const targetOptions = computed<TargetOption[]>(() => {
  const loaded: TargetOption[] = convTargets.value.map((t) => ({
    index: t.index,
    name: t.name,
    dataVersion: t.dataVersion,
    loaded: true,
    preparing: false,
    progressText: "",
  }));
  const loadedNames = new Set(loaded.map((o) => o.name));
  // 投影自己的版本与已加载的版本不重复列出：前者转过去没有意义，
  // 后者已经在上面那一组里
  const sourceVersion = selected.value?.dataVersion;
  const pending: TargetOption[] = supported.value
    .filter(
      (v) => !loadedNames.has(v.name) && !ready.value.some((r) => r.name === v.name),
    )
    .filter((v) => v.dataVersion !== sourceVersion)
    .map((v) => {
      const p = progress.value.get(v.name);
      return {
        index: null,
        name: v.name,
        dataVersion: v.dataVersion,
        loaded: false,
        preparing: preparingName.value === v.name,
        progressText: describeProgress(v.name, p),
      };
    });
  return [...loaded, ...pending];
});

/** 把进度事件的阶段与百分比说成人话 */
function describeProgress(
  name: string,
  p: { stage: string; received: number; total: number; error: string } | undefined,
): string {
  if (preparingName.value !== name || !p) return "";
  if (p.error) return t("convert.progressFailed", { error: p.error });
  if (p.stage === "download") {
    const pct = p.total > 0 ? Math.floor((p.received / p.total) * 100) : 0;
    return t("convert.progressDownloading", { pct });
  }
  return t("convert.progressBuilding");
}

/** 选中目标版本。
 *
 * 未加载的版本用一个 `pending:<版本名>` 前缀当值：选中它并不是「选定」，
 * 而是「开始准备这个版本」，因此这里不动已选中的目标版本，只发起补库。
 * 用下拉的 change 而不是给选项挂点击，是因为禁用项收不到点击事件，
 * 而禁用态又正是没下标可选的未加载版本该有的表现。
 */
function onTargetChange(v: unknown) {
  if (typeof v === "string" && v.startsWith("pending:")) {
    const opt = targetOptions.value.find((o) => `pending:${o.name}` === v);
    if (opt) void onPrepareVersion(opt);
    return;
  }
  selectConvTarget(typeof v === "number" ? v : undefined);
}

/** 点未加载的版本：现场下载/构建它，补完自动选中 */
async function onPrepareVersion(opt: TargetOption) {
  if (running.value || opt.loaded) return;
  const sourcePath = selected.value?.path;
  const sequence = ++prepareSequence;
  preparingName.value = opt.name;
  try {
    const ok = await prepareVersion(opt.name);
    if (!convOpen.value || selected.value?.path !== sourcePath) return;
    if (!ok) {
      Message.error(t("convert.readyFailed", { name: opt.name }));
      return;
    }
    // 重新取一次列表：新版本在方块库里的下标只有后端知道
    if (!(await reloadConvTargets())) return;
    if (!convOpen.value || selected.value?.path !== sourcePath) return;
    const hit = convTargets.value.find((t) => t.name === opt.name);
    if (hit) {
      selectConvTarget(hit.index);
      Message.success(t("convert.readySelected", { name: opt.name }));
    } else {
      Message.success(t("convert.readyOther", { name: opt.name }));
    }
  } finally {
    if (sequence === prepareSequence) preparingName.value = "";
  }
}

/** 行只标「当前正在编辑」这一件事。
 *
 * 替代品的状态色不在这里染整行，也不染整格，而是只给那一格里的输入框上色
 * （见样式里的 .conv-block--todo / --auto）：整行或整格上色会把表格刷成
 * 一片绿红，连输入框下面的名字与 ID 也泡在色块里，反而看不出重点。
 */
function convRowClass(raw: TableData): string {
  const rec = raw as unknown as ConvertItem;
  return convActiveItem.value?.name === rec.name ? "conv-row--active" : "";
}

function onConvRowClick(raw: TableData) {
  selectConvRow(raw as unknown as ConvertItem);
}

/** 源方块的显示名称；分析结果自带两套名字，缺哪套退另一套 */
function sourceName(rec: ConvertItem): string {
  return displayName({ zhName: rec.nameZh, enName: rec.nameEn }) || rec.name;
}

/** 这一项缺当前语言那一套名字（显示名称是退回另一种语言来的） */
function nameIsFallback(rec: ConvertItem): boolean {
  return isFallbackName({ zhName: rec.nameZh, enName: rec.nameEn });
}

/** 替代品那一栏要显示的显示名称。
 *
 * 三种情况必须分开：查到名字、查过但库里没有、还没查完。
 * 混在一起（都显示空白）时用户分不清是自己填错了 ID 还是界面没反应。 */
function replName(rec: ConvertItem): string {
  const id = convReplacementOf(rec);
  if (!id) return "";
  const name = convNameOf(id);
  if (name) return name;
  return convNameQueried(id) ? t("convert.replNotFound") : t("convert.replQuerying");
}
</script>

<template>
  <a-modal
    v-model:visible="convOpen"
    :title="t('convert.title')"
    :width="1180"
    :ok-loading="convBusy"
    :ok-text="t('convert.ok')"
    :ok-button-props="{ disabled: convTargetIdx === undefined || !selected || !convAnalyzed }"
    :body-style="{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }"
    @ok="doConvert"
    modal-class="conv-window"
  >
    <a-alert v-if="!selected" type="warning" style="margin-bottom: 12px">
      {{ t("convert.pickFile") }}
    </a-alert>

    <a-space style="margin-bottom: 10px">
      <span>{{ t("convert.targetVersion") }}</span>
      <a-select
        :model-value="convTargetIdx"
        :placeholder="t('convert.targetPlaceholder')"
        allow-search
        style="width: 300px"
        @change="onTargetChange"
      >
        <a-option
          v-for="opt in targetOptions"
          :key="opt.name"
          :value="opt.index ?? `pending:${opt.name}`"
          :class="{ 'conv-version--pending': !opt.loaded }"
        >
          <!-- 未加载的版本：选中它就等于「点一下开始准备」，由 change 处理；
               这里只负责把它画成灰字并标出进度 -->
          <span v-if="!opt.loaded" class="conv-version__pending">
            <a-spin v-if="opt.preparing" :size="12" />
            {{ t("convert.versionOption", { name: opt.name, dataVersion: opt.dataVersion }) }}
            <span class="conv-version__tag">
              {{ opt.progressText || t("convert.pendingTag") }}
            </span>
          </span>
          <span v-else>
            {{ t("convert.versionOption", { name: opt.name, dataVersion: opt.dataVersion }) }}
          </span>
        </a-option>
      </a-select>
      <a-button size="small" :loading="convBusy" @click="runConvertAnalyze">
        {{ t("convert.analyze") }}
      </a-button>
    </a-space>

    <div class="hint-dim" style="margin-bottom: 10px">{{ t("convert.hint") }}</div>

    <a-spin :loading="convBusy" style="width: 100%">
      <template v-if="convAnalyzed">
        <a-alert v-if="convItems.length === 0" type="success">
          {{ t("convert.allExist") }}
        </a-alert>

        <div v-else class="conv-split">
          <!-- 左栏：缺哪些方块。红=还没定下替代品，绿=已经有了（自动推荐或已指定） -->
          <div class="conv-split__pane conv-split__pane--main">
            <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
            <div ref="convTableHost" class="conv-table-host" tabindex="0">
            <a-table
              :columns="convColumns"
              :data="convItems"
              :pagination="{
                pageSize: CONV_PAGE_SIZE,
                current: convPage,
                total: convItems.length,
              }"
              :row-class="convRowClass"
              row-key="name"
              size="small"
              @row-click="onConvRowClick"
              @page-change="(p: number) => (convPage = p)"
            >
              <template #from="{ record }">
                <div class="conv-block">
                  <!-- 名字由后端随分析结果给出。当前语言缺这一项时退回另一种语言，
                       并标灰说明——与「方块」页签里的处理一致，否则同一个方块
                       在页签里标灰、在这里不标，用户无从判断 -->
                  <span
                    :class="{ 'hint-dim': nameIsFallback(record) }"
                    :title="nameIsFallback(record) ? t('displayName.missing') : undefined"
                  >
                    {{ sourceName(record) }}
                  </span>
                  <span class="conv-block__id">{{ record.name }}</span>
                </div>
              </template>

              <template #repl="{ record }">
                <!-- conv-block--todo / --auto 只给输入框上色（见样式里那两条
                     .arco-input-wrapper 规则）：红=还没定替代品，绿=自动识别
                     出来的推荐；用户自己选过或填过的行不上色 -->
                <div class="conv-block" :class="`conv-block--${convReplState(record)}`">
                  <a-input
                    v-model="record.chosen"
                    :placeholder="record.suggested || t('convert.replPlaceholder')"
                    size="small"
                    allow-clear
                    @input="scheduleConvNameLookup(record)"
                  />
                  <!-- 实际会用到的替代品：显示名称 + 方块 ID 都写成正文。
                       只靠输入框的 placeholder 显示自动推荐时，一旦聚焦或点开
                       就看不见了，用户无法确认转换时究竟会换成哪个方块。 -->
                  <span v-if="convReplacementOf(record)" class="conv-block__eff">
                    <span class="conv-block__zh">{{ replName(record) }}</span>
                    <span class="conv-block__id">{{ convReplacementOf(record) }}</span>
                  </span>
                </div>
              </template>

              <template #state="{ record }">
                <a-tag v-if="convIsAuto(record)" size="small" color="green">
                  {{ t("convert.stateAuto") }}
                </a-tag>
                <a-tag v-else-if="record.chosen" size="small" color="green">
                  {{ t("convert.stateChosen") }}
                </a-tag>
                <a-tag v-else size="small" color="red">{{ t("convert.stateTodo") }}</a-tag>
              </template>
            </a-table>
            </div>
            </a-scrollbar>
          </div>

          <!-- 右栏：方块列表，点选即指定给左栏当前行 -->
          <div class="conv-split__pane conv-split__pane--pick">
            <a-input-search
              v-model="convPickQuery"
              :placeholder="t('common.searchBlocks')"
              size="small"
              :loading="convPickBusy"
              allow-clear
              @input="onConvPickSearch"
              @search="onConvPickSearch"
              @clear="onConvPickSearch('')"
            />
            <div class="hint-dim" style="margin: 6px 0">
              <i18n-t scope="global" v-if="convActiveItem" keypath="convert.pickHintTarget" tag="span">
                <template #name>
                  <b>{{ sourceName(convActiveItem) }}</b>
                </template>
              </i18n-t>
              <template v-else>{{ t("convert.pickHintNoTarget") }}</template>
            </div>
            <a-scrollbar
              type="embed"
              class="conv-pick scroll-fill__inner"
              outer-class="scroll-fill"
            >
              <a-empty
                v-if="convPickHits.length === 0"
                :description="
                  convPickQuery.trim() ? t('common.noMatch') : t('common.typeToSearch')
                "
              />
              <button
                v-for="h in convPickHits"
                :key="h.name"
                type="button"
                class="conv-pick__item"
                :class="{
                  'is-current':
                    convActiveItem && convReplacementOf(convActiveItem) === h.name,
                  'is-other-version': !convHitInTarget(h),
                }"
                :title="
                  convHitInTarget(h) ? t('convert.pickTitle') : t('convert.pickTitleBlocked')
                "
                @click="pickConvBlock(h)"
              >
                <span class="conv-block__zh">
                  {{ displayName({ zhName: h.zh, enName: h.en }) || h.name }}
                  <span v-if="!convHitInTarget(h)" class="conv-pick__warn">
                    {{ t("convert.pickWarn") }}
                  </span>
                </span>
                <!-- 英文名与方块 ID 一并给出，显示名称在上方；
                     英文名与显示名称相同时不再重复（英语界面下本会重复一次） -->
                <span class="conv-block__id">
                  {{ h.name }}<template v-if="h.en && h.en !== h.zh"> · {{ h.en }}</template>
                </span>
              </button>
            </a-scrollbar>
          </div>
        </div>
      </template>
      <a-empty v-else :description="t('convert.empty')" />
    </a-spin>
  </a-modal>
</template>
