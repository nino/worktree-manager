import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";
import { apiMock } from "./apiMock";

// api.ts captures `window.api` at import time, so it must exist before any app
// module loads. Set the shared mock object once; tests reconfigure its methods.
(window as unknown as { api: unknown }).api = apiMock;

afterEach(() => cleanup());
