import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { apiMock } from "./apiMock";

// api.ts captures `window.api` at import time, so it must exist before any app
// module loads. Set the shared mock object once; tests reconfigure its methods.
(window as unknown as { api: unknown }).api = apiMock;

// happy-dom omits these browser APIs that components rely on; stub them so
// mounting (e.g. the terminal drawer or branch picker) doesn't throw.
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}

afterEach(() => cleanup());
