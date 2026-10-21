/**
 * Feature 88 (C2) — identity status → setup-wizard banner derivation.
 * Pure + unit-tested; SetupApp and the settings identity card share it.
 */

export interface IdentityBanner {
  tone: "amber" | "green" | "red" | "dim";
  title: string;
  subtitle: string;
}

export interface IdentityLike {
  state: string;
  reason?: string | null;
}

export function identityBanner(status: IdentityLike | null | undefined): IdentityBanner {
  if (!status) {
    return {
      tone: "dim",
      title: "Checking cloud access...",
      subtitle: "Contacting the NEXUS cloud.",
    };
  }
  switch (status.state) {
    case "approved":
      return {
        tone: "green",
        title: "Cloud connected",
        subtitle: "Full NEXUS cloud features are active on this laptop.",
      };
    case "pending":
      return {
        tone: "amber",
        title: "Awaiting admin approval",
        subtitle: "Local features work now. Cloud features unlock once the admin approves this device.",
      };
    case "provisional":
      return {
        tone: "dim",
        title: "Cloud unavailable",
        subtitle: status.reason === "rate_limited"
          ? "Too many registration attempts — will retry automatically."
          : "NEXUS will retry connecting automatically. Everything local works meanwhile.",
      };
    case "suspended":
      return {
        tone: "red",
        title: "Cloud access suspended",
        subtitle: "Please contact your administrator to restore access.",
      };
    case "revoked":
      return {
        tone: "red",
        title: "Cloud access revoked",
        subtitle: "This device's cloud access has been revoked. Reinstall to register a new profile.",
      };
    case "expired":
      return {
        tone: "red",
        title: "Cloud access expired",
        subtitle: "Please ask your administrator to renew this device's grant.",
      };
    default:
      return {
        tone: "red",
        title: "Cloud access not enabled",
        subtitle: "Cloud access isn't enabled for this device yet, sir.",
      };
  }
}
