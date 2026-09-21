import { createEffect, createSignal, on, onCleanup, onMount } from "solid-js";
import { drawScene, fitView, type View } from "../lib/draw.ts";
import { useRidge } from "../state.ts";

const ZOOM_STEP = 1.15;

/* The artwork canvas. Drag pans, the wheel zooms about the cursor, and a
 * double-click fits the figure again. */
export default function Stage() {
  const { raw, scene, status } = useRidge();
  let canvas!: HTMLCanvasElement;
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  // Unset until the user pans or zooms: the figure then stays fitted.
  const [panned, setPanned] = createSignal<View>();
  const [dragging, setDragging] = createSignal<{ x: number; y: number; view: View }>();
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
  const at = (e: MouseEvent) => {
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    return { x: (e.clientX - rect.left) * dpr, y: (e.clientY - rect.top) * dpr };
  };

  return (
    <div id="stage">
      <canvas
        id="canvas"
        ref={canvas}
        classList={{ dragging: !!dragging() }}
        onPointerDown={(e) => {
          const v = view();
          if (!v) return;
          setDragging({ ...at(e), view: v });
          canvas.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const d = dragging();
          if (!d) return;
          const p = at(e);
          setPanned({ ...d.view, tx: d.view.tx + p.x - d.x, ty: d.view.ty + p.y - d.y });
        }}
        onPointerUp={() => setDragging(undefined)}
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
      <div id="status" classList={{ busy: status().busy }}>{status().text}</div>
      <div id="hint">drag to pan · wheel to zoom · double-click to reset</div>
    </div>
  );
}
