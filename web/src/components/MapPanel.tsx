import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import {
  clampLat, clampLng, movedBbox, selectionBbox, SRTM_LAT_MAX, startsSelection, type LatLng, type MapTool,
} from "../lib/params.ts";
import type { Bbox } from "../lib/pipeline.ts";
import { useRidge } from "../state.ts";

const WORLD: L.LatLngBoundsExpression = [[-90, -180], [90, 180]];
const toBounds = ([lon0, lat0, lon1, lat1]: Bbox): L.LatLngBoundsExpression => [
  [clampLat(lat0), clampLng(lon0)],
  [clampLat(lat1), clampLng(lon1)],
];

/* The location map: pan it with the move tool, drag out the area to render
 * with the select tool, and drag the selection to move it as-is. */
export default function MapPanel() {
  const { params, setBbox, recenter } = useRidge();
  const [tool, setTool] = createSignal<MapTool>("move");
  const [open, setOpen] = createSignal(true);
  const [drawStart, setDrawStart] = createSignal<LatLng>();
  // A move drag: where it started, and the bbox it started from.
  const [moveStart, setMoveStart] = createSignal<{ at: LatLng; bbox: Bbox }>();
  const [overSelection, setOverSelection] = createSignal(false);
  let container!: HTMLDivElement;

  onMount(() => {
    // A single copy of the world: no wrapping, and panning stops at its edges,
    // so every point on the map is one real longitude for the selection.
    const map = L.map(container, { boxZoom: false, maxBounds: WORLD, maxBoundsViscosity: 1 });
    onCleanup(() => map.remove());
    L.tileLayer("https://tile.openstreetmap.org/{z}/{x}/{y}.png", {
      noWrap: true,
      bounds: WORLD,
      maxZoom: 13,
      attribution: '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a>',
    }).addTo(map);

    // Never zoom out past the point where the world is narrower than the panel.
    const fitWorldWidth = () => map.setMinZoom(Math.max(0, Math.ceil(Math.log2(map.getSize().x / 256))));
    map.on("resize", fitWorldWidth);

    // SRTM covers only 60S..60N; shade the rest.
    const excluded = { className: "excluded-zone", interactive: false, stroke: false };
    L.polygon([[SRTM_LAT_MAX, -180], [SRTM_LAT_MAX, 180], [90, 180], [90, -180]], excluded)
      .addTo(map).bindTooltip("no SRTM data above 60°N");
    L.polygon([[-SRTM_LAT_MAX, -180], [-SRTM_LAT_MAX, 180], [-90, 180], [-90, -180]], excluded)
      .addTo(map).bindTooltip("no SRTM data below 60°S");

    const rect = L.rectangle(toBounds(params.bbox), { className: "bbox-rect", interactive: false }).addTo(map);

    // The rectangle follows the bbox, except while a new one is being drawn.
    createEffect(() => {
      if (!drawStart()) rect.setBounds(toBounds(params.bbox));
    });
    // Center on the bbox at load and for each preset.
    createEffect(on(recenter, () => {
      map.fitBounds(toBounds(params.bbox), { padding: [12, 12] });
      fitWorldWidth();
    }));
    // Panning belongs to the move tool, and pauses for any map drag.
    createEffect(() => {
      if (tool() === "select" || drawStart() || moveStart()) map.dragging.disable();
      else map.dragging.enable();
    });
    // Releasing the button off the map must still end a move drag.
    createEffect(() => {
      if (!moveStart()) return;
      const commit = () => setMoveStart(undefined);
      document.addEventListener("mouseup", commit);
      onCleanup(() => document.removeEventListener("mouseup", commit));
    });
    // Leaflet has to re-measure once the panel is shown again.
    createEffect(on(open, (isOpen) => {
      if (isOpen) setTimeout(() => map.invalidateSize(), 60);
    }, { defer: true }));

    const insideSelection = (lat: number, lng: number) => {
      const [w, s, e, n] = params.bbox;
      return lng >= w && lng <= e && lat >= s && lat <= n;
    };

    map.on("mousedown", (e) => {
      const lat = clampLat(e.latlng.lat), lng = clampLng(e.latlng.lng);
      // A drag inside the selection moves it as-is; Shift still draws.
      if (!e.originalEvent.shiftKey && insideSelection(lat, lng)) {
        setMoveStart({ at: { lat, lng }, bbox: [...params.bbox] as Bbox });
        return;
      }
      if (!startsSelection(tool(), e.originalEvent.shiftKey)) return;
      const start = { lat, lng };
      setDrawStart(start);
      rect.setBounds(L.latLngBounds(start, start));
    });
    map.on("mousemove", (e) => {
      const lat = clampLat(e.latlng.lat), lng = clampLng(e.latlng.lng);
      const move = moveStart();
      if (move) {
        // Commit as it goes: the store debounces the refetch, so the
        // scene follows a pause in the drag without waiting for mouseup.
        setBbox(movedBbox(move.bbox, e.latlng.lat - move.at.lat, e.latlng.lng - move.at.lng));
        return;
      }
      const start = drawStart();
      if (start) {
        rect.setBounds(L.latLngBounds(start, { lat, lng }));
        return;
      }
      setOverSelection(insideSelection(lat, lng));
    });
    map.on("mouseup", (e) => {
      setMoveStart(undefined);
      const start = drawStart();
      if (!start) return;
      setDrawStart(undefined);
      const bbox = selectionBbox(start, e.latlng);
      if (bbox) setBbox(bbox);
    });
  });

  return (
    <section id="map-panel" classList={{ closed: !open() }}>
      <div id="map-panel-head" title="click to collapse / expand" onClick={() => setOpen(!open())}>
        <span>location map, move to explore, select to pick the area, drag the selection to move it</span>
        <button id="map-toggle">{open() ? "hide" : "show"}</button>
      </div>
      <div id="map-body">
        <div
          id="map"
          ref={container}
          classList={{
            "select-mode": tool() === "select",
            "leaflet-drawing": !!drawStart(),
            "over-selection": overSelection() && !moveStart() && !drawStart(),
            "moving-selection": !!moveStart(),
          }}
        />
        <div id="map-tools" role="group" aria-label="map tool">
          <button
            id="map-tool-move"
            classList={{ active: tool() === "move" }}
            aria-pressed={tool() === "move"}
            title="Move: drag to pan the map"
            onClick={() => setTool("move")}
          >
            move
          </button>
          <button
            id="map-tool-select"
            classList={{ active: tool() === "select" }}
            aria-pressed={tool() === "select"}
            title="Select: drag to draw the area (or hold Shift in move mode)"
            onClick={() => setTool("select")}
          >
            select
          </button>
        </div>
        <div id="map-hint">gray = beyond SRTM coverage (±60° latitude) · tiles © OpenStreetMap</div>
      </div>
    </section>
  );
}
