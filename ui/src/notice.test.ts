import { describe, expect, it } from "vitest";

import { noticeIsLong, noticeSummary } from "./notice";

describe("notice", () => {
  it("keeps short notices and folds long ones", () => {
    expect(noticeSummary("one\ntwo")).toBe("one\ntwo");
    expect(noticeSummary("one\n  two\nthree\nfour")).toBe("one\ntwo … (2 more lines)");
  });

  it("offers details for many lines or much text", () => {
    expect(noticeIsLong("a\nb\nc")).toBe(false);
    expect(noticeIsLong("a\nb\nc\nd")).toBe(true);
    expect(noticeIsLong("x".repeat(241))).toBe(true);
  });
});
