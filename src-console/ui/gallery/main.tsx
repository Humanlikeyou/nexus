import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../../styles/tokens.css";
import "./gallery.css";
import { Gallery } from "./Gallery";

// Dev-only page (ui-gallery.html, not in the production build).
//   /ui-gallery.html                 light and dark side by side (two iframes)
//   /ui-gallery.html?theme=dark      one theme; add &motion=reduce for data-reduce-motion
const params = new URLSearchParams(window.location.search);
const theme = params.get("theme");
const root = document.documentElement;
if (params.get("motion") === "reduce") root.dataset.reduceMotion = "true";

function SideBySide() {
  const motion = params.get("motion") === "reduce" ? "&motion=reduce" : "";
  return (
    <div className="ui-gallery-split">
      <iframe title="Light theme" src={`?theme=light${motion}`} />
      <iframe title="Dark theme" src={`?theme=dark${motion}`} />
    </div>
  );
}

if (theme === "light" || theme === "dark") root.dataset.theme = theme;
createRoot(document.getElementById("root")!).render(
  <StrictMode>{theme === "light" || theme === "dark" ? <Gallery theme={theme} /> : <SideBySide />}</StrictMode>,
);
