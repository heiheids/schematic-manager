import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h, provide, ref } from "vue";
import { flushPromises, mount } from "@vue/test-utils";
import { createCardPreviewSlice } from "../store/cardPreview";
import type { CardPreviewSlice } from "../store/cardPreview";
import { APP_STORE } from "../store";
import type { AppStore } from "../store";
import SchematicCard from "../components/SchematicCard.vue";
import i18n from "../i18n";
import type { SchematicSummary, SceneData } from "../types";

const mocks = vi.hoisted(() => ({
  buildScene: vi.fn(),
  renderBlockIcons: vi.fn(),
  renderPng: vi.fn(),
  dispose: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  buildScene: mocks.buildScene,
  renderBlockIcons: mocks.renderBlockIcons,
}));

vi.mock("../render/scene3d", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../render/scene3d")>();
  return {
    ...actual,
    OffscreenRenderer: class {
      renderPng = mocks.renderPng;
      dispose = mocks.dispose;
    },
  };
});

const file: SchematicSummary = {
  path: "/p/a.litematic",
  fileName: "a.litematic",
  dirName: "/p",
  fileSize: 1,
  error: "",
  formatVersion: 6,
  dataVersion: 3465,
  mcVersionName: "1.20.1",
  name: "A",
  author: "",
  description: "",
  timeCreated: 0,
  timeModified: 0,
  enclosing: [1, 1, 1],
  totalBlocks: 1,
  totalVolume: 1,
  regionCount: 1,
  entityCount: 0,
  tileEntityCount: 0,
  pendingTicks: 0,
  hasPreview: false,
};

const originalCreateUrl = Object.getOwnPropertyDescriptor(URL, "createObjectURL");
const originalRevokeUrl = Object.getOwnPropertyDescriptor(URL, "revokeObjectURL");

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function mountSlice() {
  let slice!: CardPreviewSlice;
  const wrapper = mount(
    defineComponent({
      setup() {
        slice = createCardPreviewSlice();
        return () => null;
      },
    }),
  );
  return { slice, wrapper };
}

beforeEach(() => {
  mocks.buildScene.mockReset();
  mocks.renderBlockIcons.mockReset();
  mocks.renderPng.mockReset();
  mocks.dispose.mockReset();
  mocks.renderPng.mockResolvedValue(new Uint8Array([1]));
  Object.defineProperty(URL, "createObjectURL", {
    configurable: true,
    value: vi.fn(() => "blob:preview"),
  });
  Object.defineProperty(URL, "revokeObjectURL", {
    configurable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  if (originalCreateUrl) Object.defineProperty(URL, "createObjectURL", originalCreateUrl);
  else Reflect.deleteProperty(URL, "createObjectURL");
  if (originalRevokeUrl) Object.defineProperty(URL, "revokeObjectURL", originalRevokeUrl);
  else Reflect.deleteProperty(URL, "revokeObjectURL");
});

describe("卡片预览队列", () => {
  it("局部失效只丢弃对应路径，其它排队请求继续完成", async () => {
    const oldA = deferred<SceneData>();
    const rendered: string[] = [];
    mocks.buildScene.mockImplementation((path: string) => {
      if (path === file.path) return oldA.promise;
      return Promise.resolve({ origin: path } as unknown as SceneData);
    });
    mocks.renderPng.mockImplementation((data: SceneData) => {
      rendered.push((data as unknown as { origin: string }).origin);
      return Promise.resolve(new Uint8Array([1]));
    });
    const other = { ...file, path: "/p/b.litematic", fileName: "b.litematic" };
    const { slice, wrapper } = mountSlice();

    slice.requestPreview(file);
    slice.requestPreview(other);
    await flushPromises();
    expect(mocks.buildScene).toHaveBeenCalledTimes(1);

    slice.invalidate([file.path]);
    mocks.buildScene.mockImplementation((path: string) =>
      Promise.resolve({ origin: `new:${path}` } as unknown as SceneData),
    );
    slice.requestPreview(file);
    oldA.resolve({ origin: "old:a" } as unknown as SceneData);
    await flushPromises();

    expect(slice.previewOf(other.path).status).toBe("ready");
    expect(slice.previewOf(file.path).status).toBe("ready");
    expect(rendered).toContain(`new:${other.path}`);
    expect(rendered).toContain(`new:${file.path}`);
    expect(rendered).not.toContain("old:a");
    wrapper.unmount();
  });

  it("可见卡片在缓存失效后自动重新请求", async () => {
    vi.stubGlobal("IntersectionObserver", undefined);
    mocks.buildScene.mockResolvedValue({} as SceneData);
    mocks.renderPng.mockRejectedValue(new Error("WebGL 不可用"));
    let slice!: CardPreviewSlice;
    const wrapper = mount(
      defineComponent({
        setup() {
          slice = createCardPreviewSlice();
          provide(APP_STORE, {
            checkedPaths: ref(new Set<string>()),
            setFileChecked: vi.fn(),
            requestPreview: slice.requestPreview,
            previewOf: slice.previewOf,
            isDup: () => false,
            isSuspect: () => false,
            dupNoOf: () => 0,
            dupColorNo: () => 0,
            onRowContextMenu: vi.fn(),
            rowClass: () => "",
          } as unknown as AppStore);
          return () => h(SchematicCard, { file });
        },
      }),
      {
        global: {
          plugins: [i18n],
          stubs: {
            "a-button": { template: "<button><slot /></button>" },
            "a-checkbox": { template: "<input type='checkbox' />" },
          },
        },
      },
    );
    await flushPromises();
    expect(mocks.buildScene).toHaveBeenCalledTimes(1);
    expect(slice.previewOf(file.path).status).toBe("error");

    slice.invalidate();
    await flushPromises();
    expect(mocks.buildScene).toHaveBeenCalledTimes(2);
    wrapper.unmount();
    expect(mocks.dispose).toHaveBeenCalledTimes(1);
  });
});
