import { expect, it } from "vitest";

import { splitAgentPrefix } from "./branch";

it("splits agent prefixes only", () => {
  expect(splitAgentPrefix("claude/rust-rewrite")).toEqual(["claude", "rust-rewrite"]);
  expect(splitAgentPrefix("cursor/a/b")).toEqual(["cursor", "a/b"]);
  expect(splitAgentPrefix("claude/")).toBeNull();
  expect(splitAgentPrefix("claude")).toBeNull();
  expect(splitAgentPrefix("feature/claude/x")).toBeNull();
});
