//! CLI renderer — `cargo run -p ridge-core --bin render -- --help`
//!
//! Milestone tool for Phase 2: run the whole pipeline offline and emit an
//! SVG, mirroring upstream README examples.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use ridge_core::colormap::LineColor;
use ridge_core::geometry::{build_scene, ColorKind, DEFAULT_SIZE_SCALE};
use ridge_core::svg::{render_svg, Annotation, LabelStyle, LineColorSpec, PlotStyle, VAlign};
use ridge_core::{srtm, Bbox, DEFAULT_BBOX};

/// Render ridgeline terrain art to a standalone SVG, offline or from the tile
/// mirror.
#[derive(Parser, Debug)]
#[command(name = "render", version, about)]
struct Args {
    /// Bounding box "lon0,lat0,lon1,lat1" [default: The White Mountains, NH]
    #[arg(
        long,
        value_name = "LON0,LAT0,LON1,LAT1",
        value_parser = parse_bbox,
        allow_hyphen_values = true
    )]
    bbox: Option<Bbox>,

    /// Number of horizontal lines
    #[arg(long, value_name = "N", default_value_t = 80)]
    num_lines: usize,

    /// Number of points sampled on each line
    #[arg(long, value_name = "N", default_value_t = 300)]
    elevation_pts: usize,

    /// Viewpoint angle in degrees
    #[arg(
        long,
        value_name = "DEG",
        default_value_t = 0.0,
        allow_hyphen_values = true
    )]
    angle: f64,

    /// Crop the corners when rotating
    #[arg(long)]
    crop: bool,

    /// Rotation interpolation order (0 = nearest, 1 = bilinear)
    #[arg(
        long,
        value_name = "0|1",
        default_value_t = 0,
        value_parser = clap::value_parser!(u32).range(0..=1)
    )]
    interpolation: u32,

    /// Percentile below which elevations are masked as water
    #[arg(long, value_name = "P", default_value_t = 10.0)]
    water_ntile: f64,

    /// Flatness cutoff for lake detection
    #[arg(long, value_name = "N", default_value_t = 3)]
    lake_flatness: i32,

    /// Vertical exaggeration
    #[arg(long, value_name = "R", default_value_t = 40.0)]
    vertical_ratio: f64,

    /// Line width in points
    #[arg(long, value_name = "PT", default_value_t = 2.0)]
    linewidth: f64,

    /// Line color or colormap: black, orange, "#0f0f0f", viridis, ocean, ...
    #[arg(long, value_name = "NAME", default_value = "black")]
    color: String,

    /// Colormap coloring mode
    #[arg(long, value_enum, default_value = "gradient")]
    kind: Kind,

    /// Background color
    #[arg(long, value_name = "NAME", default_value = "#ece9ec")]
    background: String,

    /// Label text ("\n" for line breaks; "" for none)
    #[arg(long, default_value = "The White\nMountains")]
    label: String,

    /// Label color (defaults to the line color)
    #[arg(long, value_name = "NAME")]
    label_color: Option<String>,

    /// Label horizontal position, in axes fractions
    #[arg(
        long,
        value_name = "F",
        default_value_t = 0.62,
        allow_hyphen_values = true
    )]
    label_x: f64,

    /// Label vertical position, in axes fractions
    #[arg(
        long,
        value_name = "F",
        default_value_t = 0.15,
        allow_hyphen_values = true
    )]
    label_y: f64,

    /// Label font size in points
    #[arg(long, value_name = "PT", default_value_t = 60.0)]
    label_size: f64,

    /// Figure width in inches
    #[arg(long, value_name = "IN", default_value_t = DEFAULT_SIZE_SCALE)]
    size_scale: f64,

    /// Dot + label at a coordinate, written "lon,lat,text"
    #[arg(
        long,
        value_name = "LON,LAT,TEXT",
        value_parser = parse_annotation,
        allow_hyphen_values = true
    )]
    annotate: Option<AnnotationArg>,

    /// Comma-separated SRTM mirror base URLs, tried in order
    #[arg(
        long,
        value_name = "URL",
        default_value = "https://srtm.kurviger.de/SRTM1/,https://srtm.kurviger.de/SRTM3/"
    )]
    srtm_base: String,

    /// Tile cache directory (default: ridge-redux/srtm in the OS cache dir)
    #[arg(long, value_name = "DIR")]
    cache_dir: Option<PathBuf>,

    /// Read .hgt tiles from this directory instead of the network (offline)
    #[arg(long, value_name = "DIR")]
    fixture_dir: Option<PathBuf>,

    /// Output SVG file
    #[arg(long, value_name = "FILE", default_value = "ridge.svg")]
    out: PathBuf,
}

/// How a colormap-driven `line_color` colors the lines.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum Kind {
    /// Color by line index (upstream `kind="gradient"`)
    Gradient,
    /// Color by elevation along the line (upstream `kind="elevation"`)
    Elevation,
}

impl From<Kind> for ColorKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Gradient => ColorKind::Gradient,
            Kind::Elevation => ColorKind::Elevation,
        }
    }
}

/// A `--annotate "lon,lat,text"` value.
#[derive(Clone, Debug)]
struct AnnotationArg {
    lon: f64,
    lat: f64,
    text: String,
}

fn parse_bbox(value: &str) -> Result<Bbox, String> {
    let nums: Vec<f64> = value
        .split(',')
        .map(|part| {
            part.trim()
                .parse()
                .map_err(|_| format!("bad number {:?} in bbox", part.trim()))
        })
        .collect::<Result<_, _>>()?;
    match nums.as_slice() {
        [lon0, lat0, lon1, lat1] => Ok(Bbox::new(*lon0, *lat0, *lon1, *lat1)),
        _ => Err(format!(
            "bbox wants 4 comma-separated numbers, got {}",
            nums.len()
        )),
    }
}

fn parse_annotation(value: &str) -> Result<AnnotationArg, String> {
    let mut parts = value.splitn(3, ',');
    let (Some(lon), Some(lat), Some(text)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("annotate wants \"lon,lat,text\"".into());
    };
    Ok(AnnotationArg {
        lon: lon
            .trim()
            .parse()
            .map_err(|_| format!("bad longitude {lon:?}"))?,
        lat: lat
            .trim()
            .parse()
            .map_err(|_| format!("bad latitude {lat:?}"))?,
        text: text.to_string(),
    })
}

/// Print an error and exit with `code` (matching clap's parse-error code 2).
fn die(code: i32, message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(code);
}

fn main() {
    let args = Args::parse();
    let bbox = args.bbox.unwrap_or(DEFAULT_BBOX);

    let source: Box<dyn srtm::TileSource> = match &args.fixture_dir {
        Some(dir) => Box::new(srtm::DirSource::new(dir)),
        None => {
            let cache_dir = args.cache_dir.clone().unwrap_or_else(|| {
                let dir = srtm::default_cache_dir();
                match srtm::migrate_legacy_cache(&dir) {
                    Ok(0) => {}
                    Ok(n) => eprintln!("moved {n} cached tile(s) to {}", dir.display()),
                    Err(e) => eprintln!("warning: couldn't move the old tile cache: {e}"),
                }
                dir
            });
            let bases: Vec<&str> = args.srtm_base.split(',').map(str::trim).collect();
            match srtm::RemoteSource::new(&bases, &cache_dir) {
                Ok(src) => Box::new(src),
                Err(e) => die(1, &e.to_string()),
            }
        }
    };

    let line_color = LineColor::parse(&args.color)
        .unwrap_or_else(|| die(2, &format!("unknown color {:?}", args.color)));
    let label_color = args.label_color.as_ref().map(|name| {
        LineColor::parse(name).unwrap_or_else(|| die(2, &format!("unknown color {name:?}")))
    });
    let background = ridge_core::colormap::hex_checked(&args.background).unwrap_or([236, 232, 236]);

    let scene = build_scene(
        source.as_ref(),
        &bbox,
        args.num_lines,
        args.elevation_pts,
        args.angle,
        args.crop,
        args.interpolation,
        false,
        args.water_ntile,
        args.lake_flatness,
        args.vertical_ratio,
        args.size_scale,
    )
    .unwrap_or_else(|e| die(1, &e.to_string()));

    let label_color = label_color
        .map(|c| match c {
            LineColor::Solid(rgb) => rgb,
            LineColor::Map(cm) => cm.at(0.0),
        })
        .unwrap_or_else(|| scene.label_color(&line_color));

    let annotation = args.annotate.as_ref().map(|a| {
        let (lon0, lon1) = bbox.longs();
        let (lat0, lat1) = bbox.lats();
        Annotation {
            label: a.text.clone(),
            x: (a.lon - lon0) / (lon1 - lon0),
            y: (a.lat - lat0) / (lat1 - lat0),
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
        kind: args.kind.into(),
        background,
        linewidth_pt: args.linewidth,
        size_scale: args.size_scale,
        label: if args.label.is_empty() {
            None
        } else {
            Some(LabelStyle {
                text: args.label.clone(),
                color: label_color,
                x: args.label_x,
                y: args.label_y,
                size_pt: args.label_size,
                vertical_alignment: VAlign::Bottom,
                font_family: "Cinzel".into(),
                background: true,
            })
        },
        annotation,
    };

    let svg = render_svg(&scene, &style);
    if let Err(e) = std::fs::write(&args.out, &svg) {
        die(1, &format!("writing {}: {e}", args.out.display()));
    }
    println!(
        "wrote {} ({} rows x {} points, {:.1} KiB)",
        args.out.display(),
        scene.rows.len(),
        scene.n_points,
        svg.len() as f64 / 1024.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(argv: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("render").chain(argv.iter().copied()))
    }

    #[test]
    fn schema_is_well_formed() {
        // clap's own invariant checker: duplicate/conflicting args, unusable
        // defaults, etc. Recommended by the clap book over a bespoke test.
        Args::command().debug_assert();
    }

    #[test]
    fn defaults_match_upstream() {
        let a = parse(&[]).unwrap();
        assert_eq!(a.num_lines, 80);
        assert_eq!(a.elevation_pts, 300);
        assert_eq!(a.angle, 0.0);
        assert_eq!(a.interpolation, 0);
        assert_eq!(a.water_ntile, 10.0);
        assert_eq!(a.lake_flatness, 3);
        assert_eq!(a.vertical_ratio, 40.0);
        assert_eq!(a.linewidth, 2.0);
        assert_eq!(a.color, "black");
        assert!(matches!(a.kind, Kind::Gradient));
        assert_eq!(a.background, "#ece9ec");
        assert_eq!(a.label, "The White\nMountains");
        assert_eq!(a.label_x, 0.62);
        assert_eq!(a.label_y, 0.15);
        assert_eq!(a.label_size, 60.0);
        assert_eq!(a.size_scale, DEFAULT_SIZE_SCALE);
        assert_eq!(a.out, PathBuf::from("ridge.svg"));
        assert!(a.bbox.is_none());
        assert!(a.annotate.is_none());
        assert!(a.label_color.is_none());
        assert!(a.cache_dir.is_none());
        assert!(a.fixture_dir.is_none());
        assert!(!a.crop);
    }

    #[test]
    fn hyphen_leading_values_are_accepted() {
        // These flags set `allow_hyphen_values`; without it clap reads the
        // value as a flag. Negative coordinates and angles are the norm.
        let a = parse(&[
            "--bbox",
            "-71.9,43.7,-70.9,44.4",
            "--angle",
            "-33",
            "--annotate",
            "-71.3173,44.2946,Mt Washington",
            "--label-x",
            "-0.1",
        ])
        .unwrap();
        assert_eq!(a.bbox, Some(Bbox::new(-71.9, 43.7, -70.9, 44.4)));
        assert_eq!(a.angle, -33.0);
        assert_eq!(a.label_x, -0.1);
        let ann = a.annotate.as_ref().unwrap();
        assert_eq!(
            (ann.lon, ann.lat, ann.text.as_str()),
            (-71.3173, 44.2946, "Mt Washington")
        );
    }

    #[test]
    fn every_option_round_trips() {
        let a = parse(&[
            "--bbox",
            "1,2,3,4",
            "--num-lines",
            "12",
            "--elevation-pts",
            "34",
            "--angle",
            "90",
            "--crop",
            "--interpolation",
            "1",
            "--water-ntile",
            "5",
            "--lake-flatness",
            "7",
            "--vertical-ratio",
            "120",
            "--linewidth",
            "3",
            "--color",
            "ocean",
            "--kind",
            "elevation",
            "--background",
            "#000000",
            "--label",
            "Hello\nWorld",
            "--label-color",
            "white",
            "--label-x",
            "0.1",
            "--label-y",
            "0.2",
            "--label-size",
            "30",
            "--size-scale",
            "10",
            "--annotate",
            "1,2,Summit",
            "--srtm-base",
            "https://example.test/",
            "--cache-dir",
            "/tmp/cache",
            "--fixture-dir",
            "/tmp/tiles",
            "--out",
            "out.svg",
        ])
        .unwrap();
        assert_eq!(a.bbox, Some(Bbox::new(1.0, 2.0, 3.0, 4.0)));
        assert_eq!(a.num_lines, 12);
        assert_eq!(a.elevation_pts, 34);
        assert_eq!(a.angle, 90.0);
        assert!(a.crop);
        assert_eq!(a.interpolation, 1);
        assert_eq!(a.water_ntile, 5.0);
        assert_eq!(a.lake_flatness, 7);
        assert_eq!(a.vertical_ratio, 120.0);
        assert_eq!(a.linewidth, 3.0);
        assert_eq!(a.color, "ocean");
        assert!(matches!(a.kind, Kind::Elevation));
        assert_eq!(a.background, "#000000");
        assert_eq!(a.label, "Hello\nWorld");
        assert_eq!(a.label_color.as_deref(), Some("white"));
        assert_eq!((a.label_x, a.label_y, a.label_size), (0.1, 0.2, 30.0));
        assert_eq!(a.size_scale, 10.0);
        let ann = a.annotate.as_ref().unwrap();
        assert_eq!((ann.lon, ann.lat, ann.text.as_str()), (1.0, 2.0, "Summit"));
        assert_eq!(a.srtm_base, "https://example.test/");
        assert_eq!(
            a.cache_dir.as_deref(),
            Some(std::path::Path::new("/tmp/cache"))
        );
        assert_eq!(
            a.fixture_dir.as_deref(),
            Some(std::path::Path::new("/tmp/tiles"))
        );
        assert_eq!(a.out, PathBuf::from("out.svg"));
    }

    #[test]
    fn invalid_input_is_rejected() {
        assert!(parse(&["--interpolation", "2"]).is_err());
        assert!(parse(&["--kind", "bogus"]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["--num-lines", "abc"]).is_err());
        assert!(parse(&["--num-lines"]).is_err(), "missing value");
    }

    #[test]
    fn parse_bbox_accepts_four_numbers_and_tolerates_spaces() {
        assert_eq!(
            parse_bbox("1,2,3,4").unwrap(),
            Bbox::new(1.0, 2.0, 3.0, 4.0)
        );
        assert_eq!(
            parse_bbox(" 1 , 2 , 3 , 4 ").unwrap(),
            Bbox::new(1.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn parse_bbox_rejects_wrong_arity_and_garbage() {
        for bad in ["", "1", "1,2", "1,2,3", "1,2,3,4,5", "a,b,c,d", "1,2,3,"] {
            assert!(parse_bbox(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn parse_bbox_defers_geographic_validity() {
        // The parser only enforces arity + numeric-ness; it happily accepts
        // NaN, and `Bbox::is_valid` (checked later by `build_scene`) is what
        // rejects unusable boxes.
        assert!(parse_bbox("nan,0,0,0").is_ok());
        assert!(!parse_bbox("nan,0,0,0").unwrap().is_valid());
        assert!(!parse_bbox("10,10,0,0").unwrap().is_valid());
    }

    #[test]
    fn parse_annotation_keeps_commas_in_text() {
        let a = parse_annotation("-71.3,44.3,Mt, Washington").unwrap();
        assert_eq!(
            (a.lon, a.lat, a.text.as_str()),
            (-71.3, 44.3, "Mt, Washington")
        );
    }

    #[test]
    fn parse_annotation_rejects_bad_shapes() {
        for bad in ["", "1", "1,2", "x,2,t", "1,y,t"] {
            assert!(parse_annotation(bad).is_err(), "should reject {bad:?}");
        }
    }

    /// Deterministic stand-in for a fuzzer: throw a few thousand random
    /// separator-heavy strings at the parsers and the full argv pipeline and
    /// assert nothing panics. Seeded, so a failure reproduces.
    #[test]
    fn parsing_never_panics_on_arbitrary_input() {
        let mut rng = SplitMix64(0x9e37_79b9_7f4a_7c15);
        for _ in 0..10_000 {
            let s = random_nasty_string(&mut rng);
            let _ = parse_bbox(&s);
            let _ = parse_annotation(&s);
            let _ = Args::try_parse_from(["render", s.as_str()]);
            let _ = Args::try_parse_from(["render", "--bbox", s.as_str()]);
            let _ = Args::try_parse_from(["render", "--annotate", s.as_str()]);
            let _ = Args::try_parse_from(["render", "--label", s.as_str()]);
        }
    }

    /// A fixed-seed splitmix64 — enough for reproducible input generation
    /// without pulling in a dependency.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
    }

    fn random_nasty_string(rng: &mut SplitMix64) -> String {
        const ALPHABET: &[u8] = b"0123456789.,-+ eE_/:;\t\n#\"'\\{}[]()";
        let len = (rng.next_u64() % 24) as usize;
        (0..len)
            .map(|_| ALPHABET[(rng.next_u64() as usize) % ALPHABET.len()] as char)
            .collect()
    }
}
