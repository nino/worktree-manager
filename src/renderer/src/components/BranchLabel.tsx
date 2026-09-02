import { splitToolPrefix } from "../branchTool";
import { ToolIcon } from "./ToolIcons";

interface BranchLabelProps {
  branch: string;
}

/**
 * A branch name with a `claude/` or `cursor/` prefix swapped for that tool's
 * icon. The prefix text stays in the DOM, visually hidden, so the accessible
 * name, text search, and copy-by-selection still see the full branch name.
 */
export function BranchLabel({ branch }: BranchLabelProps) {
  const split = splitToolPrefix(branch);
  if (!split) return <>{branch}</>;
  return (
    <>
      <ToolIcon tool={split.tool} />
      <span className="sr-only">{split.tool}/</span>
      {split.rest}
    </>
  );
}
