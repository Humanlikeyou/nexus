import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles/tokens.css";
import "./styles/motion.css";
import "./styles/emerald.css";
import "./styles/glass.css";
import "./styles/legacy-aliases.css";
import "./styles/wordmark.css";
import App from "./App";
import "./styles.css";
import "./styles/glass-apply.css";
import "./styles/glass-motion.css";
import "./styles/route-motion.css";
import "./styles/calm.css";
import { installReduceTransparency } from "./shell/reduceTransparency";
import { installVibrancy } from "./shell/vibrancy";

// Before the first render, so the transparent window never paints a frame with the wrong background.
installVibrancy();
installReduceTransparency();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
