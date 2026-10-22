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
import ExportDialog from "./ExportDialog.vue";
import ViewPresetDialog from "./ViewPresetDialog.vue";
import { PRESET_MENU } from "../render/scene3d";
import {
  PREVIEW_MAX_H,
  PREVIEW_MAX_W,
  PREVIEW_MIN_H,
  PREVIEW_MIN_W,
} from "../composables/usePreviewResize";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Message } from "@arco-design/web-vue";
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
  exportOpen,
  exportBatchMode,
  exporting,
  exportingProgress,
  setView,
  startRender,
  exportPreview,
  selected,
  draft,
  dirty,
  dirtyMeta,
  saving,
  resetDraft,
  saveDraft,
  saveAs,
  checkedCount,
  hasChecked,
  openConvert,
  viewMenu,
  savedViews,
  viewPresetOpen,
  applySavedViewByName,
  removeSavedView,
  orthoView,
  setOrthoView,
  openViewMenu,
  closeViewMenu,
} = useAppStore();

/** 预览行的 DOM，用于实测它拿到的高度 */
const previewRowEl = ref<HTMLElement | null>(null);
let rowObserver: ResizeObserver | null = null;

/** 视角右键菜单的尺寸，与 global.scss 里 `.view-menu` / `&__submenu` 的
 *  min-width 与 max-height 保持一致；改样式时必须同步这里，否则翻转定位会算偏。 */
const VIEW_MENU_W = 168;
const VIEW_MENU_H = 340;
const VIEW_SUB_W = 150;
const VIEW_SUB_H = 340;

/** 在预览舞台上右键：打开视角菜单。
 *
 * 菜单靠下/靠右时向内翻，避免被窗口边缘裁掉。翻转后还要夹一下上/左边界：
 * 在窗口左上角附近右键时，`clientY - MENU_H` 会算出负值，菜单顶部跑到窗口外。
 */
function onStageContextMenu(e: MouseEvent) {
  e.preventDefault();
  const rawX = e.clientX + VIEW_MENU_W > window.innerWidth ? e.clientX - VIEW_MENU_W : e.clientX;
  const rawY = e.clientY + VIEW_MENU_H > window.innerHeight ? e.clientY - VIEW_MENU_H : e.clientY;
  openViewMenu(Math.max(rawX, 4), Math.max(rawY, 4));
}

/** 点在菜单外面或按 Esc 时关闭 */
function onDocPointerDown(e: Event) {
  if (!viewMenu.value.open) return;
  const target = e.target as HTMLElement | null;
  if (target?.closest(".view-menu")) return;
  closeViewMenu();
}

function onDocKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") closeViewMenu();
}

/** 预设角度子菜单是否展开 */
const presetSubOpen = ref(false);

/** 子菜单位置：贴着主菜单右侧，右边放不下就翻到左侧 */
const submenuStyle = computed(() => {
  const right = viewMenu.value.x + VIEW_MENU_W + VIEW_SUB_W;
  const left =
    right > window.innerWidth
      ? Math.max(viewMenu.value.x - VIEW_SUB_W, 4)
      : viewMenu.value.x + VIEW_MENU_W - 4;
  // 上边界同时受「主菜单自身位置」与「窗口高度」约束：主菜单上翻过的话，
  // 子菜单跟着它的顶端走，否则会脱节飘到别处
  const limit = Math.max(window.innerHeight - VIEW_SUB_H - 4, 4);
  const top = Math.min(Math.max(viewMenu.value.y, 4), limit);
  return { left: left + "px", top: top + "px" };
});

// 菜单关掉时把子菜单也收起来，否则下次右键会带着上次的展开状态出现
watch(
  () => viewMenu.value.open,
  (open) => {
    if (!open) presetSubOpen.value = false;
  },
);

/** 从菜单里选一个预设角度：先关菜单再切视角，
 *  否则菜单会停在原地挡住刚换好的画面。 */
function pickViewFromMenu(key: string) {
  closeViewMenu();
  setView(key);
}

/** 应用一条自定义预设 */
function pickCustomPreset(name: string) {
  closeViewMenu();
  applySavedViewByName(name);
}

/** 删除一条自定义预设 */
function deletePreset(name: string) {
  removeSavedView(name);
  Message.success(t("view.deletePresetDone", { name }));
}

/** 打开「保存为预设」面板：先关菜单，否则两个浮层叠在一起 */
function openSavePreset() {
  closeViewMenu();
  viewPresetOpen.value = true;
}

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

  // 关闭视角菜单的两个入口：点别处、按 Esc。绑在 document 上而不是菜单上，
  // 否则点在画布上时事件根本到不了这里，菜单会一直挂着。
  document.addEventListener("pointerdown", onDocPointerDown, true);
  document.addEventListener("keydown", onDocKeydown);
});

onBeforeUnmount(() => {
  rowObserver?.disconnect();
  rowObserver = null;
  document.removeEventListener("pointerdown", onDocPointerDown, true);
  document.removeEventListener("keydown", onDocKeydown);
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
          @contextmenu="onStageContextMenu"
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
              @pick-view="setView"
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
              @input="dirtyMeta = true"
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
                @input="dirtyMeta = true"
              />
            </div>
            <div class="preview__row">
              <span class="preview__row-label">{{ t("detail.author") }}</span>
              <a-input
                class="preview__row-control"
                v-model="draft.author"
                :disabled="!!selected.error"
                @input="dirtyMeta = true"
              />
            </div>
            <a-form-item :label="t('detail.description')">
              <a-textarea
                v-model="draft.description"
                :auto-size="{ minRows: 6, maxRows: 14 }"
                :placeholder="t('detail.descriptionPlaceholder')"
                :disabled="!!selected.error"
                @input="dirtyMeta = true"
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
            <!-- 另存为：把当前内容连同未保存的元数据写到新文件，原文件不动 -->
            <a-button size="small" :disabled="!!selected.error" @click="saveAs">
              {{ t("files.saveAs") }}
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
        <!-- 视角切换改为画布里的 XYZ 方向标：它本身就是「当前朝向的读数」，
             比一排文字按钮更直观，也不占控制条。等距与轴向视角的完整清单
             在预览区的右键菜单里。 -->
        <span class="preview__label">{{ t("detail.brightness") }}</span>
        <!-- 关掉拖动手柄上的数值气泡：右侧已有常驻百分比，气泡是同一信息的第二份，
             拖动时还会挡住滑杆本身 -->
        <a-slider
          v-model="brightness"
          :min="0.5"
          :max="2"
          :step="0.05"
          :show-tooltip="false"
          :style="{ width: '140px' }"
        />
        <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
        <!-- 尺寸改在导出面板里选：系统保存框放不下自定义控件，
             而尺寸与「存到哪」本来就是两件事 -->
        <a-button
          size="small"
          type="primary"
          :loading="exporting && !exportingProgress"
          :disabled="!scene || exporting"
          @click="exportBatchMode = false; exportOpen = true"
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
          @click="exportBatchMode = true; exportOpen = true"
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

    <!-- 导出尺寸面板：点「导出预览图」先在这里定尺寸，确认后才走系统保存框 -->
    <ExportDialog />

    <!-- 预览区右键菜单：保存为预设、投影方式、预设角度子菜单。
         定位用 fixed，跟着鼠标走；点击别处或按 Esc 关闭（见 onDocPointerDown）。 -->
    <div
      v-if="viewMenu.open"
      class="view-menu ctx-menu"
      :style="{ left: viewMenu.x + 'px', top: viewMenu.y + 'px' }"
      role="menu"
      @contextmenu.prevent
    >
      <div
        class="ctx-menu__item"
        role="menuitem"
        tabindex="0"
        @click="openSavePreset"
        @keydown.enter.prevent="openSavePreset"
        @keydown.space.prevent="openSavePreset"
      >
        {{ t("view.menuSavePreset") }}
      </div>

      <div
        class="ctx-menu__item"
        role="menuitemcheckbox"
        tabindex="0"
        :aria-checked="orthoView"
        @click="setOrthoView(!orthoView)"
        @keydown.enter.prevent="setOrthoView(!orthoView)"
        @keydown.space.prevent="setOrthoView(!orthoView)"
      >
        <span class="view-menu__check">{{ orthoView ? "✓" : "" }}</span>
        {{ t("view.menuOrtho") }}
      </div>

      <!-- 预设角度：悬停或点击展开子菜单。十个项目太多，平铺会把主菜单
           撑得很长，且 issue 指定的就是子菜单形态。
           自定义预设也归进这个子菜单：它们同样是「按角度切视角」，
           和内置的十个放在一起才找得到，分到主菜单上是两类东西混排。 -->
      <div
        class="ctx-menu__item view-menu__sub"
        role="menuitem"
        tabindex="0"
        aria-haspopup="true"
        :aria-expanded="presetSubOpen"
        @mouseenter="presetSubOpen = true"
        @click.stop="presetSubOpen = !presetSubOpen"
        @keydown.enter.prevent="presetSubOpen = true"
        @keydown.space.prevent="presetSubOpen = true"
        @keydown.right.prevent="presetSubOpen = true"
      >
        <span class="view-menu__sub-label">{{ t("view.menuPresets") }}</span>
        <span class="view-menu__arrow">▸</span>
      </div>
      <div
        v-if="presetSubOpen"
        class="view-menu__submenu ctx-menu"
        :style="submenuStyle"
        role="menu"
        @mouseenter="presetSubOpen = true"
      >
        <div
          v-for="item in PRESET_MENU"
          :key="item.key"
          class="ctx-menu__item"
          role="menuitem"
          tabindex="0"
          @click="pickViewFromMenu(item.key)"
          @keydown.enter.prevent="pickViewFromMenu(item.key)"
          @keydown.space.prevent="pickViewFromMenu(item.key)"
        >
          {{ t(item.labelKey) }}
        </div>

        <!-- 自定义预设：存在时接在十个内置预设后面，每项带删除入口 -->
        <template v-if="savedViews.length > 0">
          <div class="ctx-menu__sep" role="none" />
          <div class="ctx-menu__label" role="none">
            {{ t("view.menuCustomPresets") }}
          </div>
          <div
            v-for="preset in savedViews"
            :key="preset.name"
            class="ctx-menu__item view-menu__custom"
            role="menuitem"
            tabindex="0"
            @click="pickCustomPreset(preset.name)"
            @keydown.enter.prevent="pickCustomPreset(preset.name)"
            @keydown.space.prevent="pickCustomPreset(preset.name)"
          >
            <span class="view-menu__name">{{ preset.name }}</span>
            <a
              class="view-menu__del"
              role="button"
              tabindex="0"
              :title="t('view.menuDeletePreset')"
              @click.stop="deletePreset(preset.name)"
              @keydown.enter.prevent.stop="deletePreset(preset.name)"
              @keydown.space.prevent.stop="deletePreset(preset.name)"
              >×</a
            >
          </div>
        </template>
      </div>
    </div>

    <!-- 保存为预设角度的面板 -->
    <ViewPresetDialog />
  </div>
</template>
