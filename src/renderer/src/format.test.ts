import { describe, expect, it } from "vitest";
import type { UpdateStatus } from "@shared/types";
import { describeUpdate } from "./format";

function status(over: Partial<UpdateStatus> = {}): UpdateStatus {
  return { state: "idle", currentVersion: "1.0.7", ...over };
}

describe("describeUpdate", () => {
  it("separates a checked idle state from a not-yet-checked one", () => {
    expect(describeUpdate(status())).toBe("Not checked yet.");
    expect(describeUpdate(status({ lastCheckedAt: 1_700_000_000_000 }))).toBe("Up to date.");
  });

  it("names the version coming down, with progress once there is any", () => {
    expect(describeUpdate(status({ state: "downloading", newVersion: "1.0.9" }))).toBe(
      "Downloading version 1.0.9…",
    );
    expect(describeUpdate(status({ state: "downloading", newVersion: "1.0.9", percent: 42 }))).toBe(
      "Downloading version 1.0.9… 42%",
    );
    // A download can start before the feed's version is known.
    expect(describeUpdate(status({ state: "downloading" }))).toBe("Downloading an update…");
  });

  it("offers the restart once a build is staged", () => {
    expect(describeUpdate(status({ state: "ready", newVersion: "1.0.9" }))).toBe(
      "Version 1.0.9 is ready — restart to install it.",
    );
  });

  it("surfaces the reason a check failed", () => {
    expect(describeUpdate(status({ state: "error", message: "net::ERR_FAILED" }))).toBe(
      "Last check failed: net::ERR_FAILED",
    );
  });

  it("explains why a build from source never updates itself", () => {
    const message = "Automatic updates are off in development builds.";
    expect(describeUpdate(status({ state: "unsupported", message }))).toBe(message);
  });
});
