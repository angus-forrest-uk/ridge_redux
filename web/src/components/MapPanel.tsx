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
  const { params, setBbox, recenter, tiles, setGeoScale, suspendFetch, resumeFetch } = useRidge();
  const [tool, setTool] = createSignal<MapTool>("move");
  const [open, setOpen] = createSignal(true);
  const [drawStart, setDrawStart] = createSignal<LatLng>();
  // A move drag: the bbox it started from, the map center it started
  // from, and whether the map actually panned yet. The drag IS a Leaflet
  // pan — the basemap and the rectangle slide with the hand — and each
  // pan step translates the bbox by the center delta.
  const [moveStart, setMoveStart] = createSignal<{
    bbox: Bbox;
    center: LatLng;
    moved: boolean;
    finishing: boolean;
  }>();
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

    // Publish the map's degrees per screen pixel: the canvas move-drag
    // travels at this scale, so the same gesture moves the selection the
    // same amount on both surfaces. getBounds throws before the map has a
    // view (fitBounds comes later), so swallow that first call.
    const publishGeoScale = () => {
      try {
        const b = map.getBounds(), size = map.getSize();
        if (size.x < 1 || size.y < 1) return;
        setGeoScale({
          latPerPx: Math.abs(b.getNorth() - b.getSouth()) / size.y,
          lngPerPx: Math.abs(b.getEast() - b.getWest()) / size.x,
        });
      } catch {
        /* no view yet */
      }
    };
    map.on("move zoom viewreset resize", publishGeoScale);
    map.whenReady(publishGeoScale);

    // SRTM covers only 60S..60N; shade the rest. The outer corner sits at
    // the projection's own limit: 90° has no Mercator coordinates, and a
    // polygon corner there silently fails to render.
    const MERCATOR_MAX_LAT = 85.0511;
    const excluded = { className: "excluded-zone", interactive: false, stroke: false };
    const edge = { className: "excluded-edge", interactive: false };
    for (const sign of [1, -1]) {
      L.polygon(
        [
          [SRTM_LAT_MAX * sign, -180],
          [SRTM_LAT_MAX * sign, 180],
          [MERCATOR_MAX_LAT * sign, 180],
          [MERCATOR_MAX_LAT * sign, -180],
        ],
        excluded,
      )
        .addTo(map)
        .bindTooltip(`no SRTM data ${sign > 0 ? "above 60°N" : "below 60°S"}`, {
          permanent: true,
          direction: "center",
          className: "zone-label",
        });
      L.polyline(
        [
          [SRTM_LAT_MAX * sign, -180],
          [SRTM_LAT_MAX * sign, 180],
        ],
        edge,
      ).addTo(map);
    }

    // Green shading for the tiles the server has locally; refreshed after
    // each fetch, so the ring around the selection lights up as it moves.
    const tileShade = L.layerGroup().addTo(map);
    createEffect(() => {
      tileShade.clearLayers();
      for (const [latLo, lonLo] of tiles()) {
        L.rectangle(
          [
            [latLo, lonLo],
            [latLo + 1, lonLo + 1],
          ],
          { className: "loaded-tile", interactive: false },
        ).addTo(tileShade);
      }
    });

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
    // Panning is the move gesture itself; only the select tool and an
    // active draw need the map pinned.
    createEffect(() => {
      if (tool() === "select" || drawStart()) map.dragging.disable();
      else map.dragging.enable();
    });
    // The drag pans the map natively (inertia included); the selection
    // rides the pan, and the fetch resumes when the map settles.
    map.on("move", () => {
      const move = moveStart();
      if (!move || move.finishing) return;
      const c = map.getCenter();
      const dLat = c.lat - move.center.lat;
      const dLng = c.lng - move.center.lng;
      if (!dLat && !dLng) return;
      move.center = { lat: c.lat, lng: c.lng };
      move.moved = true;
      setBbox(movedBbox(move.bbox, dLat, dLng));
    });
    map.on("moveend", () => {
      if (!moveStart()) return;
      setMoveStart(undefined);
      resumeFetch();
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
      // A drag inside the selection (move tool) pans the map and carries
      // the selection with it; Shift still draws.
      if (!e.originalEvent.shiftKey && insideSelection(lat, lng) && tool() === "move") {
        const c = map.getCenter();
        setMoveStart({
          bbox: [...params.bbox] as Bbox,
          center: { lat: c.lat, lng: c.lng },
          moved: false,
          finishing: false,
        });
        suspendFetch();
        return;
      }
      if (!startsSelection(tool(), e.originalEvent.shiftKey)) return;
      const start = { lat, lng };
      setDrawStart(start);
      rect.setBounds(L.latLngBounds(start, start));
    });
    map.on("mousemove", (e) => {
      const start = drawStart();
      if (start) {
        const lat = clampLat(e.latlng.lat), lng = clampLng(e.latlng.lng);
        rect.setBounds(L.latLngBounds(start, { lat, lng }));
        return;
      }
      setOverSelection(
        !moveStart() && insideSelection(clampLat(e.latlng.lat), clampLng(e.latlng.lng)),
      );
    });
    map.on("mouseup", (e) => {
      const move = moveStart();
      if (move) {
        if (move.moved) {
          // Inertia may still be sliding the map: moveend finishes it.
          setMoveStart({ ...move, finishing: true });
        } else {
          setMoveStart(undefined);
          resumeFetch();
        }
        return;
      }
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
        <div id="map-hint">gray = beyond SRTM coverage (±60°) · green = tiles on disk · map data © OpenStreetMap</div>
      </div>
    </section>
  );
}
