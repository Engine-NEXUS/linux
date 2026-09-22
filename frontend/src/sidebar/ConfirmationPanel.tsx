/**
 * NEXUS Confirmation Panel — interactive action confirmation card
 * displayed inside the Response Sidebar.
 *
 * Supports:
 * - WhatsApp message confirmations (recipient, message content preview)
 * - Swiggy / MCP actions (server, tool, parameter summaries)
 * - GitHub destructive operations (PR #, repo, destructive action badge)
 * - 1-click Confirm and Cancel buttons with loading feedback
 * - Voice assistance hint ("Say 'Yes' or 'Cancel'")
 */

import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ConfirmationData, useSidebar } from "./sidebarStore";

interface ConfirmationPanelProps {
  data: ConfirmationData;
  onClose?: () => void;
}

export function ConfirmationPanel({ data, onClose }: ConfirmationPanelProps) {
  const [submitting, setSubmitting] = useState(false);
  const [actionStatus, setActionStatus] = useState<string | null>(null);

  const cmd = data.command || {};
  const isMcp = cmd.kind === "mcp";
  const server = cmd.server || "";
  const tool = cmd.tool || "";
  const params = cmd.params || {};

  const isWhatsApp = isMcp && server === "whatsapp";
  const isSwiggy = isMcp && typeof server === "string" && server.startsWith("swiggy");
  const isAmazon = isMcp && server === "amazon";
  const isGitHub = !isMcp && (cmd.type || cmd.action || cmd.repo || cmd.pr_number !== undefined);

  // Handle Confirm Click
  const handleConfirm = useCallback(async () => {
    if (submitting) return;
    setSubmitting(true);
    setActionStatus("Executing...");

    try {
      if (isMcp) {
        console.log("[ConfirmationPanel] Confirming MCP action:", cmd);
        await invoke("orchestrator_mcp_confirm", {
          requestId: data.requestId || "mcp-confirm",
          confirmed: true,
          pending: cmd,
        });
      } else {
        console.log("[ConfirmationPanel] Confirming GitHub action:", cmd);
        await invoke("orchestrator_github_execute", {
          command: cmd,
          confirmed: true,
        });
      }
      useSidebar.getState().hide();
      onClose?.();
    } catch (err) {
      console.error("[ConfirmationPanel] Confirm failed:", err);
      setActionStatus("Execution failed");
      setSubmitting(false);
    }
  }, [submitting, isMcp, cmd, data.requestId, onClose]);

  // Handle Cancel Click
  const handleCancel = useCallback(async () => {
    if (submitting) return;
    setSubmitting(true);
    setActionStatus("Cancelling...");

    try {
      if (isMcp) {
        console.log("[ConfirmationPanel] Cancelling MCP action:", cmd);
        await invoke("orchestrator_mcp_confirm", {
          requestId: data.requestId || "mcp-cancel",
          confirmed: false,
          pending: cmd,
        });
      } else {
        await invoke("hide_sidebar").catch(() => {});
      }
      useSidebar.getState().hide();
      onClose?.();
    } catch (err) {
      console.warn("[ConfirmationPanel] Cancel failed:", err);
      useSidebar.getState().hide();
      onClose?.();
    }
  }, [submitting, isMcp, cmd, data.requestId, onClose]);

  // Determine button label
  let confirmLabel = "Confirm";
  if (isWhatsApp) confirmLabel = "Send Message";
  else if (isSwiggy) confirmLabel = "Confirm Order";
  else if (isGitHub) confirmLabel = "Confirm Action";

  return (
    <div className="confirmation-panel">
      {/* ── Card Header ── */}
      <div className="confirmation-header">
        <div className="confirmation-badge">
          {isWhatsApp ? (
            <span className="badge-whatsapp">WhatsApp</span>
          ) : isSwiggy ? (
            <span className="badge-swiggy">Swiggy</span>
          ) : isAmazon ? (
            <span className="badge-amazon">Amazon</span>
          ) : isGitHub ? (
            <span className="badge-github">GitHub</span>
          ) : (
            <span className="badge-general">Action</span>
          )}
          <span className="badge-req">Approval Required</span>
        </div>
        <h3 className="confirmation-title">
          {isWhatsApp
            ? "Send WhatsApp Message"
            : isSwiggy
            ? "Swiggy Order Request"
            : isGitHub
            ? "GitHub Action Request"
            : "Confirm Action"}
        </h3>
        <p className="confirmation-prompt">{data.prompt}</p>
      </div>

      {/* ── Structured Details ── */}
      <div className="confirmation-details">
        {isWhatsApp && (
          <>
            <div className="confirmation-field">
              <span className="field-label">Recipient</span>
              <span className="field-value field-recipient">{params.recipient || "Unknown Contact"}</span>
            </div>
            <div className="confirmation-field message-field">
              <span className="field-label">Message</span>
              <div className="field-message-bubble">
                {params.message || "(empty message)"}
              </div>
            </div>
          </>
        )}

        {isGitHub && (
          <>
            {cmd.repo && (
              <div className="confirmation-field">
                <span className="field-label">Repository</span>
                <span className="field-value font-mono">{cmd.repo}</span>
              </div>
            )}
            {cmd.pr_number !== undefined && (
              <div className="confirmation-field">
                <span className="field-label">Pull Request</span>
                <span className="field-value font-mono">#{cmd.pr_number}</span>
              </div>
            )}
            {cmd.branch && (
              <div className="confirmation-field">
                <span className="field-label">Branch</span>
                <span className="field-value font-mono">{cmd.branch}</span>
              </div>
            )}
          </>
        )}

        {isSwiggy && (
          <div className="confirmation-field">
            <span className="field-label">Action</span>
            <span className="field-value">{tool.replace(/_/g, " ")}</span>
          </div>
        )}

        {!isWhatsApp && !isGitHub && !isSwiggy && (
          <div className="confirmation-field">
            <span className="field-label">Target</span>
            <span className="field-value">{server ? `${server} / ${tool}` : "System Operation"}</span>
          </div>
        )}
      </div>

      {/* ── Status Message if any ── */}
      {actionStatus && (
        <div className="confirmation-status">{actionStatus}</div>
      )}

      {/* ── Action Buttons ── */}
      <div className="confirmation-actions">
        <button
          type="button"
          className="btn-confirmation btn-confirm"
          onClick={handleConfirm}
          disabled={submitting}
        >
          {submitting ? (
            <span className="btn-spinner" />
          ) : (
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <polyline points="20 6 9 17 4 12" />
            </svg>
          )}
          <span>{confirmLabel}</span>
        </button>

        <button
          type="button"
          className="btn-confirmation btn-cancel"
          onClick={handleCancel}
          disabled={submitting}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
          <span>Cancel</span>
        </button>
      </div>

      {/* ── Spoken Voice Guidance Hint ── */}
      <div className="confirmation-hint">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
          <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
          <line x1="12" y1="19" x2="12" y2="22" />
        </svg>
        <span>Say <strong>"Yes"</strong> or <strong>"Cancel"</strong>, or click an option above</span>
      </div>
    </div>
  );
}

export default ConfirmationPanel;
