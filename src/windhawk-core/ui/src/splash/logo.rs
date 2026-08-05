//! The splash logo's geometry: the Windhawk mark read out of the SVG asset
//! (`icons/main-icon-no-background.svg`, a copy of the art folder's file) as
//! resolution-independent path segments, plus the transform that centers it in a
//! window. The painter turns the segments into a GDI+ path; everything here is
//! pure, so the parsing and the placement are unit-testable without a window.
//!
//! Only the path-data subset this asset uses is understood - moveto, lineto,
//! curveto, smooth curveto and closepath, absolute and relative. The file is a
//! fixed asset of this crate rather than arbitrary input, so anything else fails
//! the parse outright (the splash then paints the themed background alone)
//! instead of guessing at a shape.

/// The square the mark is fitted into, in logical (DPI-independent) pixels: it
/// keeps its aspect ratio inside this box, so a wide mark spans the full width and
/// leaves room above and below.
pub const LOGO_BOX: f32 = 250.0;

/// The most of a small window the box may take, so the mark still reads as a mark
/// rather than filling the frame when the window is near its minimum size.
const MAX_SPAN: f32 = 0.9;

/// How many points a curve is sampled at when measuring the logo's extent.
/// Control points can sit well outside the curve they bend, so the bounding box
/// is taken from points ON the curve - otherwise the mark would be centered
/// against a box larger than what is drawn.
const CURVE_SAMPLES: u32 = 16;

/// A point in the SVG's own coordinate space. The units are arbitrary: the
/// placement scales the mark to the window, so only their ratios matter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// One step of the outline. `Cubic` carries the two control points and the end
/// point; its start is the current point, as in SVG and GDI+ alike.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// The extent of the drawn mark in SVG coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Bounds {
    fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    fn height(&self) -> f32 {
        self.max_y - self.min_y
    }
}

/// Where the mark lands in a window: multiply each SVG point by `scale`, then
/// offset by `(dx, dy)`, to get its client-area position in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub scale: f32,
    pub dx: f32,
    pub dy: f32,
}

/// The parsed mark: its outline and the extent that outline occupies.
pub struct Logo {
    segments: Vec<Segment>,
    bounds: Bounds,
}

impl Logo {
    /// Read the mark out of an SVG document: the concatenated outlines of every
    /// `<path>` element in it. `None` when the document holds no usable path -
    /// no path element, an empty or malformed `d`, or a command outside the
    /// supported subset.
    pub fn parse(svg: &str) -> Option<Logo> {
        let mut segments = Vec::new();
        for data in path_data(svg) {
            parse_path_data(data, &mut segments)?;
        }
        let bounds = bounds(&segments)?;
        // A degenerate outline (everything on one line) has no area to scale
        // into a window, and would divide by zero below.
        if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
            return None;
        }
        Some(Logo { segments, bounds })
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Center the mark in a client area of `width` x `height` pixels, fitted into
    /// a `box_size` square, all in the same units (the painter works in physical
    /// pixels). The offsets already account for the outline's own origin, so a
    /// caller only applies `point * scale + offset`.
    pub fn placement(&self, width: f32, height: f32, box_size: f32) -> Placement {
        // Never wider or taller than a window smaller than the box itself.
        let box_size = box_size.min(width * MAX_SPAN).min(height * MAX_SPAN);
        let scale = (box_size / self.bounds.width()).min(box_size / self.bounds.height());
        Placement {
            scale,
            dx: (width - self.bounds.width() * scale) / 2.0 - self.bounds.min_x * scale,
            dy: (height - self.bounds.height() * scale) / 2.0 - self.bounds.min_y * scale,
        }
    }
}

/// The `d` attribute of every `<path>` element in the document, in order.
fn path_data(svg: &str) -> Vec<&str> {
    let mut data = Vec::new();
    for element in svg.split("<path").skip(1) {
        // Stop at the element's own end so a `d="..."` belonging to a later
        // element is never picked up for this one.
        let element = element.split('>').next().unwrap_or(element);
        if let Some((_, after)) = element.split_once(" d=\"")
            && let Some((value, _)) = after.split_once('"')
        {
            data.push(value);
        }
    }
    data
}

/// Append one path's data to `segments`, resolving relative commands against the
/// running current point. `None` on a malformed or unsupported path.
fn parse_path_data(data: &str, segments: &mut Vec<Segment>) -> Option<()> {
    let bytes = data.as_bytes();
    let mut at = 0usize;
    // The current point every relative coordinate is measured from, and the
    // subpath's start, which a closepath returns to.
    let mut current = Point { x: 0.0, y: 0.0 };
    let mut subpath_start = current;
    // The command in force, so a run of coordinates after it repeats it (SVG's
    // implicit repetition, with an extra moveto pair meaning lineto).
    let mut command = 0u8;
    // The second control point of the curve just added, which a smooth curveto
    // reflects about the current point to get its own first one. `None` after any
    // other command, where the reflection is the current point itself.
    let mut previous_control: Option<Point> = None;

    loop {
        skip_separators(bytes, &mut at);
        if at >= bytes.len() {
            return Some(());
        }

        if bytes[at].is_ascii_alphabetic() {
            command = bytes[at];
            at += 1;
        } else if command == 0 {
            // Coordinates before any command at all.
            return None;
        }

        let relative = command.is_ascii_lowercase();
        let mut curve_control = None;
        match command.to_ascii_uppercase() {
            b'M' => {
                let point = next_point(bytes, &mut at, current, relative)?;
                segments.push(Segment::Move(point));
                current = point;
                subpath_start = point;
                // A further coordinate pair under a moveto is a lineto.
                command = if relative { b'l' } else { b'L' };
            }
            b'L' => {
                let point = next_point(bytes, &mut at, current, relative)?;
                segments.push(Segment::Line(point));
                current = point;
            }
            b'C' => {
                let control1 = next_point(bytes, &mut at, current, relative)?;
                let control2 = next_point(bytes, &mut at, current, relative)?;
                let end = next_point(bytes, &mut at, current, relative)?;
                segments.push(Segment::Cubic(control1, control2, end));
                current = end;
                curve_control = Some(control2);
            }
            // The smooth curveto: its first control point is the previous curve's
            // second one mirrored through the current point, which is what keeps
            // the join smooth; after anything but a curve it is the current point.
            b'S' => {
                let control1 = match previous_control {
                    Some(control) => Point {
                        x: 2.0 * current.x - control.x,
                        y: 2.0 * current.y - control.y,
                    },
                    None => current,
                };
                let control2 = next_point(bytes, &mut at, current, relative)?;
                let end = next_point(bytes, &mut at, current, relative)?;
                segments.push(Segment::Cubic(control1, control2, end));
                current = end;
                curve_control = Some(control2);
            }
            b'Z' => {
                segments.push(Segment::Close);
                current = subpath_start;
                // A closepath takes no coordinates, so it cannot repeat: clearing
                // the command makes a number following it the malformed input it
                // is, rather than a closepath that consumes nothing and loops.
                command = 0;
            }
            // Every other SVG command (the quadratic curves, the arcs, the
            // horizontal/vertical linetos): not in this asset, so not guessed at.
            _ => return None,
        }
        previous_control = curve_control;
    }
}

/// Read one coordinate pair, making it absolute when the command is relative.
fn next_point(bytes: &[u8], at: &mut usize, current: Point, relative: bool) -> Option<Point> {
    let x = next_number(bytes, at)?;
    let y = next_number(bytes, at)?;
    Some(if relative {
        Point {
            x: current.x + x,
            y: current.y + y,
        }
    } else {
        Point { x, y }
    })
}

/// Read one number, skipping the separators before it.
fn next_number(bytes: &[u8], at: &mut usize) -> Option<f32> {
    skip_separators(bytes, at);
    let start = *at;

    if *at < bytes.len() && (bytes[*at] == b'-' || bytes[*at] == b'+') {
        *at += 1;
    }
    let mut digits = 0;
    let mut seen_dot = false;
    while *at < bytes.len() {
        match bytes[*at] {
            b'0'..=b'9' => digits += 1,
            // The fractional dot, once - a second one starts the next number
            // (SVG allows "1.5.5" for 1.5 then 0.5).
            b'.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        *at += 1;
    }
    if digits == 0 {
        return None;
    }
    // An exponent, which only counts when digits follow it.
    if *at < bytes.len() && (bytes[*at] | 0x20) == b'e' {
        let exponent = *at;
        let mut scan = *at + 1;
        if scan < bytes.len() && (bytes[scan] == b'-' || bytes[scan] == b'+') {
            scan += 1;
        }
        let mut exponent_digits = 0;
        while scan < bytes.len() && bytes[scan].is_ascii_digit() {
            scan += 1;
            exponent_digits += 1;
        }
        *at = if exponent_digits > 0 { scan } else { exponent };
    }

    std::str::from_utf8(&bytes[start..*at]).ok()?.parse().ok()
}

/// Skip the whitespace and commas between tokens.
fn skip_separators(bytes: &[u8], at: &mut usize) {
    while *at < bytes.len() && (bytes[*at].is_ascii_whitespace() || bytes[*at] == b',') {
        *at += 1;
    }
}

/// The extent of the outline, measured along the curves rather than around their
/// control points (see [`CURVE_SAMPLES`]). `None` for an outline with no points.
fn bounds(segments: &[Segment]) -> Option<Bounds> {
    let mut bounds: Option<Bounds> = None;
    let mut include = |point: Point| {
        bounds = Some(match bounds {
            None => Bounds {
                min_x: point.x,
                min_y: point.y,
                max_x: point.x,
                max_y: point.y,
            },
            Some(current) => Bounds {
                min_x: current.min_x.min(point.x),
                min_y: current.min_y.min(point.y),
                max_x: current.max_x.max(point.x),
                max_y: current.max_y.max(point.y),
            },
        });
    };

    let mut current = Point { x: 0.0, y: 0.0 };
    let mut subpath_start = current;
    for segment in segments {
        match *segment {
            Segment::Move(point) => {
                include(point);
                current = point;
                subpath_start = point;
            }
            Segment::Line(point) => {
                include(point);
                current = point;
            }
            Segment::Cubic(control1, control2, end) => {
                for step in 0..=CURVE_SAMPLES {
                    let t = step as f32 / CURVE_SAMPLES as f32;
                    include(cubic_at(current, control1, control2, end, t));
                }
                current = end;
            }
            Segment::Close => current = subpath_start,
        }
    }

    bounds
}

/// The point at `t` (0..1) along a cubic bezier.
fn cubic_at(start: Point, control1: Point, control2: Point, end: Point, t: f32) -> Point {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point {
        x: a * start.x + b * control1.x + c * control2.x + d * end.x,
        y: a * start.y + b * control1.y + c * control2.y + d * end.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    #[test]
    fn parses_absolute_and_relative_commands() {
        let logo = Logo::parse(r#"<svg><path d="M10,10 L20,10 l0,10 Z" /></svg>"#).expect("parsed");
        assert_eq!(
            logo.segments(),
            [
                Segment::Move(point(10.0, 10.0)),
                Segment::Line(point(20.0, 10.0)),
                // Relative to the previous point, not to the origin.
                Segment::Line(point(20.0, 20.0)),
                Segment::Close,
            ]
        );
    }

    #[test]
    fn repeats_the_command_across_a_run_of_coordinates() {
        // A moveto with a second pair continues as a lineto, and the pairs after
        // a curveto are further curves.
        let logo = Logo::parse(r#"<svg><path d="m0,0 10,0c1,1 2,2 3,3 4,4 5,5 6,6"/></svg>"#)
            .expect("parsed");
        assert_eq!(
            logo.segments(),
            [
                Segment::Move(point(0.0, 0.0)),
                Segment::Line(point(10.0, 0.0)),
                Segment::Cubic(point(11.0, 1.0), point(12.0, 2.0), point(13.0, 3.0)),
                Segment::Cubic(point(17.0, 7.0), point(18.0, 8.0), point(19.0, 9.0)),
            ]
        );
    }

    #[test]
    fn a_closepath_returns_to_the_subpath_start() {
        let logo = Logo::parse(r#"<svg><path d="M5,5 L15,5 Z l0,10"/></svg>"#).expect("parsed");
        // The relative lineto after the close measures from (5,5), the start of
        // the closed subpath, rather than from the closepath's own last point.
        assert_eq!(
            logo.segments().last(),
            Some(&Segment::Line(point(5.0, 15.0)))
        );
    }

    #[test]
    fn a_smooth_curveto_mirrors_the_previous_control_point() {
        let logo = Logo::parse(r#"<svg><path d="M0,0 C10,10 20,10 30,0 S45,5 50,0"/></svg>"#)
            .expect("parsed");
        // The smooth curve's own control point and end point are read as given;
        // its FIRST control point is the previous curve's second one (20,10)
        // mirrored through the current point (30,0), which continues its slope.
        assert_eq!(
            logo.segments().last(),
            Some(&Segment::Cubic(
                point(40.0, -10.0),
                point(45.0, 5.0),
                point(50.0, 0.0)
            ))
        );
    }

    #[test]
    fn rejects_a_document_without_a_usable_path() {
        // No path element at all, and a path carrying a command outside the
        // supported subset (an arc): neither yields a shape to draw.
        assert!(Logo::parse("<svg><rect width=\"10\" height=\"10\"/></svg>").is_none());
        assert!(Logo::parse(r#"<svg><path d="M0,0 A5,5 0 0 1 10,10"/></svg>"#).is_none());
        assert!(Logo::parse(r#"<svg><path d="M0,0 L10,0"/></svg>"#).is_none());
    }

    #[test]
    fn ignores_a_path_elements_other_attributes() {
        // The fill/id/stroke attributes around `d` must not be mistaken for it.
        let logo = Logo::parse(
            r##"<svg><path fill="#ffffff" id="d" d="M0,0 L10,0 L10,10 Z" stroke-width="0"/></svg>"##,
        )
        .expect("parsed");
        assert_eq!(logo.segments().len(), 4);
    }

    #[test]
    fn bounds_follow_the_curve_not_the_control_points() {
        // A curve whose control points sit far below it: the drawn shape is
        // 10 x 7.5, since it only dips to y = 7.5 while its control points reach
        // y = 10.
        let logo =
            Logo::parse(r#"<svg><path d="M0,0 C0,10 10,10 10,0 Z"/></svg>"#).expect("parsed");
        let placed = logo.placement(1000.0, 1000.0, 100.0);

        // The mark is wider than it is tall, so the box's width binds: 10 units
        // across become 100. The vertical centering is what tells the two extents
        // apart - the drawn shape is 7.5 tall (75 scaled), not the 10 (100) its
        // control points span.
        assert!((placed.scale - 10.0).abs() < 0.01, "{placed:?}");
        assert!(
            (placed.dy - (1000.0 - 75.0) / 2.0).abs() < 0.1,
            "{placed:?}"
        );
    }

    #[test]
    fn placement_fits_the_mark_into_the_box_and_centers_it() {
        let logo =
            Logo::parse(r#"<svg><path d="M0,0 L100,0 L100,100 L0,100 Z"/></svg>"#).expect("parsed");
        let placed = logo.placement(1000.0, 500.0, 250.0);

        // A square mark takes the whole box...
        assert!((placed.scale * 100.0 - 250.0).abs() < 0.01);
        // ... centered in the window, not in the box.
        assert!((placed.dx - (1000.0 - 250.0) / 2.0).abs() < 0.01);
        assert!((placed.dy - (500.0 - 250.0) / 2.0).abs() < 0.01);
    }

    #[test]
    fn placement_shrinks_the_box_to_fit_a_small_window() {
        let logo =
            Logo::parse(r#"<svg><path d="M0,0 L100,0 L100,100 L0,100 Z"/></svg>"#).expect("parsed");
        // A window shorter than the box: the mark is held to MAX_SPAN of it rather
        // than running off the top and bottom edges.
        let placed = logo.placement(1000.0, 200.0, 250.0);

        assert!((placed.scale * 100.0 - 180.0).abs() < 0.01, "{placed:?}");
    }

    // The shipped asset must parse, or the splash would show a bare background.
    #[test]
    fn the_shipped_logo_parses_and_is_wider_than_it_is_tall() {
        let logo = Logo::parse(super::super::LOGO_SVG).expect("the shipped logo parses");
        assert!(logo.segments().len() > 20);

        let placed = logo.placement(1000.0, 1000.0, LOGO_BOX);
        // The mark is a wide shape, so its width is what the box binds: it spans
        // the full box across and leaves room above and below.
        assert!((placed.scale * logo.bounds.width() - LOGO_BOX).abs() < 0.01);
        assert!(placed.scale * logo.bounds.height() < LOGO_BOX);
        assert!(logo.bounds.width() > logo.bounds.height());
    }
}
