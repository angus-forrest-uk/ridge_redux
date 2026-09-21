//! Server-side SVG rendering of a [`RidgeScene`] — the matplotlib-free
//! replacement for `RidgeMap.plot_map` / `plot_annotation`.
//!
//! The same scene renders in the web frontend's `<canvas>`; this module is
//! for final (vector, print-ready) artwork export.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::colormap::Rgb;
use crate::geometry::{ColorKind, FigureLayout, RidgeScene, FIG_DPI};
fn hex3(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Upstream `label_verticalalignment`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VAlign {
    Top,
    Bottom,
}

/// The big `plot_map` label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelStyle {
    pub text: String,
    pub color: Rgb,
    /// Axes-fraction coordinates (0..1), like upstream `label_x` / `label_y`.
    pub x: f64,
    pub y: f64,
    pub size_pt: f64,
    pub vertical_alignment: VAlign,
    pub font_family: String,
    pub background: bool,
}

/// A `plot_annotation` marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotation {
    pub label: String,
    /// Axes-fraction coordinates of the dot.
    pub x: f64,
    pub y: f64,
    /// Label offset in axes fractions, like upstream `x_offset` / `y_offset`.
    pub x_offset: f64,
    pub y_offset: f64,
    pub label_size_pt: f64,
    /// Marker size in points (`annotation_size`).
    pub dot_pt: f64,
    pub color: Rgb,
    pub background: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotStyle {
    pub line: LineColorSpec,
    pub kind: ColorKind,
    pub background: Rgb,
    /// Line width in points (matplotlib `linewidth`).
    pub linewidth_pt: f64,
    /// Figure width in inches (upstream `size_scale`).
    pub size_scale: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<LabelStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation: Option<Annotation>,
}

/// Serde-friendly mirror of `LineColor`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum LineColorSpec {
    Solid { rgb: Rgb },
    Map { name: crate::colormap::Colormap },
}

impl From<&crate::colormap::LineColor> for LineColorSpec {
    fn from(l: &crate::colormap::LineColor) -> Self {
        match l {
            crate::colormap::LineColor::Solid(rgb) => LineColorSpec::Solid { rgb: *rgb },
            crate::colormap::LineColor::Map(cm) => LineColorSpec::Map { name: *cm },
        }
    }
}

impl LineColorSpec {
    fn to_line(&self) -> crate::colormap::LineColor {
        match self {
            LineColorSpec::Solid { rgb } => crate::colormap::LineColor::Solid(*rgb),
            LineColorSpec::Map { name } => crate::colormap::LineColor::Map(*name),
        }
    }
}

/// Render the scene to a standalone SVG document.
pub fn render_svg(scene: &RidgeScene, style: &PlotStyle) -> String {
    let layout = &scene.layout;
    let line = style.line.to_line();
    let lw_px = style.linewidth_pt / 72.0
        * FIG_DPI
        * (layout.width_px / (style.size_scale * FIG_DPI)).max(1e-9);
    // ^ linewidth scales with the figure just like matplotlib points do.

    let mut s = String::with_capacity(1 << 20);
    let _ = writeln!(
        s,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#,
        w = layout.width_px,
        h = layout.height_px,
    );

    // Background.
    let bg = hex3(style.background);
    let _ = writeln!(
        s,
        r#"  <rect x="0" y="0" width="{w}" height="{h}" fill="{bg}" />"#,
        w = layout.width_px,
        h = layout.height_px
    );

    // Clip everything to the axes rect (matplotlib clips to the axes).
    let _ = writeln!(
        s,
        r#"  <defs><clipPath id="axes"><rect x="{x}" y="{y}" width="{cw}" height="{ch}" /></clipPath></defs>"#,
        x = layout.axes[0],
        y = layout.axes[1],
        cw = layout.axes[2] - layout.axes[0],
        ch = layout.axes[3] - layout.axes[1],
    );
    let _ = writeln!(s, r#"  <g clip-path="url(#axes)">"#);

    for (idx, row) in scene.rows.iter().enumerate() {
        let runs = row.runs();
        if runs.is_empty() {
            continue;
        }
        // Fill polygons (baseline -> curve -> baseline), one per run.
        let fill_path = runs
            .iter()
            .map(|&(a, b)| {
                let mut d = String::new();
                let (x0, y_base) = layout.to_px(a as f64, row.baseline);
                let _ = write!(d, "M {x0:.2} {y_base:.2}");
                for i in a..b {
                    let (x, y) = layout.to_px(i as f64, row.y[i]);
                    let _ = write!(d, " L {x:.2} {y:.2}");
                }
                let (x1, _) = layout.to_px((b - 1) as f64, row.baseline);
                let _ = write!(d, " L {x1:.2} {y_base:.2} Z");
                d
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            s,
            r#"    <path d="{fill_path}" fill="{bg}" stroke="none" />"#
        );

        // Strokes.
        match (style.kind, &line) {
            (ColorKind::Elevation, crate::colormap::LineColor::Map(_)) => {
                for &(a, b) in &runs {
                    let mut d = String::new();
                    let (mut px, mut py) = layout.to_px(a as f64, row.y[a]);
                    let _ = write!(d, "M {px:.2} {py:.2}");
                    for i in (a + 1)..b {
                        let color = scene.elevation_color(&line, row.y[i - 1] - row.baseline);
                        let (x, y) = layout.to_px(i as f64, row.y[i]);
                        let _ = write!(d, " L {x:.2} {y:.2}");
                        let _ = writeln!(
                            s,
                            r#"    <path d="{}" fill="none" stroke="{}" stroke-width="{lw:.3}" stroke-linecap="round" stroke-linejoin="round" />"#,
                            d,
                            hex3(color),
                            lw = lw_px
                        );
                        d = format!("M {x:.2} {y:.2}");
                        px = x;
                        py = y;
                    }
                    let _ = (px, py);
                }
            }
            _ => {
                let color = scene.gradient_color(&line, idx);
                let path = runs
                    .iter()
                    .filter_map(|&(a, b)| {
                        if b - a < 2 {
                            return None;
                        }
                        let mut d = String::new();
                        let (x, y) = layout.to_px(a as f64, row.y[a]);
                        let _ = write!(d, "M {x:.2} {y:.2}");
                        for i in (a + 1)..b {
                            let (x, y) = layout.to_px(i as f64, row.y[i]);
                            let _ = write!(d, " L {x:.2} {y:.2}");
                        }
                        Some(d)
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                if !path.is_empty() {
                    let _ = writeln!(
                        s,
                        r#"    <path d="{path}" fill="none" stroke="{color}" stroke-width="{lw:.3}" stroke-linecap="round" stroke-linejoin="round" />"#,
                        color = hex3(color),
                        lw = lw_px
                    );
                }
            }
        }
    }
    let _ = writeln!(s, "  </g>");

    if let Some(label) = &style.label {
        render_label(&mut s, layout, label, &bg);
    }
    if let Some(ann) = &style.annotation {
        render_annotation(&mut s, layout, ann, &bg);
    }

    let _ = writeln!(s, "</svg>");
    s
}

fn text_block(text: &str, fs: f64) -> (Vec<String>, f64, f64) {
    // (lines, width_px, height_px) with a chunky estimate for Cinzel-like fonts.
    let lines: Vec<String> = text.split('\n').map(|l| l.to_string()).collect();
    let maxlen = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
    let width = maxlen * fs * 0.62;
    let height = lines.len() as f64 * fs * 1.2;
    (lines, width, height)
}

fn render_label(s: &mut String, layout: &FigureLayout, label: &LabelStyle, bg_hex: &str) {
    let fs = label.size_pt / 72.0 * FIG_DPI;
    let (lines, tw, th) = text_block(&label.text, fs);
    if lines.iter().all(|l| l.is_empty()) {
        return;
    }
    let (ax, ay) = layout.frac_to_px(label.x, label.y);
    let pad = fs * 0.25;
    let (rect_top, first_baseline) = match label.vertical_alignment {
        VAlign::Top => (ay, ay + 0.9 * fs),
        VAlign::Bottom => (ay - th, ay - 0.15 * fs),
    };
    if label.background {
        let _ = writeln!(
            s,
            r#"  <rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="{bg}" />"#,
            x = ax - pad,
            y = rect_top - pad,
            w = tw + 2.0 * pad,
            h = th + 1.4 * pad,
            bg = bg_hex
        );
    }
    let _ = writeln!(
        s,
        r#"  <text x="{ax:.2}" y="{y:.2}" font-family="{family}, serif" font-size="{fs:.2}" fill="{color}" xml:space="preserve">"#,
        y = first_baseline,
        family = esc(&label.font_family),
        color = hex3(label.color)
    );
    for (k, line_text) in lines.iter().enumerate() {
        let dy = if k == 0 { 0.0 } else { 1.2 * fs };
        let _ = writeln!(
            s,
            r#"    <tspan x="{ax:.2}" dy="{dy:.2}">{}</tspan>"#,
            esc(line_text)
        );
    }
    let _ = writeln!(s, "  </text>");
}

fn render_annotation(s: &mut String, layout: &FigureLayout, ann: &Annotation, bg_hex: &str) {
    let (dx, dy) = layout.frac_to_px(ann.x, ann.y);
    let r_px = ann.dot_pt / 72.0 * FIG_DPI / 2.0;
    let _ = writeln!(
        s,
        r#"  <circle cx="{dx:.2}" cy="{dy:.2}" r="{r:.2}" fill="{color}" />"#,
        r = r_px,
        color = hex3(ann.color)
    );
    if ann.label.is_empty() {
        return;
    }
    let fs = ann.label_size_pt / 72.0 * FIG_DPI;
    let (lx, ly) = layout.frac_to_px(ann.x + ann.x_offset, ann.y + ann.y_offset);
    let (lines, tw, th) = text_block(&ann.label, fs);
    let pad = fs * 0.25;
    if ann.background {
        let _ = writeln!(
            s,
            r#"  <rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" fill="{bg}" />"#,
            x = lx - pad,
            y = ly - 0.15 * fs - pad,
            w = tw + 2.0 * pad,
            h = th + 1.4 * pad,
            bg = bg_hex
        );
    }
    let _ = writeln!(
        s,
        r#"  <text x="{lx:.2}" y="{y:.2}" font-family="Cinzel, serif" font-size="{fs:.2}" fill="{color}" xml:space="preserve">{}</text>"#,
        esc(&lines.join(" ")),
        y = ly - 0.15 * fs,
        color = hex3(ann.color)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::srtm::SyntheticSource;

    fn scene() -> RidgeScene {
        let src = SyntheticSource { side: 1201 };
        crate::geometry::build_scene(
            &src,
            &crate::DEFAULT_BBOX,
            12,
            16,
            0.0,
            false,
            0,
            false,
            10.0,
            3,
            40.0,
            20.0,
        )
        .unwrap()
    }

    #[test]
    fn svg_contains_lines_and_background() {
        let sc = scene();
        let style = PlotStyle {
            line: LineColorSpec::Solid { rgb: [0, 0, 0] },
            kind: ColorKind::Gradient,
            background: [236, 232, 236],
            linewidth_pt: 2.0,
            size_scale: 20.0,
            label: Some(LabelStyle {
                text: "The White\nMountains".into(),
                color: [0, 0, 0],
                x: 0.62,
                y: 0.15,
                size_pt: 60.0,
                vertical_alignment: VAlign::Bottom,
                font_family: "Cinzel".into(),
                background: true,
            }),
            annotation: None,
        };
        let svg = render_svg(&sc, &style);
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("fill=\"#ece8ec\"")); // upstream default background
        assert!(svg.contains("<path"));
        assert!(svg.contains("The White"));
        assert!(svg.contains("Mountains"));
        assert!(svg.contains("Cinzel"));
        // One fill + one stroke path per row with data.
        assert_eq!(svg.matches("<path").count() >= sc.rows.len(), true);
    }

    #[test]
    fn svg_elevation_kind() {
        let sc = scene();
        let style = PlotStyle {
            line: LineColorSpec::Map {
                name: crate::colormap::Colormap::Ocean,
            },
            kind: ColorKind::Elevation,
            background: [236, 232, 236],
            linewidth_pt: 2.0,
            size_scale: 20.0,
            label: None,
            annotation: Some(Annotation {
                label: "SUMMIT".into(),
                x: 0.5,
                y: 0.5,
                x_offset: 0.01,
                y_offset: 0.01,
                label_size_pt: 20.0,
                dot_pt: 8.0,
                color: [0, 0, 0],
                background: false,
            }),
        };
        let svg = render_svg(&sc, &style);
        assert!(svg.contains("<circle"));
        assert!(svg.contains("SUMMIT"));
        // Per-segment strokes produce more paths than rows.
        assert!(svg.matches("<path").count() > sc.rows.len());
    }
}
