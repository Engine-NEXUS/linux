import React from "react";
import ReactDOM from "react-dom/client";
import { SidebarApp } from "./SidebarApp";
import "./sidebar.css";

// Wayland ignores set_position: window lands compositor-centered with no
// native blur and no backdrop capture. Mark platform so CSS renders a
// solid floating panel instead of the translucent Windows look.
if (/linux/i.test(navigator.userAgent) || /linux/i.test(navigator.platform ?? "")) {
  document.body.dataset.platform = "linux";
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SidebarApp />
  </React.StrictMode>,
);
