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
import RegionList from "./RegionList.vue";
import { PRESET_MENU } from "../render/scene3d";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Message } from "@arco-design/web-vue";
import { displayName } from "../i18n";
import { useAppStore } from "../store";
import { formatDims, formatSize } from "../utils/format";

const { t } = useI18n();

const {
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
  // 关闭视角菜单的两个入口：点别处、按 Esc。绑在 document 上而不是菜单上，
  // 否则点在画布上时事件根本到不了这里，菜单会一直挂着。
  document.addEventListener("pointerdown", onDocPointerDown, true);
  document.addEventListener("keydown", onDocKeydown);
});

onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocPointerDown, true);
  document.removeEventListener("keydown", onDocKeydown);
});

</script>

<template>
  <div class="detail editor-detail">
    <aside class="editor-detail__regions">
      <RegionList />
    </aside>

    <section class="editor-detail__preview">
      <div class="editor-preview__stage" @contextmenu="onStageContextMenu">
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
      <div class="preview__bar">
        <span class="preview__label">{{ t("detail.brightness") }}</span>
        <a-slider
          v-model="brightness"
          :min="0.5"
          :max="2"
          :step="0.05"
          :show-tooltip="false"
          :style="{ width: '140px' }"
        />
        <span class="preview__value">{{ Math.round(brightness * 100) }}%</span>
        <a-button
          size="small"
          type="primary"
          :loading="exporting && !exportingProgress"
          :disabled="!scene || exporting"
          @click="exportBatchMode = false; exportOpen = true"
        >
          {{ t("detail.exportPng") }}
        </a-button>
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
    </section>

    <aside class="editor-operations">
      <div class="editor-operations__meta">
        <div v-if="selected" class="preview__info">
          <div class="preview__info-body">
            <a-scrollbar type="embed" class="scroll-fill__inner" outer-class="scroll-fill">
              <div class="preview__row">
                <span class="preview__row-label">{{ t("detail.fileName") }}</span>
                <a-input
                  class="preview__row-control"
                  v-model="draft.fileName"
                  :disabled="!!selected.error || saving"
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
                    :disabled="!!selected.error || saving"
                    @input="dirtyMeta = true"
                  />
                </div>
                <div class="preview__row">
                  <span class="preview__row-label">{{ t("detail.author") }}</span>
                  <a-input
                    class="preview__row-control"
                    v-model="draft.author"
                    :disabled="!!selected.error || saving"
                    @input="dirtyMeta = true"
                  />
                </div>
                <a-form-item :label="t('detail.description')">
                  <a-textarea
                    v-model="draft.description"
                    :auto-size="{ minRows: 6, maxRows: 14 }"
                    :placeholder="t('detail.descriptionPlaceholder')"
                    :disabled="!!selected.error || saving"
                    @input="dirtyMeta = true"
                  />
                </a-form-item>
              </a-form>
            </a-scrollbar>
          </div>
          <div class="preview__actions">
            <a-button size="small" :disabled="saving" @click="openConvert">{{ t("detail.convert") }}</a-button>
            <a-button
              type="primary"
              size="small"
              :loading="saving"
              :disabled="!dirty || !!selected.error"
              @click="saveDraft"
            >
              {{ t("detail.save") }}
            </a-button>
            <a-button size="small" :disabled="!!selected.error || saving" @click="saveAs">
              {{ t("files.saveAs") }}
            </a-button>
            <a-button size="small" :disabled="!dirty || saving" @click="resetDraft(selected)">
              {{ t("detail.undo") }}
            </a-button>
            <span v-if="dirty" class="hint-warn">{{ t("detail.unsaved") }}</span>
            <span v-else-if="!selected.error" class="hint-dim">{{ t("detail.backupNote") }}</span>
          </div>
        </div>
        <div v-else class="preview__info">
          <a-empty :description="t('detail.empty')" />
        </div>
      </div>

      <a-tabs class="detail__tabs" default-active-key="info">
        <a-tab-pane key="info" :title="t('detail.tabInfo')">
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
    </aside>

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
