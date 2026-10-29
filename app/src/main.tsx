import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { createAppClient } from "./api";
import "./styles.css";

const client = createAppClient();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App client={client} />
  </StrictMode>,
);
