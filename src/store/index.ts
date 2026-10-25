/**
 * 应用状态的总装配点。
 *
 * 各域（src/store/*.ts）各自管理自己的状态与动作；这里把它们的返回值
 * 合并成一个 AppStore，经 provide/inject 交给整棵组件树使用。
 *
 * 为什么不用模块级单例：测试用例会反复挂载 App，模块级响应式对象会在
 * 用例之间残留状态。createAppStore() 每次调用都产出一份全新状态，
 * 由 App.vue 在 setup 里创建并提供。
 *
 * 装配顺序即 onMounted 注册顺序，不能随意调换：
 * 侧栏拖动（resize 监听）最先，文件列表的右键菜单监听其次，
 * 界面状态恢复最后——恢复只改状态，必须发生在界面骨架就绪之后。
 */
import { inject, provide } from "vue";
import type { InjectionKey } from "vue";
import { useSidebarResize } from "../composables/useSidebarResize";
import { usePreviewResize } from "../composables/usePreviewResize";
import { createFileListSlice } from "./fileList";
import { createFileManagerSlice } from "./fileManager";
import { createCardPreviewSlice } from "./cardPreview";
import { createScenePreviewSlice } from "./scenePreview";
import { createBlockReplaceSlice } from "./blockReplace";
import { createVersionConvertSlice } from "./versionConvert";
import { createBackupsSlice } from "./backups";
import { createSetupSlice } from "./setup";
import { createUpdateSlice } from "./update";
import { createWorkspaceSlice } from "./workspace";
import { createUiStateSlice } from "./uiState";

export interface AppStore
  extends ReturnType<typeof useSidebarResize>,
    ReturnType<typeof usePreviewResize>,
    ReturnType<typeof createFileListSlice>,
    ReturnType<typeof createScenePreviewSlice>,
    ReturnType<typeof createBlockReplaceSlice>,
    ReturnType<typeof createVersionConvertSlice>,
    ReturnType<typeof createBackupsSlice>,
    ReturnType<typeof createSetupSlice>,
    ReturnType<typeof createUpdateSlice>,
    ReturnType<typeof createFileManagerSlice>,
    ReturnType<typeof createCardPreviewSlice>,
    ReturnType<typeof createWorkspaceSlice>,
    ReturnType<typeof createUiStateSlice> {}

/**
 * 创建一份完整的应用状态。只能在组件 setup（或等价的响应式上下文）里调用：
 * 各域会注册 watch 与生命周期钩子。
 */
export function createAppStore(): AppStore {
  const s = {} as AppStore;
  // 顺序即生命周期注册顺序（见文件头注释）。
  // 必须逐个 assign：域创建时注册的 watch 会立即求值，若用一次
  // Object.assign 合并所有域，参数求值期间 s 还是空对象，
  // 读跨域成员（如 scenePreview 读 analysis）会拿到 undefined。
  Object.assign(s, useSidebarResize());
  Object.assign(s, usePreviewResize());
  Object.assign(s, createFileListSlice(s));
  Object.assign(s, createScenePreviewSlice(s));
  Object.assign(s, createBlockReplaceSlice(s));
  Object.assign(s, createVersionConvertSlice(s));
  Object.assign(s, createBackupsSlice(s));
  Object.assign(s, createSetupSlice(s));
  Object.assign(s, createUpdateSlice());
  Object.assign(s, createFileManagerSlice(s));
  Object.assign(s, createCardPreviewSlice());
  Object.assign(s, createWorkspaceSlice(s));
  Object.assign(s, createUiStateSlice(s));
  return s;
}

export const APP_STORE: InjectionKey<AppStore> = Symbol("app-store");

/** 在根组件里创建并提供应用状态 */
export function provideAppStore(): AppStore {
  const s = createAppStore();
  provide(APP_STORE, s);
  return s;
}

/** 在子组件里取应用状态；缺了 provide 属于装配错误，必须当场暴露 */
export function useAppStore(): AppStore {
  const s = inject(APP_STORE);
  if (!s) throw new Error("useAppStore：上层组件没有 provideAppStore()");
  return s;
}
