//! Colors: named RGB values, matplotlib-style colormaps.
//!
//! `viridis`/`magma`/`inferno`/`plasma`/`cividis` come from the `colorous`
//! crate. The classic matplotlib maps (`spring`, `summer`, `autumn`,
//! `winter`, `cool`, `bone`, `ocean`, `gnuplot`) are implemented here from
//! matplotlib's own formulas (`lib/matplotlib/_cm.py`), because no
//! permissively licensed crate ships them with matplotlib's definitions:
//! `colorous::COOL` and `colorgrad::preset::cool()` are the Cubehelix/d3
//! "cool", not matplotlib's, and `prismatica` is GPL-3.0 and carries only a
//! handful of matplotlib maps. `tests::matplotlib_formulas` pins the exact
//! values, so swapping in a crate's approximation would quietly change every
//! render.

/// 8-bit RGB.
pub type Rgb = [u8; 3];

pub fn hex(hexstr: &str) -> Rgb {
    hex_checked(hexstr).expect("valid hex color")
}

pub fn hex_checked(hexstr: &str) -> Option<Rgb> {
    let h = hexstr.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

#[inline]
fn clamped(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Colormap {
    Viridis,
    Magma,
    Inferno,
    Plasma,
    Cividis,
    Spring,
    Summer,
    Autumn,
    Winter,
    Cool,
    Bone,
    Ocean,
    Gnuplot,
}

impl Colormap {
    pub fn from_name(name: &str) -> Option<Colormap> {
        Some(match name.to_ascii_lowercase().as_str() {
            "viridis" => Colormap::Viridis,
            "magma" => Colormap::Magma,
            "inferno" => Colormap::Inferno,
            "plasma" => Colormap::Plasma,
            "cividis" => Colormap::Cividis,
            "spring" => Colormap::Spring,
            "summer" => Colormap::Summer,
            "autumn" => Colormap::Autumn,
            "winter" => Colormap::Winter,
            "cool" => Colormap::Cool,
            "bone" => Colormap::Bone,
            "ocean" => Colormap::Ocean,
            "gnuplot" => Colormap::Gnuplot,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            Colormap::Viridis => "viridis",
            Colormap::Magma => "magma",
            Colormap::Inferno => "inferno",
            Colormap::Plasma => "plasma",
            Colormap::Cividis => "cividis",
            Colormap::Spring => "spring",
            Colormap::Summer => "summer",
            Colormap::Autumn => "autumn",
            Colormap::Winter => "winter",
            Colormap::Cool => "cool",
            Colormap::Bone => "bone",
            Colormap::Ocean => "ocean",
            Colormap::Gnuplot => "gnuplot",
        }
    }

    /// Evaluate at `t` in [0, 1].
    pub fn at(&self, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |v: f64| (clamped(v) * 255.0).round() as u8;
        match self {
            Colormap::Viridis => conv(colorous::VIRIDIS.eval_continuous(t)),
            Colormap::Magma => conv(colorous::MAGMA.eval_continuous(t)),
            Colormap::Inferno => conv(colorous::INFERNO.eval_continuous(t)),
            Colormap::Plasma => conv(colorous::PLASMA.eval_continuous(t)),
            Colormap::Cividis => conv(colorous::CIVIDIS.eval_continuous(t)),
            // matplotlib formulas
            Colormap::Spring => [f(1.0), f(t), f(1.0 - t)], // (1, t, 1-t)
            Colormap::Summer => [f(t), f(1.0 - 0.5 * t), f(0.4 * t)], // (t, 1-0.5t, 0.4t)
            Colormap::Autumn => [f(1.0), f(t), f(0.0)],
            Colormap::Winter => [f(0.0), f(t), f(1.0 - 0.5 * t)],
            Colormap::Cool => [f(t), f(1.0 - t), f(1.0)],
            Colormap::Bone => {
                // matplotlib _bone_data segmented tables, red:
                //   (0,0) -> (0.746032, 0.652778) -> (1, 1)
                let r = segmented(t, &[(0.0, 0.0), (0.746032, 0.652778), (1.0, 1.0)]);
                let g = segmented(
                    t,
                    &[
                        (0.0, 0.0),
                        (0.365079, 0.319444),
                        (0.746032, 0.777778),
                        (1.0, 1.0),
                    ],
                );
                let b = segmented(t, &[(0.0, 0.0), (0.365079, 0.444444), (1.0, 1.0)]);
                [f(r), f(g), f(b)]
            }
            Colormap::Ocean => {
                // _ocean_data: red = gfunc[23] = 3x - 2, green = gfunc[28] =
                // |(3x - 1) / 2|, blue = gfunc[3] = x
                [f(3.0 * t - 2.0), f(((3.0 * t - 1.0) / 2.0).abs()), f(t)]
            }
            Colormap::Gnuplot => {
                // red = gfunc[7] = sqrt(x), green = gfunc[5] = x^3,
                // blue = gfunc[15] = sin(2 pi x)
                [
                    f(t.sqrt()),
                    f(t * t * t),
                    f((t * 2.0 * std::f64::consts::PI).sin()),
                ]
            }
        }
    }
}

fn conv(c: colorous::Color) -> Rgb {
    [c.r, c.g, c.b]
}

/// Linear interpolation over (x, y) anchor points (matplotlib segmented data
/// with the y0/y1 pairs already collapsed: y is continuous).
fn segmented(t: f64, points: &[(f64, f64)]) -> f64 {
    debug_assert!(points.len() >= 2);
    if t <= points[0].0 {
        return points[0].1;
    }
    for w in points.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if t <= x1 {
            return y0 + (y1 - y0) * (t - x0) / (x1 - x0);
        }
    }
    points[points.len() - 1].1
}

/// Either a solid color or a colormap to walk across the lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineColor {
    Solid(Rgb),
    Map(Colormap),
}

impl LineColor {
    pub fn parse(name: &str) -> Option<LineColor> {
        let lower = name.to_ascii_lowercase();
        if let Some(cm) = Colormap::from_name(&lower) {
            return Some(LineColor::Map(cm));
        }
        let named: Rgb = match lower.as_str() {
            "black" => [0, 0, 0],
            "white" => [255, 255, 255],
            "orange" => [255, 165, 0],
            "red" => [255, 0, 0],
            "green" => [0, 128, 0],
            "blue" => [0, 0, 255],
            "purple" => [128, 0, 128],
            "brown" => [165, 42, 42],
            "pink" => [255, 192, 203],
            "gray" | "grey" => [128, 128, 128],
            "darkgray" | "darkgrey" => [169, 169, 169],
            "navy" => [0, 0, 128],
            "teal" => [0, 128, 128],
            "crimson" => [220, 20, 60],
            _ => return hex_checked(name).map(LineColor::Solid),
        };
        Some(LineColor::Solid(named))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints() {
        for cm in [
            Colormap::Viridis,
            Colormap::Spring,
            Colormap::Bone,
            Colormap::Ocean,
            Colormap::Gnuplot,
            Colormap::Cool,
        ] {
            let a = cm.at(0.0);
            let b = cm.at(1.0);
            assert_eq!(a.len(), 3);
            assert_ne!(a, b, "{} should not be constant", cm.name());
        }
    }

    #[test]
    fn matplotlib_formulas() {
        assert_eq!(Colormap::Spring.at(0.5), [255, 128, 128]);
        assert_eq!(Colormap::Cool.at(0.0), [0, 255, 255]);
        assert_eq!(Colormap::Cool.at(1.0), [255, 0, 255]);
        // ocean red = 3x-2 clamps below x=2/3
        assert_eq!(Colormap::Ocean.at(0.5)[0], 0);
        assert_eq!(Colormap::Ocean.at(1.0)[0], 255);
    }

    #[test]
    fn parse_names() {
        assert_eq!(
            LineColor::parse("viridis"),
            Some(LineColor::Map(Colormap::Viridis))
        );
        assert_eq!(LineColor::parse("black"), Some(LineColor::Solid([0, 0, 0])));
        assert_eq!(
            LineColor::parse("#0f0f0f"),
            Some(LineColor::Solid([15, 15, 15]))
        );
        assert_eq!(LineColor::parse("not-a-color"), None);
    }
}
