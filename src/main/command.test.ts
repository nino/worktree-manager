import { describe, expect, it } from "vitest";
import { buildCommand, shellQuote } from "./command";

describe("shellQuote", () => {
  it("wraps in single quotes", () => {
    expect(shellQuote("/a/b c")).toBe("'/a/b c'");
  });

  it("escapes embedded single quotes", () => {
    expect(shellQuote("it's")).toBe(`'it'\\''s'`);
  });
});

describe("buildCommand", () => {
  it("appends the quoted path when there is no placeholder", () => {
    expect(buildCommand("code", "/w t")).toBe("code '/w t'");
  });

  it("substitutes {path} placeholders", () => {
    expect(buildCommand("open -na Ghostty --args --working-directory={path}", "/w t")).toBe(
      "open -na Ghostty --args --working-directory='/w t'",
    );
  });

  it("substitutes every occurrence of {path}", () => {
    expect(buildCommand("echo {path} {path}", "/x")).toBe("echo '/x' '/x'");
  });
});
