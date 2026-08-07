import { describe, expect, it } from "vitest";
import { GitError } from "./git";
import { describeAddFailure } from "./repos";

/** A GitError shaped exactly like `runGit` raises one. */
function gitError(stderr: string): GitError {
  return new GitError(`git rev-parse --show-toplevel failed: ${stderr}`, "/tmp", [], stderr);
}

describe("describeAddFailure", () => {
  it("recognises a non-repository folder", () => {
    const stderr = "fatal: not a git repository (or any of the parent directories): .git";
    expect(describeAddFailure(gitError(stderr))).toBe("Not a git repository");
  });

  it("recognises a folder that no longer exists", () => {
    expect(describeAddFailure(new Error("spawn git ENOENT"))).toBe("Folder not found");
  });

  it("keeps git's own wording for anything else, without the runGit prefix", () => {
    const stderr =
      "fatal: detected dubious ownership in repository at '/srv/app'\nTo add an\nexception";
    expect(describeAddFailure(gitError(stderr))).toBe(
      "detected dubious ownership in repository at '/srv/app'",
    );
  });

  it("falls back to a generic message for an empty error", () => {
    expect(describeAddFailure(new Error(""))).toBe("Could not add this folder");
  });
});
