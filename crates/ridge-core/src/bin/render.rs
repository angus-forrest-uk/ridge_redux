//! CLI renderer — `cargo run -p ridge-core --bin render -- --help`
//!
//! Milestone tool for Phase 2: run the whole pipeline offline and emit an
//! SVG, mirroring upstream README examples.

use ridge_core::colormap::LineColor;
use ridge_core::geometry::{build_scene, ColorKind, DEFAULT_SIZE_SCALE};
use ridge_core::svg::{render_svg, Annotation, LabelStyle, LineColorSpec, PlotStyle, VAlign};
use ridge_core::{srtm, Bbox, DEFAULT_BBOX};

#[derive(Debug, Clone, PartialEq)]
struct Args {
    bbox: Bbox,
    num_lines: usize,
    elevation_pts: usize,
    viewpoint_angle: f64,
    crop: bool,
    interpolation: u32,
    water_ntile: f64,
    lake_flatness: i32,
    vertical_ratio: f64,
    linewidth_pt: f64,
    color: String,
    kind: ColorKind,
    background: String,
    label: String,
    label_color: Option<String>,
    label_x: f64,
    label_y: f64,
    label_size_pt: f64,
    size_scale: f64,
    annotation: Option<(f64, f64, String)>,
    srtm_base: String,
    cache_dir: Option<String>,
    fixture_dir: Option<String>,
    out: String,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            bbox: DEFAULT_BBOX,
            num_lines: 80,
            elevation_pts: 300,
            viewpoint_angle: 0.0,
            crop: false,
            interpolation: 0,
            water_ntile: 10.0,
            lake_flatness: 3,
            vertical_ratio: 40.0,
            linewidth_pt: 2.0,
            color: "black".into(),
            kind: ColorKind::Gradient,
            background: "#ece9ec".into(),
            label: "The White\nMountains".into(),
            label_color: None,
            label_x: 0.62,
            label_y: 0.15,
            label_size_pt: 60.0,
            size_scale: DEFAULT_SIZE_SCALE,
            annotation: None,
            srtm_base: "https://srtm.kurviger.de/SRTM1/,https://srtm.kurviger.de/SRTM3/".into(),
            cache_dir: None,
            fixture_dir: None,
            out: "ridge.svg".into(),
        }
    }
}

fn usage() -> &'static str {
    r#"ridge render — ridgeline terrain art to SVG

USAGE:
  render [OPTIONS]

OPTIONS:
  --bbox "lon0,lat0,lon1,lat1"   Bounding box (default: White Mountains, NH)
  --num-lines N                  Horizontal lines (default 80)
  --elevation-pts N              Points per line (default 300)
  --angle DEG                    Viewpoint angle (default 0)
  --crop                         Crop corners when rotating
  --interpolation 0|1            Rotation interpolation (default 0)
  --water-ntile P                Water percentile cutoff (default 10)
  --lake-flatness N              Flatness cutoff for lakes (default 3)
  --vertical-ratio R             Vertical exaggeration (default 40)
  --linewidth PT                 Line width in points (default 2)
  --color NAME                   Color or colormap: black, orange, #0f0f0f,
                                 viridis, ocean, spring, cool, bone, gnuplot...
  --kind gradient|elevation      Colormap coloring mode (default gradient)
  --background NAME              Background color (default #ece9ec)
  --label TEXT                   Label ("\n" for line breaks; "" for none)
  --label-color NAME             Label color (defaults to line color)
  --label-x F --label-y F        Label position in axes fractions
  --label-size PT                Label font size (default 60)
  --size-scale IN                Figure width in inches (default 20)
  --annotate "lon,lat,TEXT"      Dot + label at a coordinate
  --srtm-base URL                Tile mirror (default kurviger SRTM1)
  --cache-dir DIR                Tile cache (default: ridge-redux/srtm in the OS cache dir)
  --fixture-dir DIR              Read .hgt tiles from DIR instead (offline)
  --out FILE                     Output SVG (default ridge.svg)
"#
}

fn parse_color(name: &str) -> Result<LineColor, String> {
    LineColor::parse(name).ok_or_else(|| format!("unknown color {name:?}"))
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
        let mut a = Args::default();
        while let Some(arg) = args.next() {
            let mut val = || args.next().ok_or(format!("missing value for {arg}"));
            match arg.as_str() {
                "--help" | "-h" => return Err(usage().to_string()),
                "--bbox" => {
                    let v = val()?;
                    let nums: Vec<f64> = v
                        .split(',')
                        .map(|s| s.trim().parse().map_err(|_| format!("bad number {s}")))
                        .collect::<Result<_, _>>()?;
                    if nums.len() != 4 {
                        return Err(format!("--bbox wants 4 numbers, got {}", nums.len()));
                    }
                    a.bbox = Bbox::new(nums[0], nums[1], nums[2], nums[3]);
                }
                "--num-lines" => a.num_lines = val()?.parse().map_err(|e| format!("{e}"))?,
                "--elevation-pts" => {
                    a.elevation_pts = val()?.parse().map_err(|e| format!("{e}"))?
                }
                "--angle" => a.viewpoint_angle = val()?.parse().map_err(|e| format!("{e}"))?,
                "--crop" => a.crop = true,
                "--interpolation" => {
                    let v: u32 = val()?.parse().map_err(|e| format!("{e}"))?;
                    if v > 1 {
                        return Err("only interpolation 0 and 1 are supported".into());
                    }
                    a.interpolation = v;
                }
                "--water-ntile" => a.water_ntile = val()?.parse().map_err(|e| format!("{e}"))?,
                "--lake-flatness" => {
                    a.lake_flatness = val()?.parse().map_err(|e| format!("{e}"))?
                }
                "--vertical-ratio" => {
                    a.vertical_ratio = val()?.parse().map_err(|e| format!("{e}"))?
                }
                "--linewidth" => a.linewidth_pt = val()?.parse().map_err(|e| format!("{e}"))?,
                "--color" => a.color = val()?,
                "--kind" => {
                    a.kind = match val()?.as_str() {
                        "gradient" => ColorKind::Gradient,
                        "elevation" => ColorKind::Elevation,
                        other => return Err(format!("bad --kind {other:?}")),
                    }
                }
                "--background" => a.background = val()?,
                "--label" => a.label = val()?,
                "--label-color" => a.label_color = Some(val()?),
                "--label-x" => a.label_x = val()?.parse().map_err(|e| format!("{e}"))?,
                "--label-y" => a.label_y = val()?.parse().map_err(|e| format!("{e}"))?,
                "--label-size" => a.label_size_pt = val()?.parse().map_err(|e| format!("{e}"))?,
                "--size-scale" => a.size_scale = val()?.parse().map_err(|e| format!("{e}"))?,
                "--annotate" => {
                    let v = val()?;
                    let parts: Vec<&str> = v.splitn(3, ',').collect();
                    if parts.len() != 3 {
                        return Err("--annotate wants lon,lat,text".into());
                    }
                    a.annotation = Some((
                        parts[0].trim().parse().map_err(|e| format!("{e}"))?,
                        parts[1].trim().parse().map_err(|e| format!("{e}"))?,
                        parts[2].to_string(),
                    ));
                }
                "--srtm-base" => a.srtm_base = val()?,
                "--cache-dir" => a.cache_dir = Some(val()?),
                "--fixture-dir" => a.fixture_dir = Some(val()?),
                "--out" => a.out = val()?,
                other => return Err(format!("unknown argument {other:?}\n\n{}", usage())),
            }
        }
        Ok(a)
    }
}

fn main() {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(msg) => {
            let is_help = msg.contains("USAGE") || msg.contains("ridge render");
            eprintln!("{msg}");
            std::process::exit(if is_help { 0 } else { 2 });
        }
    };

    let source: Box<dyn srtm::TileSource> = if let Some(dir) = &args.fixture_dir {
        Box::new(srtm::DirSource::new(dir))
    } else {
        let cache_dir = match args.cache_dir.clone() {
            Some(d) => std::path::PathBuf::from(d),
            None => {
                let dir = srtm::default_cache_dir();
                match srtm::migrate_legacy_cache(&dir) {
                    Ok(0) => {}
                    Ok(n) => eprintln!("moved {n} cached tile(s) to {}", dir.display()),
                    Err(e) => eprintln!("warning: couldn't move the old tile cache: {e}"),
                }
                dir
            }
        };
        let bases: Vec<&str> = args.srtm_base.split(',').map(str::trim).collect();
        match srtm::RemoteSource::new(&bases, &cache_dir) {
            Ok(src) => Box::new(src),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    };

    let line_color = match parse_color(&args.color) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let label_color = match &args.label_color {
        Some(name) => match parse_color(name) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let background = ridge_core::colormap::hex_checked(&args.background).unwrap_or([236, 232, 236]);

    let scene = match build_scene(
        source.as_ref(),
        &args.bbox,
        args.num_lines,
        args.elevation_pts,
        args.viewpoint_angle,
        args.crop,
        args.interpolation,
        false,
        args.water_ntile,
        args.lake_flatness,
        args.vertical_ratio,
        args.size_scale,
    ) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let label_color = label_color
        .map(|c| match c {
            LineColor::Solid(rgb) => rgb,
            LineColor::Map(cm) => cm.at(0.0),
        })
        .unwrap_or_else(|| scene.label_color(&line_color));

    let annotation = args.annotation.map(|(lon, lat, text)| {
        let (lon0, lon1) = args.bbox.longs();
        let (lat0, lat1) = args.bbox.lats();
        let x = (lon - lon0) / (lon1 - lon0);
        let y = (lat - lat0) / (lat1 - lat0);
        Annotation {
            label: text,
            x,
            y,
            x_offset: 0.005,
            y_offset: 0.005,
            label_size_pt: 20.0,
            dot_pt: 8.0,
            color: label_color,
            background: false,
        }
    });

    let style = PlotStyle {
        line: LineColorSpec::from(&line_color),
        kind: args.kind,
        background,
        linewidth_pt: args.linewidth_pt,
        size_scale: args.size_scale,
        label: if args.label.is_empty() {
            None
        } else {
            Some(LabelStyle {
                text: args.label.clone(),
                color: label_color,
                x: args.label_x,
                y: args.label_y,
                size_pt: args.label_size_pt,
                vertical_alignment: VAlign::Bottom,
                font_family: "Cinzel".into(),
                background: true,
            })
        },
        annotation,
    };

    let svg = render_svg(&scene, &style);
    if let Err(e) = std::fs::write(&args.out, &svg) {
        eprintln!("error writing {}: {e}", args.out);
        std::process::exit(1);
    }
    println!(
        "wrote {} ({} rows x {} points, {:.1} KiB)",
        args.out,
        scene.rows.len(),
        scene.n_points,
        svg.len() as f64 / 1024.0
    );
}
