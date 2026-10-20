import React from "react";
import ReactDOM from "react-dom/client";
import { SetupApp } from "./SetupApp";
import { markPlatform } from "../platform";
import "./setup.css";

// Linux has no native window blur — mark the platform so the glass
// surfaces render opaque (theme/tokens.css).
markPlatform();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <SetupApp />
  </React.StrictMode>
);
