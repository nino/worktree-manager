import { describe, expect, it } from "vitest";
import { tildify } from "./paths";

describe("tildify", () => {
  it("abbreviates paths under the home directory", () => {
    expect(tildify("/Users/nino/dev/x", "/Users/nino")).toBe("~/dev/x");
  });

  it("abbreviates the home directory itself", () => {
    expect(tildify("/Users/nino", "/Users/nino")).toBe("~");
  });

  it("leaves other paths alone", () => {
    expect(tildify("/opt/thing", "/Users/nino")).toBe("/opt/thing");
    expect(tildify("/Users/ninofied/x", "/Users/nino")).toBe("/Users/ninofied/x");
  });

  it("handles an empty home defensively", () => {
    expect(tildify("/a/b", "")).toBe("/a/b");
  });
});
