import { createRoot } from "react-dom/client";
import { CompanionHudApp } from "./CompanionHudApp";

import { initThemeSync } from "../sidebar/theme";
initThemeSync();
createRoot(document.getElementById("root")!).render(<CompanionHudApp />);
