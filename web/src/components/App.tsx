import { createEffect, onCleanup } from "solid-js";
import { paramsFromHash } from "../lib/params.ts";
import { createRidgeState, RidgeContext } from "../state.ts";
import Controls from "./Controls.tsx";
import Header from "./Header.tsx";
import MapPanel from "./MapPanel.tsx";
import Stage from "./Stage.tsx";

const PERMALINK_DEBOUNCE_MS = 500;

export default function App() {
  const state = createRidgeState(paramsFromHash(location.hash));
  // Keep the URL a permalink to the current view. Debounced: move drags
  // commit per frame, and browsers cap replaceState (WebKit throws past
  // 100 calls per 10 seconds).
  let hashTimer: ReturnType<typeof setTimeout> | undefined;
  createEffect(() => {
    const hash = state.hash();
    clearTimeout(hashTimer);
    hashTimer = setTimeout(() => history.replaceState(null, "", hash), PERMALINK_DEBOUNCE_MS);
  });
  onCleanup(() => clearTimeout(hashTimer));

  return (
    <RidgeContext.Provider value={state}>
      <div id="app">
        <Header />
        <main>
          <Stage />
          <Controls />
        </main>
        <MapPanel />
      </div>
    </RidgeContext.Provider>
  );
}
