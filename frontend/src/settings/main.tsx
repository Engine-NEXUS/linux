import React from "react";
import ReactDOM from "react-dom/client";
import { SettingsApp } from "./SettingsApp";
import { markPlatform } from "../platform";
import "./settings.css";

// Linux has no native window blur — mark the platform so the glass
// surfaces render opaque (theme/tokens.css).
markPlatform();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SettingsApp />
  </React.StrictMode>,
);
