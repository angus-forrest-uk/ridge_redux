import { createResource, For } from "solid-js";
import { exportSvg, fetchPresets } from "../lib/api.ts";
import { useRidge } from "../state.ts";
import ReadmeDialog from "./ReadmeDialog.tsx";

function download(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}

/* Rasterize the exported SVG at 2x the figure size. */
function svgToPng(svg: string, width: number, height: number): Promise<Blob> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = width * 2;
      canvas.height = height * 2;
      canvas.getContext("2d")!.drawImage(img, 0, 0, canvas.width, canvas.height);
      URL.revokeObjectURL(url);
      canvas.toBlob((png) => (png ? resolve(png) : reject(new Error("PNG encoding failed"))));
    };
    img.onerror = () => reject(new Error("could not load the SVG"));
    img.src = url;
  });
}

export default function Header() {
  const { params, scene, applyPreset } = useRidge();
  const [presets] = createResource(fetchPresets, { initialValue: [] });
  let readme!: { open: () => void };

  async function saveSvg() {
    try {
      download(new Blob([await exportSvg(params)], { type: "image/svg+xml" }), "ridge-map.svg");
    } catch {
      alert("export failed");
    }
  }

  async function savePng() {
    const layout = scene()?.layout;
    if (!layout) return;
    try {
      download(await svgToPng(await exportSvg(params), layout.width_px, layout.height_px), "ridge-map.png");
    } catch {
      alert("export failed");
    }
  }

  return (
    <header>
      <h1>ridge-redux</h1>
      <div class="header-actions">
        <select
          id="preset"
          onChange={(e) => {
            const preset = presets()[Number(e.currentTarget.value)];
            if (preset) applyPreset(preset);
            e.currentTarget.value = "";
          }}
        >
          <option value="">presets…</option>
          <For each={presets()}>{(preset, i) => <option value={i()}>{preset.name}</option>}</For>
        </select>
        <button id="export-svg" title="Download vector SVG" onClick={saveSvg}>SVG</button>
        <button id="export-png" title="Rasterize to PNG" onClick={savePng}>PNG</button>
        <button id="readme-open" title="Show the README" onClick={() => readme.open()}>README</button>
      </div>
      <ReadmeDialog ref={(api) => (readme = api)} />
    </header>
  );
}
