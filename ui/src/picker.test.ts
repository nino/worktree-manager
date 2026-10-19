import { describe, expect, it } from "vitest";

import { choice, moveBy, openPicker, setQuery } from "./picker";

describe("picker", () => {
  it("opens on the current branch and keeps it when the list is stale", () => {
    expect(openPicker(["main", "dev"], "dev").selected).toBe(1);
    const p = openPicker(["main"], "gone");
    expect(p.all).toEqual(["gone", "main"]);
    expect(p.selected).toBe(0);
  });

  it("does nothing for the current branch", () => {
    const p = openPicker(["main", "dev"], "main");
    expect(choice(p, 0)).toBeNull();
    expect(choice(p, 1)).toBe("dev");
    expect(choice(p, 5)).toBeNull();
  });

  it("filters, selects the best match and stops at the ends", () => {
    let p = setQuery(openPicker(["main", "feature/login", "fix"], "main"), "fl");
    expect(choice(p, p.selected)).toBe("feature/login");
    p = moveBy(p, -1);
    expect(p.selected).toBe(0);
    p = moveBy(p, 5);
    expect(p.selected).toBe(p.rows.length - 1);
    p = setQuery(p, "zzz");
    expect(p.selected).toBeNull();
  });
});
