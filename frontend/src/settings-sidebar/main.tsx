import React from "react";
import ReactDOM from "react-dom/client";
import { SettingsSidebarApp } from "./SettingsSidebarApp";
import "./settings-sidebar.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SettingsSidebarApp />
  </React.StrictMode>,
);
