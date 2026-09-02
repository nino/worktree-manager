import { describe, expect, it } from "vitest";
import { splitToolPrefix } from "./branchTool";

describe("splitToolPrefix", () => {
  it("recognises claude/ and cursor/ prefixes", () => {
    expect(splitToolPrefix("claude/skill-matching-abc")).toEqual({
      tool: "claude",
      rest: "skill-matching-abc",
    });
    expect(splitToolPrefix("cursor/fix-login")).toEqual({ tool: "cursor", rest: "fix-login" });
  });

  it("keeps nested slashes in the remainder", () => {
    expect(splitToolPrefix("claude/nino/thing")).toEqual({ tool: "claude", rest: "nino/thing" });
  });

  it("leaves other names alone", () => {
    expect(splitToolPrefix("main")).toBeNull();
    expect(splitToolPrefix("feature/claude/x")).toBeNull();
    expect(splitToolPrefix("claude")).toBeNull();
    expect(splitToolPrefix("claude/")).toBeNull();
    expect(splitToolPrefix("Claude/x")).toBeNull();
    expect(splitToolPrefix("claude-code/x")).toBeNull();
  });
});
