import React from "react";
import ReactDOM from "react-dom/client";
import { NexusProvider } from "./context/NexusContext";
import { ThemeProvider } from "./context/ThemeContext";
import App from "./App";
import "./styles/globals.css";

// Log build stamp on app start — proves which build is running
console.log(
  `%c[Electron] v${__APP_VERSION__} | build ${__BUILD_HASH__} | ${__BUILD_TIME__} | Tauri ${__TAURI_VERSION__}`,
  "color: #06b6d4; font-weight: bold",
);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemeProvider>
      <NexusProvider>
        <App />
      </NexusProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
