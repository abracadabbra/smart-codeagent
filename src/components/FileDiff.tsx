import * as Diff from "jsdiff";

interface FileDiffProps {
  oldText?: string;
  newText?: string;
  fileName?: string;
}

interface DiffLine {
  type: "added" | "removed" | "unchanged" | "context";
  content: string;
  lineNumber: number;
}

export function FileDiff({ oldText, newText, fileName }: FileDiffProps) {
  const diffResult = oldText && newText
    ? Diff.diffLines(oldText, newText, { newlineIsToken: false })
    : [];

  const lines: DiffLine[] = [];
  let oldLineNum = 1;
  let newLineNum = 1;

  diffResult.forEach((part) => {
    const contentLines = part.value.split("\n");
    contentLines.forEach((line, idx) => {
      if (idx === contentLines.length - 1 && line === "") return;

      if (part.added) {
        lines.push({ type: "added", content: line, lineNumber: newLineNum });
        newLineNum++;
      } else if (part.removed) {
        lines.push({ type: "removed", content: line, lineNumber: oldLineNum });
        oldLineNum++;
      } else {
        lines.push({ type: "unchanged", content: line, lineNumber: oldLineNum });
        oldLineNum++;
        newLineNum++;
      }
    });
  });

  if (lines.length === 0) {
    if (newText) {
      return (
        <div className="rounded-lg overflow-hidden border border-ink-800/50">
          <div className="flex items-center justify-between px-3 py-1.5 bg-ink-800/40 border-b border-ink-800/50">
            <span className="text-[11px] text-ink-400 font-medium">
              {fileName || "new file"}
            </span>
            <span className="text-[10px] text-green-400">新增</span>
          </div>
          <pre className="bg-ink-900/80 p-3 font-mono text-[11px] text-ink-100 whitespace-pre-wrap max-h-64 overflow-y-auto">
            {newText}
          </pre>
        </div>
      );
    }
    return null;
  }

  return (
    <div className="rounded-lg overflow-hidden border border-ink-800/50">
      <div className="flex items-center justify-between px-3 py-1.5 bg-ink-800/40 border-b border-ink-800/50">
        <span className="text-[11px] text-ink-400 font-medium">
          {fileName || "file"}
        </span>
        <span className="text-[10px] text-blue-400">修改</span>
      </div>
      <div className="bg-ink-900/80 overflow-x-auto">
        <table className="w-full font-mono text-[11px]">
          <tbody>
            {lines.map((line, idx) => (
              <tr key={idx}>
                <td
                  className={`w-12 text-right pr-3 select-none border-r border-ink-800/50 ${
                    line.type === "added"
                      ? "bg-green-500/10 text-green-500"
                      : line.type === "removed"
                      ? "bg-red-500/10 text-red-500"
                      : "bg-transparent text-ink-600"
                  }`}
                >
                  {line.type === "added" || line.type === "unchanged"
                    ? line.lineNumber
                    : ""}
                </td>
                <td
                  className={`w-12 text-right pr-3 select-none border-r border-ink-800/50 ${
                    line.type === "removed"
                      ? "bg-red-500/10 text-red-500"
                      : line.type === "added"
                      ? "bg-green-500/10 text-green-500"
                      : "bg-transparent text-ink-600"
                  }`}
                >
                  {line.type === "removed" || line.type === "unchanged"
                    ? line.lineNumber
                    : ""}
                </td>
                <td
                  className={`px-3 ${
                    line.type === "added"
                      ? "bg-green-500/10 text-green-300"
                      : line.type === "removed"
                      ? "bg-red-500/10 text-red-300"
                      : "bg-transparent text-ink-200"
                  }`}
                >
                  <span className="inline-block w-2 text-center mr-1">
                    {line.type === "added" ? "+" : line.type === "removed" ? "-" : " "}
                  </span>
                  <span className="whitespace-pre">{line.content || " "}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
