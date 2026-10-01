/**
 * NEXUS GitHub Conflict Panel — displays merge conflict details
 * with copy-paste options for each conflict block.
 *
 * This component is shown when the orchestrator emits a "conflict_report"
 * event. It displays:
 *   - The PR number and repo
 *   - Each conflicted file with its conflict blocks
 *   - Copy buttons for HEAD content, branch content, and full conflict blocks
 *   - Instructions on how to fix and retry
 */

import { useState, useCallback } from "react";

interface ConflictBlock {
  start_line: number;
  head_content: string;
  branch_content: string;
}

interface ConflictFile {
  filename: string;
  conflict_count: number;
  blocks: ConflictBlock[];
}

interface GitHubConflictPanelProps {
  prNumber: number;
  repo: string;
  conflictFiles: ConflictFile[];
  message: string;
  onRetry?: () => void;
}

export function GitHubConflictPanel({
  prNumber,
  repo,
  conflictFiles,
  message,
  onRetry,
}: GitHubConflictPanelProps) {
  const [copiedBlock, setCopiedBlock] = useState<string | null>(null);

  const copyToClipboard = useCallback(async (text: string, blockId: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedBlock(blockId);
      setTimeout(() => setCopiedBlock(null), 2000);
    } catch (err) {
      console.error("[NEXUS] Failed to copy:", err);
    }
  }, []);

  if (conflictFiles.length === 0) {
    return (
      <div className="github-conflict-panel">
        <div className="conflict-header">
          <h3>Merge Conflict — PR #{prNumber}</h3>
          <p className="conflict-repo">{repo}</p>
        </div>
        <p className="conflict-message">{message}</p>
        <p className="conflict-hint">
          GitHub reports this PR has conflicts, but no specific file details
          were available. Please check the PR on GitHub for more information.
        </p>
      </div>
    );
  }

  return (
    <div className="github-conflict-panel">
      <div className="conflict-header">
        <h3>Merge Conflict — PR #{prNumber}</h3>
        <p className="conflict-repo">{repo}</p>
      </div>

      <p className="conflict-message">{message}</p>

      <div className="conflict-files">
        {conflictFiles.map((file, fileIdx) => (
          <div key={fileIdx} className="conflict-file">
            <div className="conflict-file-header">
              <span className="conflict-filename">{file.filename}</span>
              <span className="conflict-count">
                {file.conflict_count} conflict{file.conflict_count !== 1 ? "s" : ""}
              </span>
            </div>

            {file.blocks.map((block, blockIdx) => {
              const blockId = `${fileIdx}-${blockIdx}`;
              const fullBlock = `<<<<<<< HEAD\n${block.head_content}\n=======\n${block.branch_content}\n>>>>>>> branch`;

              return (
                <div key={blockIdx} className="conflict-block">
                  <div className="conflict-block-header">
                    <span>Line {block.start_line}</span>
                  </div>

                  <div className="conflict-side">
                    <div className="conflict-side-label">
                      HEAD (base)
                      <button
                        className="copy-btn"
                        onClick={() => copyToClipboard(block.head_content, `${blockId}-head`)}
                      >
                        {copiedBlock === `${blockId}-head` ? "Copied!" : "Copy"}
                      </button>
                    </div>
                    <pre className="conflict-content">{block.head_content}</pre>
                  </div>

                  <div className="conflict-side">
                    <div className="conflict-side-label">
                      Branch (feature)
                      <button
                        className="copy-btn"
                        onClick={() => copyToClipboard(block.branch_content, `${blockId}-branch`)}
                      >
                        {copiedBlock === `${blockId}-branch` ? "Copied!" : "Copy"}
                      </button>
                    </div>
                    <pre className="conflict-content">{block.branch_content}</pre>
                  </div>

                  <div className="conflict-full-block">
                    <button
                      className="copy-btn copy-btn-full"
                      onClick={() => copyToClipboard(fullBlock, `${blockId}-full`)}
                    >
                      {copiedBlock === `${blockId}-full` ? "Copied!" : "Copy Full Conflict Block"}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        ))}
      </div>

      <div className="conflict-instructions">
        <h4>How to fix:</h4>
        <ol>
          <li>Copy the HEAD or branch content for each conflict</li>
          <li>Resolve the conflicts in your local clone or on GitHub</li>
          <li>Push the resolved changes to the PR branch</li>
          <li>Ask NEXUS to merge the PR again</li>
        </ol>
        {onRetry && (
          <button className="retry-btn" onClick={onRetry}>
            Retry Merge
          </button>
        )}
      </div>
    </div>
  );
}

export default GitHubConflictPanel;
