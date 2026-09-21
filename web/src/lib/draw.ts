/* Paint a scene onto a 2D canvas, the same way matplotlib draws upstream's
 * figure: rows back to front, each filled down to its baseline in the
 * background color (the fill_between occlusion trick), then stroked. */
import { cmapCss, cssColor } from "./colors.ts";
import { computeRuns } from "./pipeline.ts";
import type { Scene } from "./scene.ts";

/* Figure pixels to canvas pixels. */
export interface View {
  scale: number;
  tx: number;
  ty: number;
}

const ptToPx = (pt: number) => (pt / 72) * 100; // pt at 100 dpi -> figure px
const ELEVATION_BUCKETS = 24;

/* Center the figure in the canvas at 95% of the largest scale that fits. */
export function fitView(scene: Scene, width: number, height: number): View {
  const fw = scene.layout.width_px, fh = scene.layout.height_px;
  const scale = Math.min(width / fw, height / fh) * 0.95;
  return { scale, tx: (width - fw * scale) / 2, ty: (height - fh * scale) / 2 };
}

export function drawScene(ctx: CanvasRenderingContext2D, scene: Scene, view: View) {
  const { width_px: fw, height_px: fh, axes, xlim, ylim } = scene.layout;
  const { style } = scene;
  const background = cssColor(style.background);

  // Data -> figure px (same mapping as the backend's FigureLayout::to_px).
  const toFig = (x: number, y: number): [number, number] => [
    axes[0] + ((x - xlim[0]) / (xlim[1] - xlim[0])) * (axes[2] - axes[0]),
    axes[1] + (1 - (y - ylim[0]) / (ylim[1] - ylim[0])) * (axes[3] - axes[1]),
  ];

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.setTransform(view.scale, 0, 0, view.scale, view.tx, view.ty);

  ctx.fillStyle = background;
  ctx.fillRect(0, 0, fw, fh);

  // Clip to the axes rect, as matplotlib does.
  ctx.save();
  ctx.beginPath();
  ctx.rect(axes[0], axes[1], axes[2] - axes[0], axes[3] - axes[1]);
  ctx.clip();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.lineWidth = ptToPx(style.linewidth_pt);

  const { rows } = scene;
  rows.forEach((row, i) => {
    const runs = computeRuns(row.y);
    if (runs.length === 0) return;

    // Fill from the baseline up to the curve, per run.
    ctx.fillStyle = background;
    for (const [a, b] of runs) {
      ctx.beginPath();
      const [x0, yb] = toFig(a, row.baseline);
      ctx.moveTo(x0, yb);
      for (let k = a; k < b; k++) ctx.lineTo(...toFig(k, row.y[k]));
      ctx.lineTo(toFig(b - 1, row.baseline)[0], yb);
      ctx.closePath();
      ctx.fill();
    }

    if (style.kind === "elevation" && style.line.type === "map") {
      // Color each segment by its height, batched into a few paths.
      const span = scene.vmax - scene.vmin || 1;
      const paths: (Path2D | undefined)[] = [];
      for (const [a, b] of runs) {
        for (let k = a + 1; k < b; k++) {
          const t = Math.min(ELEVATION_BUCKETS - 1, Math.max(0,
            Math.floor(((row.y[k - 1] - row.baseline - scene.vmin) / span) * ELEVATION_BUCKETS)));
          const path = (paths[t] ??= new Path2D());
          path.moveTo(...toFig(k - 1, row.y[k - 1]));
          path.lineTo(...toFig(k, row.y[k]));
        }
      }
      paths.forEach((path, t) => {
        if (!path) return;
        ctx.strokeStyle = cmapCss(style.line.type === "map" ? style.line.name : "", (t + 0.5) / ELEVATION_BUCKETS);
        ctx.stroke(path);
      });
    } else {
      ctx.strokeStyle = style.line.type === "solid"
        ? cssColor(style.line.rgb)
        : cmapCss(style.line.name, rows.length <= 1 ? 0 : i / (rows.length - 1));
      for (const [a, b] of runs) {
        if (b - a < 2) continue;
        ctx.beginPath();
        ctx.moveTo(...toFig(a, row.y[a]));
        for (let k = a + 1; k < b; k++) ctx.lineTo(...toFig(k, row.y[k]));
        ctx.stroke();
      }
    }
  });
  ctx.restore();

  const { label, annotation } = style;
  if (label) {
    const fs = ptToPx(label.size_pt);
    ctx.font = `${fs}px "${label.font_family}", serif`;
    ctx.textBaseline = "alphabetic";
    const lines = label.text.split("\n");
    const lh = fs * 1.2;
    const bx = axes[0] + label.x * (axes[2] - axes[0]);
    const by = axes[1] + label.y * (axes[3] - axes[1]) - (lines.length - 1) * lh - fs * 0.15;
    if (label.background) {
      const widest = Math.max(...lines.map((l) => ctx.measureText(l).width));
      const pad = fs * 0.25;
      ctx.fillStyle = background;
      ctx.fillRect(bx - pad, by - fs * 0.85 - pad, widest + 2 * pad, lines.length * lh + 1.4 * pad);
    }
    ctx.fillStyle = cssColor(label.color);
    lines.forEach((text, k) => ctx.fillText(text, bx, by + k * lh));
  }

  if (annotation) {
    const ax = axes[0] + annotation.x * (axes[2] - axes[0]);
    const ay = axes[1] + (1 - annotation.y) * (axes[3] - axes[1]);
    ctx.fillStyle = cssColor(annotation.color);
    ctx.beginPath();
    ctx.arc(ax, ay, ptToPx(annotation.dot_pt) / 2, 0, Math.PI * 2);
    ctx.fill();
    if (annotation.label) {
      const fs = ptToPx(annotation.label_size_pt);
      ctx.font = `${fs}px "Cinzel", serif`;
      const lx = axes[0] + (annotation.x + annotation.x_offset) * (axes[2] - axes[0]);
      const ly = axes[1] + (1 - annotation.y - annotation.y_offset) * (axes[3] - axes[1]);
      ctx.fillText(annotation.label, lx, ly - fs * 0.15);
    }
  }
}
