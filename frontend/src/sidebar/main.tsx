import React from "react";
import ReactDOM from "react-dom/client";
import { UnifiedSidebar } from "./UnifiedSidebar";
import { markPlatform } from "../platform";
// View styles — all four views live in this ONE window now.
import "./sidebar.css";
import "./unified.css";
import "../settings-sidebar/settings-sidebar.css";
import "../architect/architect.css";
import "../pr-list/pr-list.css";

// Global error handlers — catch uncaught exceptions so they're visible
// in the CDP monitor (which captures Runtime.exceptionThrown)
window.addEventListener("error", (e) => {
  console.error("[sidebar] Uncaught error:", e.error || e.message);
});
window.addEventListener("unhandledrejection", (e) => {
  console.error("[sidebar] Unhandled promise rejection:", e.reason);
});

// Wayland ignores set_position: window lands compositor-centered with no
// native blur and no backdrop capture. Mark platform so CSS renders a
// solid floating panel instead of the translucent Windows look.
markPlatform();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <UnifiedSidebar />
  </React.StrictMode>,
);
