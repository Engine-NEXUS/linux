import React from "react";
import ReactDOM from "react-dom/client";
import { ArchitectApp } from "./ArchitectApp";
import "./architect.css";

// Global error handlers — catch uncaught exceptions so they're visible
// in the CDP monitor (which captures Runtime.exceptionThrown)
window.addEventListener("error", (e) => {
  console.error("[architect] Uncaught error:", e.error || e.message);
});
window.addEventListener("unhandledrejection", (e) => {
  console.error("[architect] Unhandled promise rejection:", e.reason);
});

console.log("[architect] main.tsx loaded, mounting React app...");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ArchitectApp />
  </React.StrictMode>,
);
