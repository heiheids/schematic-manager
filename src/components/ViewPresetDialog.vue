/**
 * 「保存为预设角度」面板。
 *
 * 字段按 issue 的要求给出：名称、投影方式、相机位置、相机角度，四项都可改。
 * 打开时以当前相机姿态为初值；改了位置或角度时，保存的是改后的值——这让面板
 * 既能「把看到的画面存下来」，也能顺手微调成想要的角度再存。
 *
 * 位置与角度是同一个姿态的两种写法：改其中一组，另一组跟着重算，
 * 免得存下一个自相矛盾的预设。项目里约定位置优先——角度由位置与目标点反解。
 */
<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Message } from "@arco-design/web-vue";
import * as THREE from "three";
import { useAppStore } from "../store";
import { anglesFromPosition, dirFromAngles } from "../render/scene3d";

const { t } = useI18n();

const { viewPresetOpen, viewer, orthoView, savedViews, saveCurrentView } = useAppStore();

/** 输入的名称 */
const name = ref("");
/** 编辑中的投影方式 */
const ortho = ref(false);
/** 编辑中的相机位置 */
const position = ref<[number, number, number]>([0, 0, 0]);
/** 编辑中的相机角度（度） */
const yaw = ref(0);
const pitch = ref(0);
/** 相机注视的目标点：不展示、不编辑，只用来在位置与角度之间换算 */
const target = ref<[number, number, number]>([0, 0, 0]);
/** 快照是否可用（3D 视图是否已就绪） */
const ready = ref(false);

/** 位置三元组的输入框模型：Arco 的 a-input-number 要求 number | undefined，
 *  这里用一份可写副本，保存时再收敛成确定的三元组。 */
type TripleModel = [number | undefined, number | undefined, number | undefined];
const posModel = ref<TripleModel>([0, 0, 0]);

/** 按当前 yaw / pitch 与距离反算位置，写回位置输入框。
 *
 * 距离沿用快照里的「相机到目标」长度：只改角度不该顺带改远近。
 */
function syncPositionFromAngles() {
  const dist = Math.hypot(
    position.value[0] - target.value[0],
    position.value[1] - target.value[1],
    position.value[2] - target.value[2],
  );
  // dirFromAngles 给的是「相机 → 目标」，位置要取反向
  const dir = dirFromAngles(yaw.value, pitch.value).multiplyScalar(-Math.max(dist, 1e-6));
  position.value = [
    target.value[0] + dir.x,
    target.value[1] + dir.y,
    target.value[2] + dir.z,
  ];
  posModel.value = [
    round3(position.value[0]),
    round3(position.value[1]),
    round3(position.value[2]),
  ];
}

/** 位置改了之后重算角度 */
function syncAnglesFromPosition() {
  const pos = readPosition();
  if (!pos) return;
  position.value = pos;
  const a = anglesFromPosition(
    new THREE.Vector3(...pos),
    new THREE.Vector3(...target.value),
  );
  yaw.value = round1(a.yaw);
  pitch.value = round1(a.pitch);
}

function round3(v: number) {
  return Math.round(v * 1000) / 1000;
}

function round1(v: number) {
  return Math.round(v * 10) / 10;
}

/** 从输入框读出位置三元组；有任一格留空或非数时返回 null。
 *
 * 面板拿到的是可写副本，用户可能把某格清空。此时不能拿旧值顶上——
 * 那等于「改了却没生效」，用户看不出发生了什么。
 */
function readPosition(): [number, number, number] | null {
  const [x, y, z] = posModel.value;
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(z)) return null;
  return [x as number, y as number, z as number];
}

watch(viewPresetOpen, (open) => {
  if (!open) return;
  name.value = "";
  const pose = viewer.value?.cameraPose();
  ready.value = !!pose;
  if (!pose) {
    ortho.value = orthoView.value;
    position.value = [0, 0, 0];
    target.value = [0, 0, 0];
    posModel.value = [0, 0, 0];
    yaw.value = 0;
    pitch.value = 0;
    return;
  }
  ortho.value = orthoView.value;
  position.value = [...pose.position];
  target.value = [...pose.target];
  posModel.value = [round3(pose.position[0]), round3(pose.position[1]), round3(pose.position[2])];
  const a = anglesFromPosition(
    new THREE.Vector3(...pose.position),
    new THREE.Vector3(...pose.target),
  );
  yaw.value = round1(a.yaw);
  pitch.value = round1(a.pitch);
});

const canSave = computed(
  () => name.value.trim().length > 0 && ready.value && readPosition() !== null,
);

function confirm() {
  const pos = readPosition();
  if (!pos) {
    Message.warning(t("view.saveInvalidPose"));
    return;
  }
  const err = saveCurrentView(name.value, {
    ortho: ortho.value,
    position: pos,
    target: target.value,
  });
  if (err) {
    Message.warning(err);
    return;
  }
  viewPresetOpen.value = false;
  Message.success(t("view.saved", { name: name.value.trim() }));
}
</script>

<template>
  <a-modal
    v-model:visible="viewPresetOpen"
    :title="t('view.saveTitle')"
    :ok-text="t('view.saveOk')"
    :cancel-text="t('common.cancel')"
    :ok-button-props="{ disabled: !canSave }"
    :mask-closable="false"
    width="440px"
    @ok="confirm"
  >
    <div class="view-preset">
      <a-form :model="{}" layout="vertical" size="small">
        <a-form-item :label="t('view.fieldName')">
          <a-input
            v-model="name"
            :placeholder="t('view.namePlaceholder')"
            :max-length="24"
            allow-clear
            @press-enter="confirm"
          />
        </a-form-item>
        <a-form-item :label="t('view.fieldProjection')">
          <a-radio-group v-model="ortho" type="button" size="small">
            <a-radio :value="false">{{ t("view.projectionPersp") }}</a-radio>
            <a-radio :value="true">{{ t("view.menuOrtho") }}</a-radio>
          </a-radio-group>
        </a-form-item>
        <a-form-item :label="t('view.fieldPosition')">
          <div class="view-preset__triple">
            <a-input-number
              v-model="posModel[0]"
              :precision="3"
              :step="1"
              hide-button
              @change="syncAnglesFromPosition"
            />
            <a-input-number
              v-model="posModel[1]"
              :precision="3"
              :step="1"
              hide-button
              @change="syncAnglesFromPosition"
            />
            <a-input-number
              v-model="posModel[2]"
              :precision="3"
              :step="1"
              hide-button
              @change="syncAnglesFromPosition"
            />
          </div>
        </a-form-item>
        <a-form-item :label="t('view.fieldAngles')">
          <div class="view-preset__pair">
            <a-input-number
              v-model="yaw"
              :precision="1"
              :step="5"
              hide-button
              :suffix="'°'"
              @change="syncPositionFromAngles"
            >
              <template #prefix>yaw</template>
            </a-input-number>
            <a-input-number
              v-model="pitch"
              :precision="1"
              :step="5"
              hide-button
              :suffix="'°'"
              @change="syncPositionFromAngles"
            >
              <template #prefix>pitch</template>
            </a-input-number>
          </div>
        </a-form-item>
      </a-form>
      <div v-if="savedViews.length > 0" class="view-preset__hint">
        {{ t("view.existingCount", { count: savedViews.length }) }}
      </div>
    </div>
  </a-modal>
</template>

<style scoped lang="scss">
.view-preset {
  &__triple {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  &__pair {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 6px;
  }

  &__hint {
    color: var(--color-text-3);
    font-size: 12px;
  }
}
</style>
