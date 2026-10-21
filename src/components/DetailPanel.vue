/**
 * 详情面板：3D 预览（含视角/亮度/导出工具条）、可编辑元数据表单、
 * 「信息 / 方块 / 实体」三个页签。
 *
 * 它是右侧整块的容器；方块与实体两个页签的表格各自独立成
 * BlockStatTable / EntityTable，这里只留页签骨架。
 * 状态在 store/scenePreview.ts 与 store/fileList.ts（元数据草稿）。
 */
<script setup lang="ts">
import SceneViewer from "./SceneViewer.vue";
import BlockStatTable from "./BlockStatTable.vue";
import EntityTable from "./EntityTable.vue";
import { EXPORT_SIZES } from "../render/scene3d";
import {
  PREVIEW_MAX_H,
  PREVIEW_MAX_W,
  PREVIEW_MIN_H,
  PREVIEW_MIN_W,
} from "../composables/usePreviewResize";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { displayName } from "../i18n";
import { useAppStore } from "../store";
import { formatDims, formatSize } from "../utils/format";

const { t } = useI18n();

const {
  previewHeight,
  previewWidth,
  previewStageHeight,
  measurePreviewRowHeight,
  previewHorizResizing,
  previewVertResizing,
  startPreviewHeightResize,
  startPreviewWidthResize,
  togglePreviewHeight,
  togglePreviewWidth,
  onPreviewHeightKey,
  onPreviewWidthKey,
  viewer,
  scene,
  sceneLoading,
  sceneError,
  sceneInfo,
  sceneGate,
  sceneBlockLimit,
  brightness,
  activeView,
  exportSize,
  exporting,
  exportingProgress,
  setView,
  startRender,
  exportPreview,
  exportBatch,
  selected,
  draft,
  dirty,
  saving,
  resetDraft,
  saveDraft,
  checkedCount,
  hasChecked,
  openConvert,
} = useAppStore();

/** 预览行的 DOM，用于实测它拿到的高度 */
const previewRowEl = ref<HTMLElement | null>(null);
let rowObserver: ResizeObserver | null = null;

onMounted(() => {
  // 舞台尺寸必须按实测高度算：控制条是 flex-wrap 的，预览区窄到放不下
  // 那一排控件时会折行，固定常数就偏了（多算的高度会把舞台顶出面板）。
  const el = previewRowEl.value;
  if (!el) return;
  rowObserver = new ResizeObserver(() => {
    measurePreviewRowHeight(el.getBoundingClientRect().height);
  });
  rowObserver.observe(el);
  measurePreviewRowHeight(el.getBoundingClientRect().height);
});

onBeforeUnmount(() => {
  rowObserver?.disconnect();
  rowObserver = null;
});

</script>

<template>
  <div
    class="detail"
    :class="{
      'is-resizing-h': previewHorizResizing,
      'is-resizing-v': previewVertResizing,
    }"
  >
    <div class="preview" :style="{ height: previewHeight + 'px' }">
      <!-- 预览与信息并排：左边是 3D 视图，右边是该投影的基本信息与元数据。
           用横向 flex 让「信息在预览右侧」这条要求不随窗口宽度漂移。 -->
      <div ref="previewRowEl" class="preview__main">
        <div
          class="preview__stage"
          :style="{ width: previewWidth + 'px', height: previewStageHeight + 'px' }"
        >
          <!-- 方块数超过阈值：默认不生成几何，等用户点「开始渲染」。
               三万块以上的场景建几何并上传显存会明显卡顿，何时做由用户定。 -->
          <div v-if="sceneGate" class="scene-gate">
            <div class="scene-gate__text">
              {{
                t("detail.gateWarning", {
                  name:
                    displayName({ zhName: sceneGate.zhName, enName: sceneGate.enName }) ||
                    sceneGate.name,
                  count: sceneGate.count.toLocaleString(),
                  limit: sceneBlockLimit.toLocaleString(),
                })
              }}
            </div>
            <a-button type="primary" size="small" @click="startRender">
              {{ t("detail.startRender") }}
            </a-button>
          </div>
          <a-spin v-else :loading="sceneLoading" style="width: 100%; height: 100%">
            <SceneViewer
              ref="viewer"
              :scene="scene"
              :brightness="brightness"
            />
          </a-spin>
        </div>

        <!-- 舞台与信息列之间的垂直分隔条：拖动它改舞台宽度，
             信息列吃掉剩余宽度，两者合起来占满预览行、右侧不留空白。
             只在有信息列时出现——没有可分隔的对象时留一条空条没有意义。 -->
        <div
          v-if="selected"
          class="splitter"
          role="separator"
          aria-orientation="vertical"
          :aria-label="t('detail.previewWidthSplitterAria')"
          :aria-valuenow="previewWidth"
          :aria-valuemin="PREVIEW_MIN_W"
          :aria-valuemax="PREVIEW_MAX_W"
          tabindex="0"
          :title="t('detail.previewWidthSplitterTitle')"
          @pointerdown="startPreviewWidthResize"
          @dblclick="togglePreviewWidth"
          @keydown="onPreviewWidthKey"
        />

        <div class="preview__info" v-if="selected">
          <!-- 可编辑区：文件名、投影名称、作者、描述。操作行在它外面，
               因此内容再长也不会把保存按钮挤出可视范围。
               这一段自己用 Arco 无轨道滚动条滚（见 .preview__info-body 的尺寸）。 -->
          <div class="preview__info-body">
          <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
          <!-- 这一栏只放四件事：文件名、投影名称、作者、描述。四者都可编辑。
               前三项按用户要求「名称在左、控件在右」横排；描述是长文本，
               标签留在上方、输入框占满整行。
               制作版本/数据版本/投影格式/尺寸/区域数量/方块总数/实体数量
               都已移到底部的「信息」页签，这里不再重复显示。 -->
          <!-- 文件名可改：改的是磁盘上的文件名，保存时会连同备份一起
               重新链接到新名字（见后端 rename_schematic）。
               输入框只放文件名本身，不含所在目录。 -->
          <div class="preview__row">
            <span class="preview__row-label">{{ t("detail.fileName") }}</span>
            <a-input
              class="preview__row-control"
              v-model="draft.fileName"
              :disabled="!!selected.error"
              @input="dirty = true"
            />
          </div>
          <div class="hint-dim" style="margin: -6px 0 10px 66px">
            {{ t("detail.renameNote") }}
          </div>

          <a-form :model="draft" layout="vertical" size="small" class="preview__form">
            <div class="preview__row">
              <span class="preview__row-label">{{ t("detail.schematicName") }}</span>
              <a-input
                class="preview__row-control"
                v-model="draft.name"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </div>
            <div class="preview__row">
              <span class="preview__row-label">{{ t("detail.author") }}</span>
              <a-input
                class="preview__row-control"
                v-model="draft.author"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </div>
            <a-form-item :label="t('detail.description')">
              <a-textarea
                v-model="draft.description"
                :auto-size="{ minRows: 6, maxRows: 14 }"
                :placeholder="t('detail.descriptionPlaceholder')"
                :disabled="!!selected.error"
                @input="dirty = true"
              />
            </a-form-item>
            </a-form>
          </a-scrollbar>
          </div>
          <!-- 操作行钉在面板底部，不随内容一起滚。
               面板高度由用户拖动决定，不钉住的话保存按钮会落到折叠线以下，
               用户得先滚动才能保存，等于按钮藏起来了。 -->
          <div class="preview__actions">
            <!-- 版本转换放在投影信息区：它读的是当前选中投影的信息再另存一份 -->
            <a-button size="small" @click="openConvert">
              {{ t("detail.convert") }}
            </a-button>
            <a-button
              type="primary"
              size="small"
              :loading="saving"
              :disabled="!dirty || !!selected.error"
              @click="saveDraft"
            >
              {{ t("detail.save") }}
            </a-button>
            <a-button size="small" :disabled="!dirty" @click="resetDraft(selected)">
              {{ t("detail.undo") }}
            </a-button>
            <span v-if="dirty" class="hint-warn">{{ t("detail.unsaved") }}</span>
            <span v-else-if="!selected.error" class="hint-dim">
              {{ t("detail.backupNote") }}
            </span>
          </div>
        </div>
        <div class="preview__info" v-else>
          <a-empty :description="t('detail.empty')" />
        </div>
      </div>
      <div class="preview__bar">
        <a-radio-group
          v-model="activeView"
          type="button"
          size="small"
          :disabled="!scene"
          @change="setView(String(activeView))"
        >
          <a-radio value="front">{{ t("detail.viewFront") }}</a-radio>
          <a-radio value="left45">{{ t("detail.viewLeft45") }}</a-radio>
          <a-radio value="right45">{{ t("detail.viewRight45") }}</a-radio>
          <a-radio value="back">{{ t("detail.viewBack") }}</a-radio>
        </a-radio-group>
        <span class="preview__label">{{ t("detail.brightness") }}</span>
        <a-slider
          v-model="brightness"
          :min="0.5"
          :max="2"
          :step="0.05"
          :style="{ width: '140px' }"
        />
        <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
        <!-- 导出尺寸：四档可选，单个与批量共用。
             选项来自 EXPORT_SIZES，避免与 scene3d.ts 的定义各写一份而漂移 -->
        <a-select v-model="exportSize" size="small" style="width: 104px">
          <a-option v-for="s in EXPORT_SIZES" :key="s" :value="s">{{ s }}</a-option>
        </a-select>
        <a-button
          size="small"
          type="primary"
          :loading="exporting && !exportingProgress"
          :disabled="!scene || exporting"
          @click="exportPreview"
        >
          {{ t("detail.exportPng") }}
        </a-button>
        <!-- 批量导出：对目录栏里勾选的文件各导出一张，输出到所选目录 -->
        <a-button
          size="small"
          :loading="exporting && !!exportingProgress"
          :disabled="!hasChecked || exporting"
          :title="
            hasChecked
              ? t('detail.batchExportHint', { count: checkedCount })
              : t('detail.batchExportEmpty')
          "
          @click="exportBatch"
        >
          {{ t("detail.exportBatch") }}
          <template v-if="exportingProgress">
            {{ t("detail.batchExportProgress", { done: exportingProgress.done, total: exportingProgress.total }) }}
          </template>
          <template v-else-if="hasChecked">
            {{ t("detail.batchExportCount", { count: checkedCount }) }}
          </template>
        </a-button>
        <span class="preview__spacer" />
        <span v-if="sceneError" class="preview__error">{{ sceneError }}</span>
        <span v-else-if="sceneInfo" class="preview__info">{{ sceneInfo }}</span>
      </div>
    </div>

    <!-- 预览区与页签区之间的水平分隔条：往上拖让预览占更多高度，
         往下拖把空间让给页签区。它只改高度，舞台宽度由上面那条垂直分隔条控制。 -->
    <div
      class="splitter splitter--horizontal"
      role="separator"
      aria-orientation="horizontal"
      :aria-label="t('detail.previewSplitterAria')"
      :aria-valuenow="previewHeight"
      :aria-valuemin="PREVIEW_MIN_H"
      :aria-valuemax="PREVIEW_MAX_H"
      tabindex="0"
      :title="t('detail.previewSplitterTitle')"
      @pointerdown="startPreviewHeightResize"
      @dblclick="togglePreviewHeight"
      @keydown="onPreviewHeightKey"
    />

    <a-tabs
      class="detail__tabs"
      default-active-key="info"
      style="padding: 0 8px"
    >
      <a-tab-pane key="info" :title="t('detail.tabInfo')">
        <!-- 只读明细。可编辑的元数据（文件名/投影名称/作者/描述）在预览区
             右侧，这里集中放**其余**的详细信息，避免两处重复同一字段。
             制作版本与数据版本分成两项：用户明确列了这两项，
             合并成「1.20.1（数据版本 3465）」会少一个可扫的标签。
             明细比页签高时用 Arco 无轨道滚动条滚。 -->
        <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
        <a-descriptions
          v-if="selected"
          :column="1"
          size="small"
          :label-style="{ width: '110px' }"
          bordered
        >
          <a-descriptions-item :label="t('detail.mcVersion')">
            {{ selected.mcVersionName || t("detail.unknown") }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.dataVersion')">
            {{ selected.dataVersion }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.formatVersion')">
            v{{ selected.formatVersion }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.dims')">
            {{ formatDims(selected.enclosing) }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.regionCount')">
            {{ selected.regionCount }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.totalBlocks')">
            {{ selected.totalBlocks }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.entityCount')">
            {{ selected.entityCount }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.tileEntityCount')">
            {{ selected.tileEntityCount }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.pendingTicks')">
            {{ selected.pendingTicks }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.hasPreview')">
            {{ selected.hasPreview ? t("detail.yes") : t("detail.no") }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.fileSize')">
            {{ formatSize(selected.fileSize) }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('detail.dirName')">
            {{ selected.dirName }}
          </a-descriptions-item>
          <a-descriptions-item v-if="selected.error" :label="t('detail.parseError')">
            <span style="color: rgb(var(--danger-6))">{{ selected.error }}</span>
          </a-descriptions-item>
        </a-descriptions>
        <a-empty v-else :description="t('detail.empty')" />
        </a-scrollbar>

      </a-tab-pane>

      <a-tab-pane key="blocks" :title="t('detail.tabBlocks')">
        <BlockStatTable />
      </a-tab-pane>

      <a-tab-pane key="entities" :title="t('detail.tabEntities')">
        <EntityTable />
      </a-tab-pane>
    </a-tabs>
  </div>
</template>
