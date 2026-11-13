//! The disposition line of the retail History timeline and its vertical marker.
//!
//! Decoded from the march2017 code image (cell-timeline5; addresses are SDK addresses;
//! VERIFIED = read in the code, ADAPTED = OpenSpore's stand-in):
//!
//! * **Where the line goes** (`0xE45760`, the paint message of the timeline window; the
//!   event point `0xE470A0` / `0xE37F40`): through the centre of every event card,
//!   `(x + width / 2, (h / 2) * (1 + f))` in the stage column (`0xE46D80` stores it at
//!   `+0x100 / +0x104`; it is the *unclamped* height of the disposition value, also for the
//!   cards of a part stack). The earned-trait event at the end has no height of its own
//!   (`0xE37F40`: it takes its own x and the y of the event before it), so the line ends
//!   level at the trait bar. Every pair of neighbouring events is one segment `p1 -> p2`
//!   of a Catmull-Rom spline (`0xE39FB0`: `0.5 * (2 p1 + (p2 - p0) t + (2 p0 - 5 p1 + 4 p2
//!   - p3) t^2 + (3 p1 - p0 - 3 p2 + p3) t^3)`) whose end points repeat at the ends of the
//!   history (`0xE37E00` clamps the event index); two events at the same x get `p2.x += 300`.
//! * **How it is drawn** (`0xE3C150`): 30 steps per segment (`0xE3C1D5`), one quad per step
//!   with vertical edges whose top and bottom are `y -/+ v`, `v = LineThickness (12) /
//!   cos(atan |slope|)` of the step's chord, i.e. the ribbon is 12 px thick across its own
//!   direction (`0xE3C3DF`..`0xE3C41F`: `fpatan`, `fsubr pi/2`, `fsin`, `fdivr`). Around the end
//!   points of the segment (`0xE3A520`, four calls) the quad's top and bottom are pushed out to the
//!   circles of radius `LineEventCircleRadius` (20) when they lie inside them: the line is the
//!   ribbon united with a disc at every event. The quads are untextured: `0xE377F0` hands four
//!   vertices with the colour `-1` to the Graphics2D polygon call `+0x54`, no `SetColor`
//!   precedes it, and none of `LineColorTop / Middle / Bottom`, `LineColorBorder`,
//!   `LineColorDropShadow`, `LineBorderThickness`, `LineDropShadowOffset*` is read anywhere in the
//!   code image (`rawfind` over the whole image: only their static initialisers touch them), so
//!   the retail line is plain white.
//! * **The intro** (`0xE3C5D3`..`0xE3C664`, `0xE4323E`): `+0x280` counts the milliseconds
//!   since the window was shown (reset by `0xE44200`, clamped to `LineIntroTimeMS` = 1000);
//!   a quad is only drawn when `screenWidth * (+0x280 / 1000)` is beyond its x: the line wipes in
//!   from the left over one second ([`intro_reveal`]).
//! * **The marker** (`0xE3C28B`..`0xE3C3C6`; window `75581F48`, a 20 x 43 sprite that
//!   `0xE40C25` finds in the layout): while the line is drawn, the point of the line at the
//!   marker's x (`+0x88`) is found by interpolating the chord of the step that spans it, the
//!   marker window is moved there (`SetOffset(area.x1, y)`) and the height is kept in `+0x24C`;
//!   `0xE42C83` turns that height into the band (thirds of the view: [`super::axis::marker_band`])
//!   whose tint fades in. The marker keeps its last height while the line does not reach its x.
//!
//! ADAPTED: the marker is evaluated at the centre x of its window (retail `+0x88` starts as the
//! left edge of the `064178C8` strip and is then updated by the update itself; both are within
//! 30 px of the sprite), the marker window's vertical anchor is its centre, the ribbon is drawn
//! by a column coverage rasteriser into a texture instead of Graphics2D polygons.

/// `LineThickness` (`0x16ACECC`).
pub const THICKNESS: f32 = 12.0;
/// `LineEventCircleRadius` (`0x16ACF0C`).
pub const CIRCLE_RADIUS: f32 = 20.0;
/// Steps per spline segment (`0xE3C1D5`: `mov edi, 0x1e`).
pub const STEPS: usize = 30;
/// `+300` of two events at the same x (`0xE45AFF`, constant `0x1477F2C`).
pub const SAME_X_SHIFT: f32 = 300.0;
/// `LineIntroTimeMS` (`0x16ACF2C`).
pub const INTRO_MS: i32 = 1000;

/// A point of the line in screen space.
pub type Point = [f32; 2];

/// `0xE39FB0`: the Catmull-Rom point of `p1 -> p2` at `t`.
pub fn catmull_rom(p0: Point, p1: Point, p2: Point, p3: Point, t: f32) -> Point {
    std::array::from_fn(|i| {
        let (a, b, c, d) = (p0[i], p1[i], p2[i], p3[i]);
        0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t + (-a + 3.0 * b - 3.0 * c + d) * t * t * t)
    })
}

/// One sample of the ribbon: the line's centre and the top / bottom of the quad edge there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub x: f32,
    pub y: f32,
    pub top: f32,
    pub bottom: f32,
}

/// `0xE3C3DF`: the vertical half extent of a step whose chord has `slope` (`thickness` across).
pub fn half_extent(thickness: f32, slope: f32) -> f32 {
    thickness / slope.abs().atan().cos()
}

/// The samples of the whole line through `points` (the event points, left to right).
/// Two events at the same x get `+300` on the second one for the spline's sake only, as the
/// retail loop does (`0xE45AF9`).
pub fn samples(points: &[Point], thickness: f32) -> Vec<Sample> {
    let n = points.len();
    let mut out = Vec::new();
    if n == 0 {
        return out;
    }
    let first = points[0];
    out.push(Sample { x: first[0], y: first[1], top: first[1] - thickness, bottom: first[1] + thickness });
    for i in 0..n.saturating_sub(1) {
        let at = |k: isize| points[(i as isize + k).clamp(0, n as isize - 1) as usize];
        let (p0, p1, mut p2, mut p3) = (at(-1), at(0), at(1), at(2));
        if p1[0] == p2[0] {
            p2[0] += SAME_X_SHIFT;
            p3[0] += SAME_X_SHIFT;
        }
        let mut prev = p1;
        for step in 1..=STEPS {
            let t = step as f32 / STEPS as f32;
            let cur = catmull_rom(p0, p1, p2, p3, t);
            let slope = if cur[0] != prev[0] { (cur[1] - prev[1]) / (cur[0] - prev[0]) } else { 0.0 };
            let v = half_extent(thickness, slope);
            out.push(Sample { x: cur[0], y: cur[1], top: cur[1] - v, bottom: cur[1] + v });
            prev = cur;
        }
    }
    out
}

/// `0xE3C5E8`: how far the line has been drawn `elapsed_ms` after the window was shown, in
/// screen pixels of a screen `width` wide.
pub fn intro_reveal(elapsed_ms: i32, width: f32) -> f32 {
    width * (elapsed_ms.clamp(0, INTRO_MS) as f32 / INTRO_MS as f32)
}

/// The rasterised line: per pixel column the top and bottom of the silhouette and the
/// centre line (`NaN` where the line is absent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Columns {
    pub top: Vec<f32>,
    pub bottom: Vec<f32>,
    pub centre: Vec<f32>,
}

/// Silhouette of the ribbon united with a disc of `radius` at every point of `discs`, clipped
/// at `reveal` (the intro wipe), for the pixel columns `0..width`. The quad edges are linear
/// between samples (the quads of `0xE3BDD0`), the discs follow `0xE3A520`.
pub fn silhouette(samples: &[Sample], discs: &[Point], radius: f32, reveal: f32, width: usize) -> Columns {
    let mut cols = Columns {
        top: vec![f32::INFINITY; width],
        bottom: vec![f32::NEG_INFINITY; width],
        centre: vec![f32::NAN; width],
    };
    for pair in samples.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b.x <= a.x {
            continue;
        }
        let start = ((a.x - 0.5).ceil().max(0.0)) as usize;
        let mut px = start;
        while px < width {
            let xc = px as f32 + 0.5;
            if xc > b.x {
                break;
            }
            if xc >= a.x && xc <= reveal {
                let f = (xc - a.x) / (b.x - a.x);
                let lerp = |p: f32, q: f32| p + (q - p) * f;
                cols.top[px] = cols.top[px].min(lerp(a.top, b.top));
                cols.bottom[px] = cols.bottom[px].max(lerp(a.bottom, b.bottom));
                cols.centre[px] = lerp(a.y, b.y);
            }
            px += 1;
        }
    }
    for disc in discs {
        let lo = ((disc[0] - radius).floor().max(0.0)) as usize;
        let hi = (((disc[0] + radius).ceil()).max(0.0) as usize).min(width.saturating_sub(1));
        for px in lo..=hi.max(lo).min(width.saturating_sub(1)) {
            let xc = px as f32 + 0.5;
            let dx = xc - disc[0];
            if dx.abs() > radius || xc > reveal {
                continue;
            }
            let dy = (radius * radius - dx * dx).sqrt();
            cols.top[px] = cols.top[px].min(disc[1] - dy);
            cols.bottom[px] = cols.bottom[px].max(disc[1] + dy);
        }
    }
    cols
}

/// Fill `rgba` (`width` x `height`, straight alpha, white) with the silhouette: the pixel
/// coverage at the top and bottom edge is the overlap of the pixel row with the span.
pub fn paint(cols: &Columns, width: usize, height: usize, rgba: &mut [u8]) {
    rgba.fill(0);
    for px in 0..width.min(cols.top.len()) {
        let (top, bottom) = (cols.top[px], cols.bottom[px]);
        if !(bottom > top) || !top.is_finite() {
            continue;
        }
        let first = top.floor().max(0.0) as usize;
        let last = (bottom.ceil().max(0.0) as usize).min(height);
        for row in first..last {
            let coverage = (bottom.min(row as f32 + 1.0) - top.max(row as f32)).clamp(0.0, 1.0);
            let at = (row * width + px) * 4;
            rgba[at] = 255;
            rgba[at + 1] = 255;
            rgba[at + 2] = 255;
            rgba[at + 3] = (coverage * 255.0 + 0.5) as u8;
        }
    }
}

/// The height of the centre line at screen column `x` (`None` where the line is not drawn).
pub fn centre_at(cols: &Columns, x: f32) -> Option<f32> {
    let px = x.floor();
    if px < 0.0 {
        return None;
    }
    cols.centre.get(px as usize).copied().filter(|y| y.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spline_passes_through_its_end_points_and_is_the_chord_on_a_straight_line() {
        let (p0, p1, p2, p3) = ([0.0, 0.0], [10.0, 5.0], [20.0, 10.0], [30.0, 15.0]);
        assert_eq!(catmull_rom(p0, p1, p2, p3, 0.0), p1);
        let end = catmull_rom(p0, p1, p2, p3, 1.0);
        assert!((end[0] - 20.0).abs() < 1e-4 && (end[1] - 10.0).abs() < 1e-4, "{end:?}");
        let mid = catmull_rom(p0, p1, p2, p3, 0.5);
        assert!((mid[0] - 15.0).abs() < 1e-4 && (mid[1] - 7.5).abs() < 1e-4, "{mid:?}");
    }

    #[test]
    fn a_flat_line_is_twelve_pixels_thick_and_a_steep_one_is_thicker_vertically() {
        assert_eq!(half_extent(12.0, 0.0), 12.0);
        // 45 degrees: 12 / cos 45 = 16.97.
        assert!((half_extent(12.0, 1.0) - 12.0 * 2f32.sqrt()).abs() < 1e-3);
        assert_eq!(half_extent(12.0, -1.0), half_extent(12.0, 1.0));
        let s = samples(&[[10.0, 100.0], [110.0, 100.0]], THICKNESS);
        assert_eq!(s.len(), 1 + STEPS);
        assert!(s.iter().skip(1).all(|p| (p.bottom - p.top - 24.0).abs() < 1e-3), "{:?}", s[5]);
    }

    #[test]
    fn the_silhouette_is_the_ribbon_united_with_a_disc_at_every_event() {
        let points = [[50.0, 100.0], [150.0, 100.0], [250.0, 100.0]];
        let s = samples(&points, THICKNESS);
        let cols = silhouette(&s, &points, CIRCLE_RADIUS, 1e9, 300);
        // Between the events only the ribbon: y +- 12.
        assert!((cols.top[100] - 88.0).abs() < 1e-3 && (cols.bottom[100] - 112.0).abs() < 1e-3);
        // Over an event the disc (radius 20) is wider than the ribbon.
        assert!((cols.top[150] - 80.0).abs() < 0.2, "{}", cols.top[150]);
        assert!((cols.bottom[150] - 120.0).abs() < 0.2);
        // Nothing before the first event's disc, nothing after the last.
        assert!(!cols.centre[10].is_finite() && !cols.centre[290].is_finite());
        assert_eq!(centre_at(&cols, 100.4), Some(100.0));
        assert_eq!(centre_at(&cols, 20.0), None);
    }

    #[test]
    fn the_line_wipes_in_from_the_left_over_one_second() {
        assert_eq!(intro_reveal(0, 1280.0), 0.0);
        assert_eq!(intro_reveal(500, 1280.0), 640.0);
        assert_eq!(intro_reveal(5000, 1280.0), 1280.0);
        let points = [[50.0, 100.0], [350.0, 100.0]];
        let s = samples(&points, THICKNESS);
        let cols = silhouette(&s, &points, CIRCLE_RADIUS, 200.0, 400);
        assert!(cols.centre[150].is_finite() && !cols.centre[250].is_finite());
    }

    #[test]
    fn painting_covers_the_span_with_antialiased_edges() {
        let cols = Columns { top: vec![1.5, f32::INFINITY], bottom: vec![4.25, f32::NEG_INFINITY], centre: vec![3.0, f32::NAN] };
        let mut rgba = vec![9u8; 2 * 6 * 4];
        paint(&cols, 2, 6, &mut rgba);
        let alpha = |row: usize, px: usize| rgba[(row * 2 + px) * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(1, 0), 128, "half of the row 1");
        assert_eq!(alpha(2, 0), 255);
        assert_eq!(alpha(4, 0), 64, "a quarter of the row 4");
        assert_eq!(alpha(5, 0), 0);
        assert!((0..6).all(|r| alpha(r, 1) == 0), "an empty column stays clear");
    }
}
