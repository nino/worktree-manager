import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { BranchLabel } from "./BranchLabel";

describe("BranchLabel", () => {
  it("swaps a tool prefix for its icon but keeps the full name as text", () => {
    const { container } = render(<BranchLabel branch="claude/skill-matching" />);
    expect(container.textContent).toBe("claude/skill-matching");
    expect(container.querySelector(".tool-icon-claude")).not.toBeNull();
    expect(container.querySelector(".sr-only")?.textContent).toBe("claude/");
  });

  it("renders other branches as plain text", () => {
    const { container } = render(<BranchLabel branch="main" />);
    expect(container.textContent).toBe("main");
    expect(container.querySelector(".tool-icon")).toBeNull();
  });
});
