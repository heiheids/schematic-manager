/**
 * 导出预览图的设置面板：先在这里定尺寸，再走系统保存框选位置。
 *
 * 为什么不把尺寸放进系统保存框：Tauri 的 save() 只接受标题、过滤器、默认
 * 路径与「能否新建目录」四个参数，放不下自定义控件。系统原生对话框里能做
 * 到「选尺寸」的办法只有把它做成文件类型项，那看起来像选格式而不是选尺寸，
 * 因此改在这里先问清楚。
 *
 * 尺寸的两种给法互不冲突：点预设会把手填值覆盖掉，手填值则反过来取消预设的
 * 高亮——两者始终只有一份真值。
 */
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Message } from "@arco-design/web-vue";
import { EXPORT_ASPECTS, EXPORT_SHORT_EDGES } from "../render/scene3d";
import type { ExportAspect } from "../render/scene3d";
import { useAppStore } from "../store";

const { t } = useI18n();

const {
  exportOpen,
  exportBatchMode,
  exportSize,
  exportAspect,
  exportPreview,
  exportBatch,
  exporting,
} = useAppStore();

/** 面板里的宽高输入值。确认后才写回 store，取消则不改动默认设置。 */
const width = ref(1024);
const height = ref(1024);
/** 当前高亮的预设（aspect + shortEdge）；手填后清空 */
const pickedAspect = ref<ExportAspect | null>(null);
const pickedShortEdge = ref<number | null>(null);

/** 打开面板时用记住的设置填好初值 */
watch(exportOpen, (open) => {
  if (!open) return;
  pickedAspect.value = exportAspect.value;
  pickedShortEdge.value = exportSize.value;
  const preset = EXPORT_ASPECTS.find((a) => a.key === exportAspect.value);
  const ratio = preset ? preset.w / preset.h : 1;
  if (ratio >= 1) {
    height.value = exportSize.value;
    width.value = Math.round(exportSize.value * ratio);
  } else {
    width.value = exportSize.value;
    height.value = Math.round(exportSize.value / ratio);
  }
});

/** 宽高比的显示文本：「16x9」写成「16 : 9」。在 script 里拼好，
 *  模板里只做引用，避免属性表达式里塞字符串字面量引起解析歧义。 */
function aspectLabel(key: ExportAspect): string {
  return key.replace("x", " : ");
}

/** 从宽高反推当前是不是恰好等于某个预设，用于给按钮上高亮 */
const activePreset = computed(() => {
  if (!pickedAspect.value || !pickedShortEdge.value) return null;
  return `${pickedAspect.value}@${pickedShortEdge.value}`;
});

/** 点预设：按短边算出宽高，覆盖手填值 */
function applyPreset(aspect: ExportAspect, shortEdge: number) {
  const preset = EXPORT_ASPECTS.find((a) => a.key === aspect);
  if (!preset) return;
  const ratio = preset.w / preset.h;
  if (ratio >= 1) {
    height.value = shortEdge;
    width.value = Math.round(shortEdge * ratio);
  } else {
    width.value = shortEdge;
    height.value = Math.round(shortEdge / ratio);
  }
  pickedAspect.value = aspect;
  pickedShortEdge.value = shortEdge;
}

/** 手填：一旦动了宽高，预设高亮就失效——两者不能同时「生效」 */
function onManualEdit() {
  pickedAspect.value = null;
  pickedShortEdge.value = null;
}

/** 宽高的合法范围。与导出面板显示的能力一致：过小没有意义，过大浏览器画不出来。 */
const MIN_EDGE = 16;
const MAX_EDGE = 8192;

function clampEdge(v: number | undefined, fallback: number): number {
  if (typeof v !== "number" || Number.isNaN(v)) return fallback;
  return Math.min(MAX_EDGE, Math.max(MIN_EDGE, Math.round(v)));
}

async function confirm() {
  const w = clampEdge(width.value, 1024);
  const h = clampEdge(height.value, 1024);
  // 输入框本身已有 min/max，走到夹紧说明用户手敲了超范围的值；
  // 静默改掉会让人以为导出的是自己填的那个尺寸
  if (w !== width.value || h !== height.value) {
    Message.warning(t("export.clamped", { min: MIN_EDGE, max: MAX_EDGE }));
  }
  width.value = w;
  height.value = h;

  // 预设对应的短边要记下来，下次打开面板直接停在同一档；
  // 手填的尺寸没有「短边档位」可言，只能记住宽高本身。
  const ratio = w / h;
  const matched = EXPORT_ASPECTS.find((a) => Math.abs(a.w / a.h - ratio) < 0.01);
  if (matched) {
    exportAspect.value = matched.key;
    exportSize.value = Math.min(w, h);
  }

  const batch = exportBatchMode.value;
  exportOpen.value = false;
  if (batch) {
    await exportBatch({ w, h });
  } else {
    await exportPreview({ w, h });
  }
}
</script>

<template>
  <a-modal
    v-model:visible="exportOpen"
    :title="exportBatchMode ? t('export.dialogTitleBatch') : t('export.dialogTitle')"
    :ok-text="t('export.dialogConfirm')"
    :cancel-text="t('export.dialogCancel')"
    :ok-loading="exporting"
    :mask-closable="false"
    width="520px"
    @ok="confirm"
  >
    <div class="export-dialog">
      <div class="export-dialog__label">{{ t("export.presets") }}</div>
      <!-- 5 种宽高比 × 4 档短边：每行一种比例，行内四个短边档位。
           用 a-form-item 是为了让比例名有一个固定宽度的 label 位，
           五行左边缘因此对齐，而不是各自按文字长短错开。 -->
      <a-form :model="{}" layout="horizontal" size="small" class="export-dialog__presets">
        <div v-for="a in EXPORT_ASPECTS" :key="a.key" :data-aspect="a.key">
          <a-form-item :label="aspectLabel(a.key)">
            <a-space :size="6">
              <a-button
                v-for="edge in EXPORT_SHORT_EDGES"
                :key="edge"
                size="mini"
                :type="activePreset === `${a.key}@${edge}` ? 'primary' : 'secondary'"
                @click="applyPreset(a.key, edge)"
              >
                {{ edge }} px
              </a-button>
            </a-space>
          </a-form-item>
        </div>
      </a-form>

      <div class="export-dialog__label">{{ t("export.customSize") }}</div>
      <a-space :size="8" align="center">
        <a-input-number
          v-model="width"
          :min="MIN_EDGE"
          :max="MAX_EDGE"
          :precision="0"
          size="small"
          style="width: 120px"
          @change="onManualEdit"
        />
        <span>×</span>
        <a-input-number
          v-model="height"
          :min="MIN_EDGE"
          :max="MAX_EDGE"
          :precision="0"
          size="small"
          style="width: 120px"
          @change="onManualEdit"
        />
        <span class="hint-dim">{{ t("export.pixelUnit") }}</span>
      </a-space>
    </div>
  </a-modal>
</template>
