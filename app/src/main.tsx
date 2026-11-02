import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { createAppClient, createAppRuntimeConnection } from "./api";
import "./styles.css";

const client = createAppClient();
const runtime = createAppRuntimeConnection();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App client={client} runtime={runtime} />
  </StrictMode>,
);
