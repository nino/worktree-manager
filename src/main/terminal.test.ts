import { describe, expect, it } from "vitest";
import { parseDefaultTerminalBundleId } from "./terminal";

describe("parseDefaultTerminalBundleId", () => {
  it("returns the RoleAll handler for public.unix-executable", () => {
    expect(
      parseDefaultTerminalBundleId([
        { LSHandlerContentType: "public.html", LSHandlerRoleAll: "com.example.browser" },
        {
          LSHandlerContentType: "public.unix-executable",
          LSHandlerRoleAll: "com.mitchellh.ghostty",
        },
      ]),
    ).toBe("com.mitchellh.ghostty");
  });

  it("prefers RoleShell over RoleAll", () => {
    expect(
      parseDefaultTerminalBundleId([
        {
          LSHandlerContentType: "public.unix-executable",
          LSHandlerRoleShell: "com.googlecode.iterm2",
          LSHandlerRoleAll: "com.apple.Terminal",
        },
      ]),
    ).toBe("com.googlecode.iterm2");
  });

  it("returns null when there is no unix-executable handler", () => {
    expect(
      parseDefaultTerminalBundleId([
        { LSHandlerContentType: "public.html", LSHandlerRoleAll: "com.example.browser" },
      ]),
    ).toBeNull();
  });

  it("ignores placeholder '-' entries", () => {
    expect(
      parseDefaultTerminalBundleId([
        { LSHandlerContentType: "public.unix-executable", LSHandlerRoleAll: "-" },
      ]),
    ).toBeNull();
  });

  it("returns null for an empty handler list", () => {
    expect(parseDefaultTerminalBundleId([])).toBeNull();
  });
});
