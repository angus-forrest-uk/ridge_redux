import { createEffect } from "solid-js";
import { paramsFromHash } from "../lib/params.ts";
import { createRidgeState, RidgeContext } from "../state.ts";
import Controls from "./Controls.tsx";
import Header from "./Header.tsx";
import MapPanel from "./MapPanel.tsx";
import Stage from "./Stage.tsx";

export default function App() {
  const state = createRidgeState(paramsFromHash(location.hash));
  // Keep the URL a permalink to the current view.
  createEffect(() => history.replaceState(null, "", state.hash()));

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
