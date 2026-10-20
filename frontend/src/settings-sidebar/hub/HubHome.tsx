import { useState, type ReactNode } from "react";
import type { ThemeMode } from "../../sidebar/theme";
import { formatCount, initialOf, requestsSubtitle, type HubAccount, type UsageStats } from "./hubModel";

export type HubPage = "home" | "keys" | "memory" | "audio" | "display" | "connections" | "advanced";

interface AccountsApi {
  accounts: HubAccount[];
  googleNeedsSignin?: boolean;
  busy: string | null;
  error: string | null;
  dismissError: () => void;
  addGoogle: () => void;
  addGithub: () => void;
  makePrimary: (email: string) => void;
  remove: (a: HubAccount) => void;
}

interface Props {
  acc: AccountsApi;
  insights: { keyCount: number | null; usage: UsageStats | null; mcp: number | null };
  themeMode: ThemeMode;
  onTheme: (m: ThemeMode) => void;
  onOpen: (p: HubPage) => void;
}

const Svg = ({ children }: { children: ReactNode }) => (
  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {children}
  </svg>
);

const ICONS = {
  key: (
    <Svg>
      <circle cx="8" cy="15" r="4" />
      <path d="M11 12l9-9M16 7l3 3" />
    </Svg>
  ),
  orb: (
    <Svg>
      <circle cx="12" cy="12" r="8" />
      <circle cx="12" cy="12" r="3" />
    </Svg>
  ),
  audio: (
    <Svg>
      <path d="M4 10v4h4l5 4V6L8 10H4z" />
      <path d="M16 9a4 4 0 010 6M18.5 6.5a8 8 0 010 11" />
    </Svg>
  ),
  display: (
    <Svg>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M8 20h8M12 16v4" />
    </Svg>
  ),
  plug: (
    <Svg>
      <path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 01-12 0V8zM12 17v4" />
    </Svg>
  ),
  theme: (
    <Svg>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
    </Svg>
  ),
  advanced: (
    <Svg>
      <path d="M4 7h10M18 7h2M4 17h2M10 17h10" />
      <circle cx="16" cy="7" r="2" />
      <circle cx="8" cy="17" r="2" />
    </Svg>
  ),
  chart: (
    <Svg>
      <path d="M4 20V10M10 20V4M16 20v-7M22 20H2" />
    </Svg>
  ),
};

function Avatar({ a, size }: { a: HubAccount; size: number }) {
  const [broken, setBroken] = useState(false);
  const cls = `hx-avatar hx-avatar--${size}`;
  if (a.picture && !broken) {
    return <img className={cls} src={a.picture} alt="" referrerPolicy="no-referrer" onError={() => setBroken(true)} />;
  }
  return (
    <div className={`${cls} hx-avatar--fallback`} aria-hidden="true">
      {initialOf(a)}
    </div>
  );
}

function Row({ icon, title, sub, onClick, right }: { icon: ReactNode; title: string; sub: string; onClick?: () => void; right?: ReactNode }) {
  return (
    <div className={`hx-row ${onClick ? "hx-row--tap" : ""}`} onClick={onClick} role={onClick ? "button" : undefined}>
      <span className="hx-row-icon">{icon}</span>
      <span className="hx-row-text">
        <span className="hx-row-title">{title}</span>
        <span className="hx-row-sub">{sub}</span>
      </span>
      {right ?? <span className="hx-chevron" aria-hidden="true">›</span>}
    </div>
  );
}

const val = (n: number | null) => (n === null ? "–" : formatCount(n));

/** Profile-style home: accounts card, insights card, grouped rows. */
export function HubHome({ acc, insights, themeMode, onTheme, onOpen }: Props) {
  const [confirm, setConfirm] = useState<string | null>(null);
  const hasGithub = acc.accounts.some((a) => a.provider === "github");

  return (
    <div className="hx-page">
      {acc.error && (
        <div className="hx-error" onClick={acc.dismissError} role="alert">
          {acc.error}
        </div>
      )}

      {/* ── Accounts ── */}
      <div className="hx-card hx-accounts">
        {acc.accounts.length === 0 && (
          <div className="hx-empty">No account connected. Add one to use Gmail, Calendar and GitHub features.</div>
        )}
        {acc.accounts.map((a) => {
          const key = `${a.provider}:${a.id}`;
          return (
            <div className="hx-account" key={key}>
              <Avatar a={a} size={44} />
              <div
                className={`hx-account-meta ${a.provider === "google" && !a.primary ? "hx-account-meta--tap" : ""}`}
                onClick={() => {
                  if (a.provider === "google" && !a.primary) acc.makePrimary(a.id);
                }}
                title={a.provider === "google" && !a.primary ? "Tap to make primary" : undefined}
              >
                <div className="hx-account-name">
                  {a.name}
                  {a.primary && <span className="hx-badge">Primary</span>}
                  <span className="hx-provider">{a.provider === "google" ? "Google" : "GitHub"}</span>
                  {a.provider === "google" && acc.googleNeedsSignin && (
                    <span className="hx-badge" style={{ background: "rgba(239, 68, 68, 0.15)", color: "#ef4444", marginLeft: 4 }}>
                      Needs Sign-In
                    </span>
                  )}
                </div>
                <div className="hx-account-handle">{a.handle}</div>
              </div>
              {a.provider === "google" && acc.googleNeedsSignin && confirm !== key && (
                <button
                  className="hx-btn hx-btn--tiny"
                  style={{ marginRight: 8, background: "rgba(59, 130, 246, 0.15)", color: "var(--nx-accent-blue)", border: "1px solid var(--nx-accent-blue)", whiteSpace: "nowrap" }}
                  disabled={acc.busy !== null}
                  onClick={(e) => {
                    e.stopPropagation();
                    acc.addGoogle();
                  }}
                >
                  {acc.busy === "add-google" ? "Signing in…" : "Re-authenticate"}
                </button>
              )}
              {confirm === key ? (
                <div className="hx-confirm-inline">
                  <button className="hx-btn hx-btn--tiny" onClick={() => setConfirm(null)}>
                    Keep
                  </button>
                  <button
                    className="hx-btn hx-btn--tiny hx-btn--danger"
                    disabled={acc.busy !== null}
                    onClick={() => {
                      setConfirm(null);
                      acc.remove(a);
                    }}
                  >
                    Remove
                  </button>
                </div>
              ) : (
                <button className="hx-icon-btn" title={`Remove ${a.handle} from NEXUS`} onClick={() => setConfirm(key)} aria-label="Remove account">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                    <path d="M18 6L6 18M6 6l12 12" />
                  </svg>
                </button>
              )}
            </div>
          );
        })}
        <div className="hx-divider" />
        <div className="hx-add">
          <button className="hx-link" disabled={acc.busy !== null} onClick={acc.addGoogle}>
            {acc.busy === "add-google" ? "Signing in…" : "+ Add Google account"}
          </button>
          {!hasGithub && (
            <button className="hx-link" disabled={acc.busy !== null} onClick={acc.addGithub}>
              {acc.busy === "add-github" ? "Waiting…" : "+ Add GitHub"}
            </button>
          )}
        </div>
      </div>
      <div className="hx-footnote">Removing an account only disconnects it from NEXUS — it does not delete it at Google or GitHub.</div>

      {/* ── Insights ── */}
      <div className="hx-card hx-insights">
        <div className="hx-insights-head">
          <span className="hx-row-icon">{ICONS.chart}</span>
          <span className="hx-row-title">Your Command Hub Insights</span>
        </div>
        <div className="hx-stats">
          <div className="hx-stat">
            <div className="hx-stat-value">{val(insights.keyCount)}</div>
            <div className="hx-stat-label">API keys</div>
            <div className="hx-stat-sub hx-stat-sub--placeholder">&nbsp;</div>
          </div>
          <div className="hx-stat">
            <div className="hx-stat-value">{val(insights.mcp)}</div>
            <div className="hx-stat-label">MCP connected</div>
            <div className="hx-stat-sub hx-stat-sub--placeholder">&nbsp;</div>
          </div>
          <div className="hx-stat">
            <div className="hx-stat-value">{insights.usage ? formatCount(insights.usage.today) : "–"}</div>
            <div className="hx-stat-label">Requests today</div>
            <div className="hx-stat-sub">{requestsSubtitle(insights.usage) || "\u00A0"}</div>
          </div>
        </div>
      </div>

      {/* ── Set up ── */}
      <div className="hx-section">How do you set up NEXUS?</div>
      <Row icon={ICONS.key} title="API Keys" sub="Add, replace or delete your keys" onClick={() => onOpen("keys")} />
      <Row icon={ICONS.chart} title="Memory" sub="What I remember, and what I send to the cloud" onClick={() => onOpen("memory")} />
      <Row
        icon={ICONS.orb}
        title="Customize Voice Orb"
        sub="3D particle patterns for thinking, speaking and listening"
        onClick={() => {
          import("@tauri-apps/api/core").then(({ invoke }) => {
            invoke("open_orb_studio_window").catch(() => {});
          });
        }}
      />
      <Row icon={ICONS.audio} title="Audio" sub="Voice, volume, wake word and speech" onClick={() => onOpen("audio")} />
      <Row icon={ICONS.display} title="Display" sub="Orb position, size, colour and 3D studio" onClick={() => onOpen("display")} />
      <Row icon={ICONS.plug} title="Connections" sub="MCP servers, Telegram and bridges" onClick={() => onOpen("connections")} />
      <Row icon={ICONS.advanced} title="Advanced" sub="Sign-in credentials and vision settings" onClick={() => onOpen("advanced")} />

      <div className="hx-section">Adjust the theme to your preferences</div>
      <Row
        icon={ICONS.theme}
        title="Change Theme"
        sub="Light, dark, or follow the system"
        right={
          <span className="hx-seg" role="group" aria-label="Theme">
            {(["light", "dark", "system"] as const).map((m) => (
              <button key={m} className={`hx-seg-btn ${themeMode === m ? "hx-seg-btn--on" : ""}`} onClick={() => onTheme(m)}>
                {m === "light" ? "Light" : m === "dark" ? "Dark" : "System"}
              </button>
            ))}
          </span>
        }
      />
    </div>
  );
}
