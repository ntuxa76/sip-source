import "@fontsource-variable/ibm-plex-sans";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import "./index.css";

import { QueryClient, QueryClientProvider, useQuery, useQueryClient } from "@tanstack/react-query";
import { RouterProvider, createRootRoute, createRoute, createRouter, useParams } from "@tanstack/react-router";
import { StrictMode, Suspense, lazy, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { Shell } from "./components/Shell";
import { Loading } from "./components/ui";
import { ApiError, get } from "./lib/api";
import { PrefsProvider } from "./lib/context";
import { AdminPage, PreferencesPage } from "./pages/Admin";
import { CallPage } from "./pages/Call";
import { DashboardList, DashboardView, ExplorePage, HomePage } from "./pages/Dashboards";
import { LivePage } from "./pages/Live";
import { LoginPage, SetupPage } from "./pages/Login";
import { SearchPage } from "./pages/Search";
import { SubscriberPage, SubscribersPage } from "./pages/Subscribers";

// CodeMirror is only needed on this one page, so it stays out of the initial bundle.
const WorkbenchPage = lazy(() => import("./pages/Workbench").then((m) => ({ default: m.WorkbenchPage })));

const looseSearch = (s: Record<string, unknown>) => s as Record<string, string>;
const root = createRootRoute({ component: Shell, validateSearch: looseSearch });
const child = (path: string, component: () => React.JSX.Element) => createRoute({ getParentRoute: () => root, path, component, validateSearch: looseSearch });

function Dashboard() {
  const { id } = useParams({ strict: false }) as { id: string };
  return <DashboardView key={id} id={id} />;
}

function Call() {
  const { uid } = useParams({ strict: false }) as { uid: string };
  return <CallPage key={uid} uid={uid} />;
}

function Subscriber() {
  const { number } = useParams({ strict: false }) as { number: string };
  return <SubscriberPage key={number} number={number} />;
}

function Admin() {
  const { section } = useParams({ strict: false }) as { section: string };
  return <AdminPage section={section} />;
}

function Workbench() {
  return (
    <Suspense fallback={<Loading page label="Opening the SQL workbench…" />}>
      <WorkbenchPage />
    </Suspense>
  );
}

const routeTree = root.addChildren([
  child("/", HomePage),
  child("/search", SearchPage),
  child("/live", LivePage),
  child("/dashboards", DashboardList),
  child("/dashboards/$id", Dashboard),
  child("/explore", ExplorePage),
  child("/subscribers", SubscribersPage),
  child("/subscribers/$number", Subscriber),
  child("/calls/$uid", Call),
  child("/sql", Workbench),
  child("/admin/$section", Admin),
  child("/preferences", PreferencesPage),
]);

const router = createRouter({ routeTree, defaultPreload: false });
const queryClient = new QueryClient({ defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 5000 } } });

/** Decides between the first-run screen, the login page and the app. */
function Gate() {
  const client = useQueryClient();
  const state = useQuery({ queryKey: ["auth-state"], queryFn: () => get("/auth/state") });
  const me = useQuery({ queryKey: ["me"], queryFn: () => get("/auth/me"), retry: false, enabled: state.data !== undefined && !state.data.setup_required });

  useEffect(() => {
    const signedOut = () => client.invalidateQueries({ queryKey: ["me"] });
    window.addEventListener("ss-signed-out", signedOut);
    return () => window.removeEventListener("ss-signed-out", signedOut);
  }, [client]);

  if (state.error) return <div className="p-6 text-crit">Can't reach the server: {(state.error as Error).message}</div>;
  if (!state.data) return <Loading page label="Starting sip-source…" />;
  if (state.data.setup_required) return <SetupPage settings={state.data.settings} tokenRequired={state.data.setup_token_required} />;
  if (me.error && (me.error as ApiError).status === 401) return <LoginPage settings={state.data.settings} sso={state.data.sso} />;
  if (me.error) return <div className="p-6 text-crit">{(me.error as Error).message}</div>;
  if (!me.data) return <Loading page label="Signing in…" work="connect" />;
  return (
    <PrefsProvider me={me.data}>
      <RouterProvider router={router} />
    </PrefsProvider>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <Gate />
    </QueryClientProvider>
  </StrictMode>,
);
