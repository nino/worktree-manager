import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { App } from "./App";
import { CreationsProvider } from "./creations";
import { RunsProvider } from "./runs";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true, staleTime: 5_000 } },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RunsProvider>
        <CreationsProvider>
          <App />
        </CreationsProvider>
      </RunsProvider>
    </QueryClientProvider>
  </StrictMode>,
);
