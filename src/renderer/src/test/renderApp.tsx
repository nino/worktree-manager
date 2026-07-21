import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { UserEvent } from "@testing-library/user-event";
import { App } from "../App";
import { CreationsProvider } from "../creations";

/** Mount the full app (real provider tree) with a fresh, retry-free QueryClient
 * and a userEvent instance. Tests query via the global `screen`. */
export function renderApp(): { user: UserEvent; queryClient: QueryClient } {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const user = userEvent.setup();
  render(
    <QueryClientProvider client={queryClient}>
      <CreationsProvider>
        <App />
      </CreationsProvider>
    </QueryClientProvider>,
  );
  return { user, queryClient };
}
