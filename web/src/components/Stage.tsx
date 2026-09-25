import { createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { LINE_SPACING, type Layout } from "../lib/pipeline.ts";
import { movedBbox } from "../lib/params.ts";
import { drawScene, fitView, type View } from "../lib/draw.ts";
import type { Bbox } from "../lib/pipeline.ts";
import { useRidge } from "../state.ts";

const ZOOM_STEP = 1.15;

type CanvasTool = "pan" | "move";

/* Degrees per canvas pixel from the figure layout: horizontal through the
 * axes rect (xlim is in window-cell units), vertical through the ridge
 * baselines (LINE_SPACING display units per row). Used when the map panel
 * hasn't published its own scale yet. */
function fallbackGeoScale(
  paramsBbox: Bbox, numLines: number, elevationPts: number,
  layout: Layout, scale: number,
): { latPerPx: number; lngPerPx: number } {
  const [w, s, e, n] = paramsBbox;
  const cellPerPxX = (layout.xlim[1] - layout.xlim[0]) / (layout.axes[2] - layout.axes[0]) / scale;
  const displayPerPxY = (layout.ylim[1] - layout.ylim[0]) / (layout.axes[3] - layout.axes[1]) / scale;
  return {
    lngPerPx: cellPerPxX * ((e - w) / Math.max(1, elevationPts - 1)),
    latPerPx: (displayPerPxY / LINE_SPACING) * ((n - s) / Math.max(1, numLines)),
  };
}

/* The artwork canvas. Drag pans or moves the area (toggle), the wheel
 * zooms about the cursor, and a double-click fits the figure again. */
export default function Stage() {
  const { raw, scene, status, params, setBbox, geoScale, setFollowMap, suspendFetch, resumeFetch } = useRidge();
  let canvas!: HTMLCanvasElement;
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  const [tool, setTool] = createSignal<CanvasTool>("pan");
  // Unset until the user pans or zooms: the figure then stays fitted.
  const [panned, setPanned] = createSignal<View>();
  const [panDrag, setPanDrag] = createSignal<{ x: number; y: number; view: View }>();
  // A move drag: the pointer start and the bbox it started from. The bbox
  // commits per frame (the scene re-windows the cached disc), fetches stay
  // suspended until release, and the map follows the moving selection.
  const [moveDrag, setMoveDrag] = createSignal<{ x: number; y: number; bbox: Bbox }>();
  // A move drag: the pointer start and the bbox it started from. The bbox
  // commits per frame (the scene re-windows the cached disc), fetches stay
  // suspended until release, and the map follows the moving selection.
  // Bumped when web fonts finish loading: the label is drawn in one (Cinzel),
  // and a canvas doesn't redraw by itself when it arrives.
  const [fontsLoaded, setFontsLoaded] = createSignal(0);

  const view = () => {
    const s = scene();
    return panned() ?? (s ? fitView(s, size().width, size().height) : undefined);
  };

  // New data: fit the new figure.
  createEffect(on(raw, () => setPanned(undefined), { defer: true }));

  onMount(() => {
    const ctx = canvas.getContext("2d")!;
    const observer = new ResizeObserver(([entry]) => {
      const dpr = window.devicePixelRatio || 1;
      setSize({
        width: Math.round(entry.contentRect.width * dpr),
        height: Math.round(entry.contentRect.height * dpr),
      });
    });
    observer.observe(canvas);
    onCleanup(() => observer.disconnect());

    const onFonts = () => setFontsLoaded((n) => n + 1);
    document.fonts.addEventListener("loadingdone", onFonts);
    onCleanup(() => document.fonts.removeEventListener("loadingdone", onFonts));

    createEffect(() => {
      const { width, height } = size();
      if (canvas.width !== width) canvas.width = width;
      if (canvas.height !== height) canvas.height = height;
      fontsLoaded();
      const s = scene(), v = view();
      if (s && v) drawScene(ctx, s, v);
    });
  });

  // Pointer position in canvas pixels.
  const at = (e: PointerEvent | MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    return { x: (e.clientX - rect.left) * dpr, y: (e.clientY - rect.top) * dpr };
  };

  return (
    <div id="stage">
      <canvas
        id="canvas"
        ref={canvas}
        classList={{
          dragging: !!panDrag(),
          "move-mode": tool() === "move",
          "move-dragging": !!moveDrag(),
        }}
        onPointerDown={(e) => {
          const p = at(e);
          if (tool() === "move") {
            if (!scene()) return;
            setMoveDrag({ x: p.x, y: p.y, bbox: [...params.bbox] as Bbox });
            setFollowMap(true);
            suspendFetch();
          } else {
            const v = view();
            if (!v) return;
            setPanDrag({ ...p, view: v });
          }
          canvas.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const md = moveDrag();
          if (md) {
            const g = geoScale() ?? fallbackGeoScale(
              md.bbox, params.num_lines, params.elevation_pts,
              scene()!.layout, view()!.scale,
            );
            const p = at(e);
            // Grab: the terrain follows the hand. Commits per frame, so
            // the scene re-windows the cached disc live; the map follows.
            setBbox(movedBbox(
              md.bbox,
              -(p.y - md.y) * g.latPerPx,
              (p.x - md.x) * g.lngPerPx,
            ));
            return;
          }
          const d = panDrag();
          if (!d) return;
          const p = at(e);
          setPanned({ ...d.view, tx: d.view.tx + p.x - d.x, ty: d.view.ty + p.y - d.y });
        }}
        onPointerUp={() => {
          if (moveDrag()) resumeFetch();
          setFollowMap(false);
          setMoveDrag(undefined);
          setPanDrag(undefined);
        }}
        onWheel={(e) => {
          e.preventDefault();
          const v = view();
          if (!v) return;
          const m = at(e);
          const scale = Math.min(40, Math.max(0.05, v.scale * (e.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP)));
          setPanned({
            scale,
            tx: m.x - ((m.x - v.tx) * scale) / v.scale,
            ty: m.y - ((m.y - v.ty) * scale) / v.scale,
          });
        }}
        onDblClick={() => setPanned(undefined)}
      />
      <div id="stage-tools" role="group" aria-label="canvas tool">
        <button
          id="stage-tool-pan"
          classList={{ active: tool() === "pan" }}
          aria-pressed={tool() === "pan"}
          title="Pan: drag to slide the view"
          onClick={() => setTool("pan")}
        >
          pan
        </button>
        <button
          id="stage-tool-move"
          classList={{ active: tool() === "move" }}
          aria-pressed={tool() === "move"}
          title="Move: drag to move the area — the selection follows, on the map too"
          onClick={() => setTool("move")}
        >
          move
        </button>
      </div>
      <div id="status" classList={{ busy: status().busy }}>{status().text}</div>
      <div id="hint">
        {tool() === "move"
          ? "drag to move the area · wheel to zoom · double-click to reset"
          : "drag to pan · wheel to zoom · double-click to reset"}
      </div>
    </div>
  );
}
