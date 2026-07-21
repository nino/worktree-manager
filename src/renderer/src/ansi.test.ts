import { describe, expect, it } from "vitest";
import { stripAnsi } from "./ansi";

const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);

describe("stripAnsi", () => {
  it("removes SGR colour codes", () => {
    expect(stripAnsi(`${ESC}[31mred${ESC}[0m`)).toBe("red");
  });

  it("removes bold and reset around text", () => {
    expect(stripAnsi(`${ESC}[1mbold${ESC}[22m done`)).toBe("bold done");
  });

  it("removes cursor-movement and clear-line sequences", () => {
    expect(stripAnsi(`${ESC}[2K${ESC}[Gprogress`)).toBe("progress");
  });

  it("removes OSC window-title sequences (BEL terminated)", () => {
    expect(stripAnsi(`${ESC}]0;my title${BEL}shell`)).toBe("shell");
  });

  it("removes OSC sequences terminated by ST (ESC backslash)", () => {
    expect(stripAnsi(`${ESC}]0;title${ESC}\\rest`)).toBe("rest");
  });

  it("leaves plain text and newlines untouched", () => {
    expect(stripAnsi("line one\nline two\n")).toBe("line one\nline two\n");
  });

  it("preserves carriage returns (not an escape sequence)", () => {
    expect(stripAnsi("50%\r100%")).toBe("50%\r100%");
  });

  it("handles a realistic coloured dev-server line", () => {
    const line = `${ESC}[32m done${ESC}[39m ready in ${ESC}[1m234${ESC}[22m ms`;
    expect(stripAnsi(line)).toBe(" done ready in 234 ms");
  });
});
