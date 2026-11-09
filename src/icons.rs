//! Small tool icons drawn with lines, so no picture files are needed.

use egui::{vec2, Align2, Color32, FontId, Painter, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    New, Open, Save, Import, Export,
    SelectAll, Cut, Copy, Paste, Undo, Redo,
    Circle, Ellipse, Rect, Polygon, Star,
    Lines, Arc, Curve, Spiral, Text, Measure,
    Move, Scale, Rotate, Mirror, Align,
    Select, Nodes, Group, Ungroup, UngroupAll, MakePiece, SelectPiece, AdjustPiece, ReleasePiece, Join, Close, Weld, Subtract, Intersect, Delete,
    Offset, BlockCopy, CircularCopy,
    Dome, Layer, Up, Down,
    TpProfile, TpPocket, TpDrill, TpEngrave, TpVCarve, TpRough, TpFinish,
    Tools, Edit, Duplicate, Trash, Clock, Preview, Recalc, SaveTp,
    Snap, SnapObjects, Grid, Pan, ZoomBox, ZoomFit, ZoomSel, ShowVectors, ShowPaths, ShowModel, TileSide, TileStacked, FlipAroundX, FlipAroundY, SideTop, SideBottom,
    ViewFit, ViewIso, ViewZ, ViewX, ViewY,
    Play, Pause, Step, End, Rewind, Pin, Collapse,
    Doc, Folder, Sample,
    TextCurve, Dimension, Trim, Fillet, Validate, CopyAlong, Nest,
    TpPrism, TpTexture, TpMoulding, Eye, EyeOff, Locked, Unlocked,
    Similar, Bulk, Constraint, Trace, Distort, JobSetup, ReliefPicture, ImportModel, ZeroPlane, TpFlute, TpChamfer, SnapShapes,
    RDimension, RCoincident, RHorizontal, RVertical, RParallel, RPerpendicular, REqual, RCollinear,
    RTangent, RConcentric, RMidpoint, RPointOn, RSymmetric, RFixed, RConstruction,
}

const BLUE: Color32 = Color32::from_rgb(158, 186, 245);
const SKY: Color32 = Color32::from_rgb(112, 160, 236);
const YELLOW: Color32 = Color32::from_rgb(226, 170, 74);
const RED: Color32 = Color32::from_rgb(244, 118, 100);
const BLACK: Color32 = Color32::from_rgb(233, 235, 238);
const INK: Color32 = Color32::from_rgb(40, 34, 28);
const PAPER: Color32 = Color32::from_rgb(92, 100, 114);
const WOOD: Color32 = Color32::from_rgb(214, 160, 104);
const WOOD_DARK: Color32 = Color32::from_rgb(142, 92, 52);
const TEAL: Color32 = Color32::from_rgb(70, 205, 190);
const SLATE: Color32 = Color32::from_rgb(58, 66, 80);
const SLATE_EDGE: Color32 = Color32::from_rgb(96, 108, 128);

thread_local! {
    /// What the icons' teal is drawn in for now: the colour of the section they're in.
    static TINT: std::cell::Cell<Option<Color32>> = const { std::cell::Cell::new(None) };
}

/// Icons drawn from here on take this colour where they'd be teal; None for teal again.
pub fn set_tint(colour: Option<Color32>) {
    TINT.with(|t| t.set(colour));
}

fn faded(c: Color32) -> Color32 {
    // Blended towards the panel grey, still tinted so it reads as a dimmed icon.
    let mix = |v: u8, panel: f32| (v as f32 * 0.3 + panel * 0.7) as u8;
    Color32::from_rgb(mix(c.r(), 54.0), mix(c.g(), 58.0), mix(c.b(), 66.0))
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, enabled: bool) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 24.0;
    let tint = TINT.with(|t| t.get());
    let tone = |col: Color32| {
        // The section's colour for the teal, and a pale version of it for the light lines.
        let pale = |t: Color32| Color32::from_rgb(((t.r() as u16 + 2 * 236) / 3) as u8, ((t.g() as u16 + 2 * 238) / 3) as u8, ((t.b() as u16 + 2 * 241) / 3) as u8);
        let col = match tint {
            Some(t) if col == TEAL => t,
            Some(t) if col == BLACK => pale(t),
            // The slate of boxes and outlines, leaning towards it.
            Some(t) if col == PAPER || col == SLATE_EDGE => Color32::from_rgb(((t.r() as u16 + col.r() as u16) / 2) as u8, ((t.g() as u16 + col.g() as u16) / 2) as u8, ((t.b() as u16 + col.b() as u16) / 2) as u8),
            _ => col,
        };
        if enabled { col } else { faded(col) }
    };
    let at = |x: f32, y: f32| c + vec2(x, y) * s;
    let stroke = |col: Color32, w: f32| Stroke::new(w * s.max(0.8), tone(col));
    let line = |pts: &[(f32, f32)], col: Color32, w: f32| {
        p.add(Shape::line(pts.iter().map(|q| at(q.0, q.1)).collect(), stroke(col, w)));
    };
    let closed = |pts: &[(f32, f32)], col: Color32, w: f32| {
        p.add(Shape::closed_line(pts.iter().map(|q| at(q.0, q.1)).collect(), stroke(col, w)));
    };
    let fill = |pts: &[(f32, f32)], col: Color32| {
        p.add(Shape::convex_polygon(pts.iter().map(|q| at(q.0, q.1)).collect(), tone(col), Stroke::NONE));
    };
    let rect_at = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    let boxed = |x0: f32, y0: f32, x1: f32, y1: f32, col: Color32, w: f32| {
        p.rect_stroke(rect_at(x0, y0, x1, y1), 1.0, stroke(col, w), StrokeKind::Inside);
    };
    let filled = |x0: f32, y0: f32, x1: f32, y1: f32, col: Color32| {
        p.rect_filled(rect_at(x0, y0, x1, y1), 0.5, tone(col));
    };
    let dot = |x: f32, y: f32, col: Color32| {
        p.circle_filled(at(x, y), 1.6 * s, tone(col));
    };
    // The drawing tools' own look: a soft filled shape with a light outline, and small square
    // handles where it can be grabbed.
    let wash = |col: Color32| {
        let t = tone(col);
        Color32::from_rgba_unmultiplied(t.r(), t.g(), t.b(), 70)
    };
    let soft = |pts: &[(f32, f32)], col: Color32| {
        // A plain mesh fanned from the middle: shapes that aren't convex fill too, and without
        // the smoothed edges a flat sliver of a triangle would stretch far out of the icon.
        let mid = pts.iter().fold((0.0, 0.0), |a, q| (a.0 + q.0 / pts.len() as f32, a.1 + q.1 / pts.len() as f32));
        let colour = wash(col);
        let mut mesh = egui::epaint::Mesh::default();
        mesh.colored_vertex(at(mid.0, mid.1), colour);
        for q in pts {
            mesh.colored_vertex(at(q.0, q.1), colour);
        }
        let n = pts.len() as u32;
        for i in 0..n {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        p.add(Shape::mesh(std::sync::Arc::new(mesh)));
    };
    let handle = |x: f32, y: f32| {
        let r = Rect::from_center_size(at(x, y), vec2(3.2 * s, 3.2 * s));
        p.rect_filled(r, 0.0, tone(BLACK));
        p.rect_stroke(r, 0.0, Stroke::new(0.8 * s.max(0.8), tone(INK)), StrokeKind::Inside);
    };
    // The toolpath icons' base: a slate plate, with the cut drawn on it in teal.
    let plate = || {
        p.rect_filled(rect_at(-10.5, -9.5, 10.5, 9.5), 3.0 * s, tone(SLATE));
        p.rect_stroke(rect_at(-10.5, -9.5, 10.5, 9.5), 3.0 * s, stroke(SLATE_EDGE, 1.0), StrokeKind::Inside);
    };
    let ring = |x: f32, y: f32, r: f32, col: Color32, w: f32| {
        p.circle_stroke(at(x, y), r * s, stroke(col, w));
    };
    let word = |t: &str, size: f32, col: Color32| {
        p.text(c, Align2::CENTER_CENTER, t, FontId::proportional(size * s), tone(col));
    };
    // A wooden tile: the shared base of the toolpath icons.
    let tile = || {
        fill(&[(-10.0, -4.0), (5.0, -8.0), (10.0, 4.0), (-5.0, 8.0)], WOOD);
        closed(&[(-10.0, -4.0), (5.0, -8.0), (10.0, 4.0), (-5.0, 8.0)], WOOD_DARK, 1.2);
    };

    match icon {
        Icon::New | Icon::Doc => {
            let page = [(-7.0, -9.0), (2.0, -9.0), (7.0, -4.0), (7.0, 9.0), (-7.0, 9.0)];
            fill(&page, SLATE);
            closed(&page, SLATE_EDGE, 1.1);
            if icon == Icon::New {
                p.circle_filled(at(4.0, 5.0), 5.0 * s, tone(TEAL));
                line(&[(1.5, 5.0), (6.5, 5.0)], INK, 1.6);
                line(&[(4.0, 2.5), (4.0, 7.5)], INK, 1.6);
            } else {
                for y in [-3.0, 0.5, 4.0] {
                    line(&[(-4.0, y), (4.0, y)], TEAL, 1.0);
                }
            }
        }
        Icon::Open | Icon::Folder => {
            // A tray with what's in it coming up and out.
            line(&[(-9.0, 1.0), (-9.0, 8.0), (9.0, 8.0), (9.0, 1.0)], SLATE_EDGE, 1.8);
            soft(&[(-9.0, 3.0), (9.0, 3.0), (9.0, 8.0), (-9.0, 8.0)], TEAL);
            line(&[(0.0, 4.0), (0.0, -8.0)], TEAL, 2.0);
            line(&[(-4.0, -4.0), (0.0, -8.5), (4.0, -4.0)], TEAL, 2.0);
        }
        Icon::Save | Icon::SaveTp => {
            // Down into a tray.
            line(&[(-9.0, 1.0), (-9.0, 8.0), (9.0, 8.0), (9.0, 1.0)], SLATE_EDGE, 1.8);
            soft(&[(-9.0, 3.0), (9.0, 3.0), (9.0, 8.0), (-9.0, 8.0)], TEAL);
            line(&[(0.0, -9.0), (0.0, 3.5)], TEAL, 2.0);
            line(&[(-4.0, -0.5), (0.0, 4.0), (4.0, -0.5)], TEAL, 2.0);
            if icon == Icon::SaveTp {
                line(&[(-7.0, 11.0), (-3.0, 9.5), (1.0, 11.0), (5.0, 9.5)], BLACK, 1.0);
            }
        }
        Icon::Import => {
            // In from the side, into the tray.
            line(&[(-9.0, 1.0), (-9.0, 8.0), (9.0, 8.0), (9.0, 1.0)], SLATE_EDGE, 1.8);
            line(&[(9.0, -8.0), (1.0, -8.0), (1.0, 4.0)], TEAL, 2.0);
            line(&[(-2.5, 0.5), (1.0, 4.5), (4.5, 0.5)], TEAL, 2.0);
        }
        Icon::Export => {
            // Up out of the tray and away to the side.
            line(&[(-9.0, 1.0), (-9.0, 8.0), (9.0, 8.0), (9.0, 1.0)], SLATE_EDGE, 1.8);
            line(&[(-1.0, 4.0), (-1.0, -6.0), (8.0, -6.0)], TEAL, 2.0);
            line(&[(4.5, -9.5), (8.5, -6.0), (4.5, -2.5)], TEAL, 2.0);
        }
        Icon::SelectAll => {
            boxed(-8.0, -8.0, 8.0, 8.0, BLUE, 1.0);
            filled(-4.0, -4.0, 4.0, 4.0, BLACK);
            for (x, y) in [(-8.0, -8.0), (8.0, -8.0), (-8.0, 8.0), (8.0, 8.0)] {
                dot(x, y, BLACK);
            }
        }
        Icon::Cut => {
            line(&[(-5.0, -9.0), (4.0, 4.0)], Color32::from_gray(190), 1.5);
            line(&[(5.0, -9.0), (-4.0, 4.0)], Color32::from_gray(190), 1.5);
            ring(-5.0, 7.0, 3.0, RED, 1.4);
            ring(5.0, 7.0, 3.0, RED, 1.4);
        }
        Icon::Copy | Icon::Duplicate => {
            p.rect_filled(rect_at(-9.0, -9.0, 3.0, 3.0), 2.5 * s, tone(SLATE));
            p.rect_stroke(rect_at(-9.0, -9.0, 3.0, 3.0), 2.5 * s, stroke(SLATE_EDGE, 1.2), StrokeKind::Inside);
            p.rect_filled(rect_at(-3.0, -3.0, 9.0, 9.0), 2.5 * s, wash(TEAL));
            p.rect_stroke(rect_at(-3.0, -3.0, 9.0, 9.0), 2.5 * s, stroke(TEAL, 1.5), StrokeKind::Inside);
        }
        Icon::Paste => {
            filled(-8.0, -7.0, 7.0, 9.0, Color32::from_rgb(190, 150, 90));
            filled(-3.0, -9.0, 2.0, -5.0, Color32::from_gray(110));
            fill(&[(-3.0, -2.0), (9.0, -2.0), (9.0, 10.0), (-3.0, 10.0)], PAPER);
            closed(&[(-3.0, -2.0), (9.0, -2.0), (9.0, 10.0), (-3.0, 10.0)], BLUE, 1.0);
        }
        Icon::Undo | Icon::Redo => {
            let k = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let curve: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.1 - i as f32 / 12.0 * 1.3);
                    (k * 6.0 * a.cos(), 2.0 - 6.0 * a.sin())
                })
                .collect();
            line(&curve, SKY, 2.2);
            line(&[(k * -8.0, -5.0), (k * -6.5, 2.5), (k * -1.0, -1.0)], BLUE, 1.6);
        }
        Icon::Circle => {
            p.circle_filled(at(0.0, 0.0), 8.5 * s, wash(TEAL));
            ring(0.0, 0.0, 8.5, TEAL, 1.6);
            // Its radius, from the middle out to a handle.
            line(&[(0.0, 0.0), (6.0, -6.0)], BLACK, 1.1);
            handle(6.0, -6.0);
        }
        Icon::Ellipse => {
            // Tilted, with its long axis drawn across.
            let (sa, ca) = (-0.45f32).sin_cos();
            let turn = |x: f32, y: f32| (x * ca - y * sa, x * sa + y * ca);
            let pts: Vec<(f32, f32)> = (0..32)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 32.0;
                    turn(10.0 * a.cos(), 5.0 * a.sin())
                })
                .collect();
            soft(&pts, TEAL);
            closed(&pts, TEAL, 1.6);
            let (e0, e1) = (turn(-10.0, 0.0), turn(10.0, 0.0));
            line(&[e0, e1], BLACK, 0.9);
            handle(e1.0, e1.1);
        }
        Icon::Rect => {
            p.rect_filled(rect_at(-9.0, -6.5, 9.0, 6.5), 3.0 * s, wash(TEAL));
            p.rect_stroke(rect_at(-9.0, -6.5, 9.0, 6.5), 3.0 * s, stroke(TEAL, 1.6), StrokeKind::Inside);
            handle(9.0, 6.5);
        }
        Icon::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 6.0;
                    (9.5 * a.cos(), 9.5 * a.sin())
                })
                .collect();
            soft(&pts, TEAL);
            closed(&pts, TEAL, 1.6);
            handle(9.5, 0.0);
        }
        Icon::Star => {
            let pts: Vec<(f32, f32)> = (0..12)
                .map(|i| {
                    let a = std::f32::consts::TAU * (i as f32 / 12.0 - 0.25);
                    let r = if i % 2 == 0 { 10.0 } else { 5.0 };
                    (r * a.cos(), r * a.sin())
                })
                .collect();
            soft(&pts, TEAL);
            closed(&pts, TEAL, 1.4);
        }
        Icon::Lines => {
            let pts = [(-10.0, -6.0), (-5.0, 7.0), (0.0, -2.0), (5.0, 7.0), (10.0, -6.0)];
            line(&pts, TEAL, 2.0);
            for q in pts {
                handle(q.0, q.1);
            }
        }
        Icon::Arc => {
            // An arch over a dotted chord.
            let pts: Vec<(f32, f32)> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    (9.5 * a.cos(), 5.0 + 11.0 * a.sin())
                })
                .collect();
            line(&pts, TEAL, 2.0);
            for k in 0..5 {
                let x = -8.0 + k as f32 * 4.0;
                line(&[(x, 5.0), (x + 2.0, 5.0)], BLACK, 0.9);
            }
            handle(-9.5, 5.0);
            handle(9.5, 5.0);
        }
        Icon::Curve => {
            // A wave across, with the pull of one of its controls.
            let pts: Vec<(f32, f32)> = (0..=28)
                .map(|i| {
                    let t = i as f32 / 28.0;
                    (-10.0 + 20.0 * t, 5.0 * (std::f32::consts::TAU * t).sin())
                })
                .collect();
            line(&pts, TEAL, 2.2);
            line(&[(-5.0, -5.0), (-1.0, -9.0)], BLACK, 0.9);
            p.circle_filled(at(-1.0, -9.0), 1.5 * s, tone(BLACK));
            handle(-10.0, 0.0);
            handle(10.0, 0.0);
        }
        Icon::Spiral => {
            let pts: Vec<(f32, f32)> = (0..=60)
                .map(|i| {
                    let t = i as f32 / 60.0;
                    let a = std::f32::consts::TAU * 2.5 * t;
                    (9.5 * t * a.cos(), 9.5 * t * a.sin())
                })
                .collect();
            line(&pts, TEAL, 1.8);
            handle(pts[60].0, pts[60].1);
        }
        Icon::Text => {
            p.text(at(-4.5, 1.0), Align2::CENTER_CENTER, "A", FontId::proportional(20.0 * s), tone(TEAL));
            p.text(at(5.0, 3.0), Align2::CENTER_CENTER, "a", FontId::proportional(14.0 * s), tone(BLACK));
        }
        Icon::Measure => {
            // A ruler laid across, with its marks.
            let r = [(-10.0, 1.0), (10.0, 1.0), (10.0, 7.0), (-10.0, 7.0)];
            fill(&r, WOOD);
            closed(&r, WOOD_DARK, 1.0);
            for k in 0..9 {
                let x = -8.0 + k as f32 * 2.0;
                let tall = if k % 2 == 0 { 3.0 } else { 1.8 };
                line(&[(x, 1.0), (x, 1.0 + tall)], INK, 0.8);
            }
            line(&[(-8.0, -6.0), (8.0, -6.0)], TEAL, 1.2);
            handle(-8.0, -6.0);
            handle(8.0, -6.0);
        }
        Icon::Move => {
            // A shape with arrows out on all four sides.
            p.rect_filled(rect_at(-3.5, -3.5, 3.5, 3.5), 1.5 * s, wash(TEAL));
            p.rect_stroke(rect_at(-3.5, -3.5, 3.5, 3.5), 1.5 * s, stroke(TEAL, 1.3), StrokeKind::Inside);
            for (dx, dy) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let (tx, ty) = (dx * 10.0, dy * 10.0);
                line(&[(dx * 5.5, dy * 5.5), (tx, ty)], BLACK, 1.2);
                fill(&[(tx + dx * 0.5, ty + dy * 0.5), (tx - dx * 3.0 - dy * 2.5, ty - dy * 3.0 + dx * 2.5), (tx - dx * 3.0 + dy * 2.5, ty - dy * 3.0 - dx * 2.5)], BLACK);
            }
        }
        Icon::Scale => {
            // A small square grown into a large one from its corner.
            p.rect_stroke(rect_at(-9.0, -9.0, 9.0, 9.0), 2.0 * s, stroke(SLATE_EDGE, 1.2), StrokeKind::Inside);
            p.rect_filled(rect_at(-9.0, 1.0, 1.0, 9.0), 1.5 * s, wash(TEAL));
            p.rect_stroke(rect_at(-9.0, 1.0, 1.0, 9.0), 1.5 * s, stroke(TEAL, 1.3), StrokeKind::Inside);
            line(&[(-1.0, 1.0), (7.0, -7.0)], BLACK, 1.2);
            line(&[(2.5, -7.0), (7.0, -7.0), (7.0, -2.5)], BLACK, 1.2);
        }
        Icon::Rotate => {
            // A tilted shape with the way it turns.
            let r = [(0.0, -6.0), (6.0, 0.0), (0.0, 6.0), (-6.0, 0.0)];
            soft(&r, TEAL);
            closed(&r, TEAL, 1.3);
            let pts: Vec<(f32, f32)> = (0..=18)
                .map(|i| {
                    let a = std::f32::consts::TAU * (0.6 + 0.55 * i as f32 / 18.0);
                    (9.5 * a.cos(), 9.5 * a.sin())
                })
                .collect();
            line(&pts, BLACK, 1.2);
            let (tx, ty) = pts[18];
            fill(&[(tx + 2.0, ty - 2.5), (tx + 2.5, ty + 2.5), (tx - 2.5, ty + 1.0)], BLACK);
        }
        Icon::Mirror => {
            // A triangle and its reflection across a dashed line.
            for k in 0..5 {
                let y = -10.0 + k as f32 * 4.5;
                line(&[(0.0, y), (0.0, y + 2.5)], BLACK, 1.0);
            }
            soft(&[(-2.5, -6.0), (-2.5, 7.0), (-10.0, 7.0)], TEAL);
            closed(&[(-2.5, -6.0), (-2.5, 7.0), (-10.0, 7.0)], TEAL, 1.3);
            closed(&[(2.5, -6.0), (2.5, 7.0), (10.0, 7.0)], SLATE_EDGE, 1.3);
        }
        Icon::Align => {
            // Bars of different lengths, all against one line.
            line(&[(-9.0, -10.0), (-9.0, 10.0)], BLACK, 1.2);
            for (y, len) in [(-6.0, 16.0), (0.0, 10.0), (6.0, 13.0)] {
                p.rect_filled(rect_at(-8.0, y - 2.0, -8.0 + len, y + 2.0), 1.0 * s, wash(TEAL));
                p.rect_stroke(rect_at(-8.0, y - 2.0, -8.0 + len, y + 2.0), 1.0 * s, stroke(TEAL, 1.1), StrokeKind::Inside);
            }
        }
        Icon::Select => {
            fill(&[(-5.0, -10.0), (6.0, 2.0), (1.0, 2.0)], PAPER);
            fill(&[(-5.0, -10.0), (1.0, 2.0), (-5.0, 6.0)], PAPER);
            closed(&[(-5.0, -10.0), (6.0, 2.0), (1.0, 2.0), (4.0, 8.0), (1.0, 10.0), (-2.0, 3.0), (-5.0, 6.0)], BLACK, 1.1);
        }
        Icon::Nodes => {
            line(&[(-8.0, 6.0), (0.0, -7.0), (8.0, 6.0)], RED, 1.3);
            for (x, y) in [(-8.0, 6.0), (0.0, -7.0), (8.0, 6.0)] {
                filled(x - 2.0, y - 2.0, x + 2.0, y + 2.0, YELLOW);
                boxed(x - 2.0, y - 2.0, x + 2.0, y + 2.0, BLACK, 0.8);
            }
        }
        Icon::Group => {
            // Two shapes held inside one frame.
            boxed(-10.0, -10.0, 10.0, 10.0, YELLOW, 1.2);
            boxed(-7.0, -7.0, -0.5, -0.5, BLUE, 1.4);
            ring(3.5, 3.5, 3.4, BLUE, 1.4);
        }
        Icon::Ungroup | Icon::UngroupAll => {
            // The group's frame broken open, around one shape to a side, or more when it's every level.
            line(&[(-10.5, -4.0), (-10.5, -10.5), (-4.0, -10.5)], RED, 1.5);
            line(&[(4.0, -10.5), (10.5, -10.5), (10.5, -4.0)], RED, 1.5);
            line(&[(10.5, 4.0), (10.5, 10.5), (4.0, 10.5)], RED, 1.5);
            line(&[(-4.0, 10.5), (-10.5, 10.5), (-10.5, 4.0)], RED, 1.5);
            if icon == Icon::Ungroup {
                boxed(-7.0, -7.0, -0.5, -0.5, BLUE, 1.4);
                ring(3.5, 3.5, 3.4, BLUE, 1.4);
            } else {
                boxed(-7.0, -7.0, -2.0, -2.0, BLUE, 1.3);
                ring(4.5, -4.5, 2.5, BLUE, 1.3);
                ring(-4.5, 4.5, 2.5, BLUE, 1.3);
                boxed(2.0, 2.0, 7.0, 7.0, BLUE, 1.3);
            }
        }
        Icon::MakePiece => {
            boxed(-10.0, -4.0, 8.0, 10.0, TEAL, 1.4);
            ring(-5.0, 3.0, 2.0, BLUE, 1.2);
            ring(3.0, 3.0, 2.0, BLUE, 1.2);
            line(&[(7.5, -11.0), (7.5, -3.0)], YELLOW, 1.8);
            line(&[(3.5, -7.0), (11.5, -7.0)], YELLOW, 1.8);
        }
        Icon::SelectPiece => {
            boxed(-11.0, -10.0, 7.0, 4.0, TEAL, 1.4);
            ring(-6.5, -3.0, 2.0, BLUE, 1.2);
            ring(1.5, -3.0, 2.0, BLUE, 1.2);
            fill(&[(1.0, 0.0), (9.0, 7.5), (4.5, 8.0)], PAPER);
            closed(&[(1.0, 0.0), (9.5, 7.0), (6.0, 7.5), (8.0, 11.0), (6.0, 12.0), (4.0, 8.5), (1.5, 10.5)], BLACK, 1.1);
        }
        Icon::AdjustPiece => {
            boxed(-11.0, -8.0, 11.0, 8.0, TEAL, 1.4);
            ring(-5.5, 0.0, 2.2, BLUE, 1.2);
            // The one shape moving on its own, with arrows out of it.
            ring(4.0, 0.0, 2.4, RED, 1.5);
            line(&[(4.0, -6.0), (4.0, -3.4)], RED, 1.3);
            line(&[(4.0, 3.4), (4.0, 6.0)], RED, 1.3);
            line(&[(-1.5, 0.0), (0.6, 0.0)], RED, 1.3);
            line(&[(7.4, 0.0), (9.5, 0.0)], RED, 1.3);
        }
        Icon::ReleasePiece => {
            // The piece's outline broken open.
            line(&[(-10.5, -2.5), (-10.5, -8.0), (-4.5, -8.0)], TEAL, 1.5);
            line(&[(4.5, -8.0), (10.5, -8.0), (10.5, -2.5)], TEAL, 1.5);
            line(&[(10.5, 2.5), (10.5, 8.0), (4.5, 8.0)], TEAL, 1.5);
            line(&[(-4.5, 8.0), (-10.5, 8.0), (-10.5, 2.5)], TEAL, 1.5);
            ring(-4.0, 0.0, 2.2, BLUE, 1.2);
            ring(4.0, 0.0, 2.2, BLUE, 1.2);
        }
        Icon::Join => {
            line(&[(-9.0, 6.0), (-3.0, 0.0)], BLUE, 1.6);
            line(&[(3.0, 0.0), (9.0, -6.0)], BLUE, 1.6);
            line(&[(-3.0, 0.0), (3.0, 0.0)], RED, 1.6);
            dot(-3.0, 0.0, YELLOW);
            dot(3.0, 0.0, YELLOW);
        }
        Icon::Close => {
            line(&[(-8.0, 8.0), (-8.0, -8.0), (8.0, -8.0), (8.0, 8.0)], BLUE, 1.6);
            line(&[(-8.0, 8.0), (8.0, 8.0)], RED, 1.6);
        }
        Icon::Weld => {
            let a: Vec<(f32, f32)> = (0..=26)
                .map(|i| {
                    let t = std::f32::consts::TAU * (0.14 + 0.72 * i as f32 / 26.0);
                    (-3.0 + 6.5 * t.cos(), 6.5 * t.sin())
                })
                .collect();
            let b: Vec<(f32, f32)> = (0..=26)
                .map(|i| {
                    let t = std::f32::consts::TAU * (0.64 + 0.72 * i as f32 / 26.0);
                    (3.0 + 6.5 * t.cos(), 6.5 * t.sin())
                })
                .collect();
            line(&a, BLUE, 1.5);
            line(&b, BLUE, 1.5);
        }
        Icon::Subtract => {
            ring(-3.0, 0.0, 6.5, BLUE, 1.5);
            ring(4.0, 0.0, 6.5, RED, 1.1);
        }
        Icon::Intersect => {
            ring(-3.0, 0.0, 6.5, SKY, 1.0);
            ring(3.0, 0.0, 6.5, SKY, 1.0);
            filled(-2.5, -3.0, 2.5, 3.0, BLUE);
        }
        Icon::Delete | Icon::Trash => {
            // A bin with its lid.
            line(&[(-9.0, -6.0), (9.0, -6.0)], RED, 1.8);
            line(&[(-3.0, -6.0), (-2.0, -9.0), (2.0, -9.0), (3.0, -6.0)], RED, 1.4);
            closed(&[(-6.5, -4.0), (6.5, -4.0), (5.0, 9.0), (-5.0, 9.0)], RED, 1.6);
            for x in [-2.5, 2.5] {
                line(&[(x, -1.0), (x * 0.8, 6.0)], RED, 1.0);
            }
        }
        Icon::Offset => {
            boxed(-10.0, -10.0, 10.0, 10.0, RED, 1.2);
            boxed(-5.0, -5.0, 5.0, 5.0, BLUE, 1.5);
        }
        Icon::BlockCopy => {
            for (x, y) in [(-9.0, -9.0), (1.0, -9.0), (-9.0, 1.0), (1.0, 1.0)] {
                boxed(x, y, x + 8.0, y + 8.0, BLACK, 1.3);
            }
        }
        Icon::CircularCopy => {
            for i in 0..6 {
                let a = std::f32::consts::TAU * i as f32 / 6.0;
                ring(7.0 * a.cos(), 7.0 * a.sin(), 2.6, BLUE, 1.2);
            }
        }
        Icon::Dome => {
            tile();
            p.circle_filled(at(0.0, -1.0), 5.5 * s, tone(Color32::from_rgb(240, 200, 150)));
            p.circle_filled(at(-1.5, -2.5), 2.5 * s, tone(Color32::from_rgb(252, 234, 204)));
        }
        Icon::Layer => {
            closed(&[(-9.0, 3.0), (0.0, 7.0), (9.0, 3.0), (0.0, -1.0)], Color32::from_gray(200), 1.2);
            fill(&[(-9.0, -2.0), (0.0, -6.5), (9.0, -2.0), (0.0, 2.5)], Color32::from_gray(180));
            closed(&[(-9.0, -2.0), (0.0, -6.5), (9.0, -2.0), (0.0, 2.5)], Color32::from_gray(200), 1.2);
        }
        Icon::Up | Icon::Down => {
            let k = if icon == Icon::Up { 1.0 } else { -1.0 };
            let pts = [(0.0, -9.0 * k), (8.0, -1.0 * k), (3.0, -1.0 * k), (3.0, 8.0 * k), (-3.0, 8.0 * k), (-3.0, -1.0 * k), (-8.0, -1.0 * k)];
            closed(&pts, Color32::from_gray(190), 1.2);
        }
        Icon::TpProfile => {
            plate();
            p.rect_stroke(rect_at(-6.5, -5.5, 6.5, 5.5), 2.5 * s, stroke(TEAL, 1.8), StrokeKind::Middle);
            // Which way the cut goes round.
            line(&[(-1.5, -8.0), (1.5, -5.5), (-1.5, -3.0)], BLACK, 1.3);
        }
        Icon::TpPocket => {
            plate();
            p.rect_filled(rect_at(-7.0, -6.0, 7.0, 6.0), 2.5 * s, wash(TEAL));
            for (k, r) in [(0.0, 2.5), (2.6, 1.6), (5.0, 0.8)] {
                p.rect_stroke(rect_at(-7.0 + k, -6.0 + k, 7.0 - k, 6.0 - k), r * s, stroke(TEAL, 1.3), StrokeKind::Middle);
            }
        }
        Icon::TpDrill => {
            plate();
            for (x, y) in [(-5.0, 3.0), (0.0, -3.5), (5.0, 3.0)] {
                ring(x, y, 2.6, TEAL, 1.3);
                line(&[(x - 4.0, y), (x + 4.0, y)], BLACK, 0.7);
                line(&[(x, y - 4.0), (x, y + 4.0)], BLACK, 0.7);
            }
        }
        Icon::TpEngrave => {
            plate();
            // A single line, as an engraving tool follows it.
            let pts: Vec<(f32, f32)> = (0..=40).map(|i| {
                let t = i as f32 / 40.0;
                let a = std::f32::consts::TAU * 1.5 * t;
                (-8.0 + 16.0 * t + 2.5 * a.sin(), 3.0 - 5.0 * a.cos() * (1.0 - t * 0.4))
            }).collect();
            line(&pts, TEAL, 1.4);
        }
        Icon::TpVCarve => {
            plate();
            // The groove a V-bit leaves, seen end on.
            soft(&[(-4.0, -3.0), (4.0, -3.0), (0.0, 6.0)], TEAL);
            line(&[(-9.0, -3.0), (-4.0, -3.0), (0.0, 6.0), (4.0, -3.0), (9.0, -3.0)], TEAL, 1.6);
        }
        Icon::TpRough => {
            plate();
            // Terraces, level by level.
            line(&[(-9.0, 6.5), (-9.0, 3.0), (-5.5, 3.0), (-5.5, -0.5), (-2.0, -0.5), (-2.0, -4.0), (2.0, -4.0), (2.0, -0.5), (5.5, -0.5), (5.5, 3.0), (9.0, 3.0), (9.0, 6.5)], TEAL, 1.6);
        }
        Icon::TpFinish => {
            plate();
            // A smooth hill, with the fine passes that leave it so.
            let hill: Vec<(f32, f32)> = (0..=20).map(|i| {
                let t = i as f32 / 20.0 * 2.0 - 1.0;
                (t * 9.0, 6.0 - 10.0 * (1.0 - t * t))
            }).collect();
            for k in -3..=3 {
                let x = k as f32 * 2.4;
                let t = x / 9.0;
                line(&[(x, 6.5), (x, 6.0 - 10.0 * (1.0 - t * t) + 1.0)], SLATE_EDGE, 0.8);
            }
            line(&hill, TEAL, 1.8);
        }
        Icon::Tools => {
            // One bit: its shank and its flutes, upright.
            filled(-2.5, -10.0, 2.5, -1.0, SLATE_EDGE);
            fill(&[(-3.0, -1.0), (3.0, -1.0), (3.0, 7.0), (0.0, 10.0), (-3.0, 7.0)], TEAL);
            for y in [1.0, 4.0, 7.0] {
                line(&[(-3.0, y + 1.5), (3.0, y - 1.5)], INK, 1.0);
            }
        }
        Icon::Edit => {
            // Sliders, set to different places.
            for (y, k) in [(-6.0, -3.0), (0.0, 4.0), (6.0, -1.0)] {
                line(&[(-9.0, y), (9.0, y)], SLATE_EDGE, 1.4);
                p.circle_filled(at(k, y), 2.6 * s, tone(TEAL));
            }
        }
        Icon::Clock => {
            // A stopwatch.
            p.circle_filled(at(0.0, 2.0), 8.0 * s, tone(SLATE));
            ring(0.0, 2.0, 8.0, TEAL, 1.6);
            filled(-2.0, -10.0, 2.0, -7.0, TEAL);
            line(&[(0.0, 2.0), (3.5, -2.5)], BLACK, 1.4);
        }
        Icon::Preview => {
            plate();
            fill(&[(-4.0, -6.0), (6.5, 0.0), (-4.0, 6.0)], TEAL);
        }
        Icon::Recalc => {
            // Two arrows chasing each other round.
            for half in [0.0f32, 0.5] {
                let pts: Vec<(f32, f32)> = (0..=12)
                    .map(|i| {
                        let a = std::f32::consts::TAU * (half + 0.05 + 0.36 * i as f32 / 12.0);
                        (8.0 * a.cos(), 8.0 * a.sin())
                    })
                    .collect();
                line(&pts, TEAL, 2.0);
                let a = std::f32::consts::TAU * (half + 0.41);
                let (tx, ty) = (8.0 * a.cos(), 8.0 * a.sin());
                let (dx, dy) = (-a.sin(), a.cos());
                fill(&[(tx + dx * 3.5, ty + dy * 3.5), (tx - dy * 3.0, ty + dx * 3.0), (tx + dy * 3.0, ty - dx * 3.0)], TEAL);
            }
        }
        Icon::Snap => {
            boxed(-9.0, -9.0, 9.0, 9.0, BLUE, 1.4);
            line(&[(-5.0, 5.0), (5.0, -5.0)], BLUE, 1.2);
            line(&[(0.0, -5.0), (5.0, -5.0), (5.0, 0.0)], BLUE, 1.2);
        }
        Icon::SnapObjects => {
            boxed(-9.0, -9.0, 9.0, 9.0, BLUE, 1.4);
            word("A", 12.0, BLUE);
            filled(-8.0, -8.0, -3.0, -3.0, SKY);
        }
        Icon::Grid => {
            for i in 0..5 {
                let o = -9.0 + i as f32 * 4.5;
                line(&[(o, -9.0), (o, 9.0)], BLACK, 1.2);
                line(&[(-9.0, o), (9.0, o)], BLACK, 1.2);
            }
        }
        Icon::Pan => {
            line(&[(-10.0, 0.0), (10.0, 0.0)], BLUE, 1.7);
            line(&[(0.0, -10.0), (0.0, 10.0)], BLUE, 1.7);
            for pts in [
                [(-10.0, 0.0), (-6.0, -3.0), (-6.0, 3.0)],
                [(10.0, 0.0), (6.0, -3.0), (6.0, 3.0)],
                [(0.0, -10.0), (-3.0, -6.0), (3.0, -6.0)],
                [(0.0, 10.0), (-3.0, 6.0), (3.0, 6.0)],
            ] {
                fill(&pts, BLUE);
            }
        }
        Icon::ZoomBox | Icon::ZoomFit | Icon::ZoomSel => {
            boxed(-9.0, -9.0, 9.0, 9.0, BLUE, 1.3);
            p.circle_filled(at(0.0, 0.0), 5.5 * s, tone(Color32::from_rgb(215, 225, 245)));
            ring(0.0, 0.0, 5.5, Color32::from_gray(200), 1.3);
            line(&[(4.0, 4.0), (9.0, 9.0)], Color32::from_gray(200), 2.0);
            if icon == Icon::ZoomSel {
                ring(0.0, 0.0, 2.2, Color32::from_rgb(190, 50, 190), 1.2);
            }
        }
        Icon::ShowVectors => {
            filled(-9.0, -9.0, 9.0, 9.0, Color32::from_rgb(70, 110, 220));
            closed(&[(0.0, -7.0), (7.0, 0.0), (0.0, 7.0), (-7.0, 0.0)], YELLOW, 2.0);
        }
        Icon::ShowPaths => {
            for i in 0..3 {
                let o = i as f32 * 4.0 - 4.0;
                line(&[(-8.0 + o, 5.0), (o, -5.0), (8.0 + o, -5.0)], BLUE, 1.2);
            }
        }
        Icon::ShowModel => {
            p.circle_filled(c, 8.0 * s, tone(Color32::from_rgb(170, 215, 170)));
            p.circle_filled(at(0.0, 1.0), 3.5 * s, tone(Color32::from_rgb(70, 140, 80)));
        }
        // The block and the line it's turned over about, with the way it turns.
        Icon::FlipAroundX | Icon::FlipAroundY => {
            fill(&[(-9.0, -6.0), (9.0, -6.0), (9.0, 6.0), (-9.0, 6.0)], WOOD);
            if icon == Icon::FlipAroundY {
                line(&[(0.0, -10.0), (0.0, 10.0)], RED, 1.4);
                line(&[(-6.0, -8.5), (6.0, -8.5)], BLUE, 1.2);
                line(&[(3.5, -10.5), (6.0, -8.5), (3.5, -6.5)], BLUE, 1.2);
            } else {
                line(&[(-11.0, 0.0), (11.0, 0.0)], RED, 1.4);
                line(&[(10.5, -5.0), (10.5, 5.0)], BLUE, 1.2);
                line(&[(8.5, 2.5), (10.5, 5.0), (12.5, 2.5)], BLUE, 1.2);
            }
        }
        // The side being worked on: a block with its number, the bottom one orange.
        Icon::SideTop | Icon::SideBottom => {
            let (col, t) = if icon == Icon::SideTop { (WOOD, "1") } else { (Color32::from_rgb(0xED, 0xC2, 0x64), "2") };
            fill(&[(-9.0, -7.0), (9.0, -7.0), (9.0, 7.0), (-9.0, 7.0)], col);
            p.text(at(0.0, 0.0), Align2::CENTER_CENTER, t, FontId::proportional(13.0 * s), tone(BLACK));
        }
        // Two windows, the one in use filled: side by side, or one above the other.
        Icon::TileSide => {
            boxed(-10.0, -8.0, 10.0, 8.0, Color32::from_gray(190), 1.0);
            filled(-10.0, -8.0, -1.0, 8.0, Color32::from_rgb(120, 150, 210));
            line(&[(0.0, -8.0), (0.0, 8.0)], Color32::from_gray(190), 1.0);
        }
        Icon::TileStacked => {
            boxed(-10.0, -8.0, 10.0, 8.0, Color32::from_gray(190), 1.0);
            filled(-10.0, -8.0, 10.0, -1.0, Color32::from_rgb(120, 150, 210));
            line(&[(-10.0, 0.0), (10.0, 0.0)], Color32::from_gray(190), 1.0);
        }
        Icon::ViewFit => {
            boxed(-9.0, -9.0, 9.0, 9.0, BLACK, 1.5);
            boxed(-4.0, -4.0, 4.0, 4.0, BLACK, 1.2);
        }
        Icon::ViewIso | Icon::ViewZ | Icon::ViewX | Icon::ViewY => {
            line(&[(-4.0, 6.0), (-4.0, -8.0)], Color32::from_rgb(40, 140, 60), 1.4);
            line(&[(-4.0, 6.0), (8.0, 8.0)], Color32::from_rgb(40, 60, 210), 1.4);
            line(&[(-4.0, 6.0), (-9.0, 9.0)], RED, 1.4);
            let t = match icon {
                Icon::ViewZ => "Z",
                Icon::ViewX => "X",
                Icon::ViewY => "Y",
                _ => "",
            };
            p.text(at(3.0, -4.0), Align2::CENTER_CENTER, t, FontId::proportional(14.0 * s), tone(BLACK));
        }
        Icon::Play => fill(&[(-5.0, -8.0), (7.0, 0.0), (-5.0, 8.0)], BLUE),
        Icon::Pause => {
            filled(-6.0, -8.0, -2.0, 8.0, BLUE);
            filled(2.0, -8.0, 6.0, 8.0, BLUE);
        }
        Icon::Step => {
            fill(&[(-6.0, -8.0), (4.0, 0.0), (-6.0, 8.0)], BLUE);
            filled(4.0, -8.0, 7.0, 8.0, BLUE);
        }
        Icon::End => {
            fill(&[(-9.0, -8.0), (-1.0, 0.0), (-9.0, 8.0)], BLUE);
            fill(&[(-1.0, -8.0), (6.0, 0.0), (-1.0, 8.0)], BLUE);
            filled(6.0, -8.0, 9.0, 8.0, BLUE);
        }
        Icon::Rewind => {
            filled(-8.0, -8.0, 8.0, -5.0, BLUE);
            fill(&[(0.0, -3.0), (8.0, 7.0), (-8.0, 7.0)], BLUE);
        }
        Icon::Pin => {
            filled(-3.0, -8.0, 3.0, 2.0, Color32::from_gray(200));
            line(&[(-6.0, 2.0), (6.0, 2.0)], Color32::from_gray(200), 1.5);
            line(&[(0.0, 2.0), (0.0, 9.0)], Color32::from_gray(200), 1.5);
        }
        Icon::Collapse => {
            line(&[(3.0, -5.0), (-3.0, 0.0), (3.0, 5.0)], Color32::WHITE, 2.0);
        }
        Icon::TextCurve => {
            // Letters riding a wave.
            let wave: Vec<(f32, f32)> = (0..=20).map(|i| {
                let t = i as f32 / 20.0;
                (-10.0 + 20.0 * t, 6.0 + 2.5 * (std::f32::consts::TAU * t).sin())
            }).collect();
            line(&wave, TEAL, 1.4);
            for (k, ch) in ["a", "b", "c"].iter().enumerate() {
                let x = -6.0 + k as f32 * 6.0;
                let y = 6.0 + 2.5 * (std::f32::consts::TAU * ((x + 10.0) / 20.0)).sin() - 5.0;
                p.text(at(x, y), Align2::CENTER_CENTER, *ch, FontId::proportional(11.0 * s), tone(BLACK));
            }
        }
        Icon::Dimension => {
            // A part's width with its figure in a tag above.
            p.rect_filled(rect_at(-9.0, 3.0, 9.0, 9.0), 1.5 * s, wash(TEAL));
            p.rect_stroke(rect_at(-9.0, 3.0, 9.0, 9.0), 1.5 * s, stroke(TEAL, 1.2), StrokeKind::Inside);
            line(&[(-9.0, -1.0), (9.0, -1.0)], BLACK, 1.0);
            line(&[(-9.0, -3.0), (-9.0, 1.0)], BLACK, 1.0);
            line(&[(9.0, -3.0), (9.0, 1.0)], BLACK, 1.0);
            p.rect_filled(rect_at(-6.0, -10.5, 6.0, -3.0), 1.5 * s, tone(PAPER));
            p.text(at(0.0, -6.8), Align2::CENTER_CENTER, "12", FontId::proportional(9.0 * s), tone(BLACK));
        }
        Icon::Trim => {
            ring(-5.0, 7.0, 2.6, BLACK, 1.2);
            ring(5.0, 7.0, 2.6, BLACK, 1.2);
            line(&[(-3.5, 5.0), (6.0, -9.0)], BLACK, 1.3);
            line(&[(3.5, 5.0), (-6.0, -9.0)], BLACK, 1.3);
            line(&[(-10.0, -1.0), (10.0, -1.0)], RED, 1.1);
        }
        Icon::Fillet => {
            let arc: Vec<(f32, f32)> = (0..=10).map(|i| {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / 10.0;
                (-8.0 + 8.0 * (1.0 - a.sin()), -8.0 + 8.0 * (1.0 - a.cos()))
            }).collect();
            line(&arc, RED, 1.8);
            line(&[(-8.0, 0.0), (-8.0, 9.0)], BLUE, 1.6);
            line(&[(0.0, -8.0), (9.0, -8.0)], BLUE, 1.6);
        }
        Icon::Validate => {
            ring(0.0, 0.0, 8.5, BLUE, 1.4);
            line(&[(-5.0, 0.0), (-1.0, 5.0), (6.0, -6.0)], Color32::from_rgb(110, 210, 130), 2.2);
        }
        Icon::CopyAlong => {
            let curve: Vec<(f32, f32)> = (0..=14).map(|i| {
                let t = i as f32 / 14.0;
                (-9.0 + 18.0 * t, 7.0 - 14.0 * t * t)
            }).collect();
            line(&curve, BLUE, 1.2);
            for t in [0.0f32, 0.5, 1.0] {
                ring(-9.0 + 18.0 * t, 7.0 - 14.0 * t * t, 2.4, YELLOW, 1.3);
            }
        }
        Icon::Nest => {
            boxed(-9.0, -9.0, 9.0, 9.0, BLUE, 1.2);
            filled(-7.0, -7.0, -1.0, -1.0, SKY);
            filled(1.0, -7.0, 7.0, -3.0, YELLOW);
            filled(-7.0, 1.0, 2.0, 7.0, YELLOW);
            filled(4.0, -1.0, 7.0, 7.0, SKY);
        }
        Icon::TpPrism => {
            plate();
            soft(&[(-8.0, 6.0), (0.0, -6.0), (8.0, 6.0)], TEAL);
            closed(&[(-8.0, 6.0), (0.0, -6.0), (8.0, 6.0)], TEAL, 1.5);
            line(&[(0.0, -6.0), (0.0, 6.0)], BLACK, 0.8);
        }
        Icon::TpTexture => {
            plate();
            // Rows of little scoops.
            for (row, y) in [(0, -3.5), (1, 3.5)] {
                for k in 0..4 {
                    let x0 = -8.0 + k as f32 * 4.5 + if row == 1 { 2.2 } else { 0.0 };
                    let cup: Vec<(f32, f32)> = (0..=8).map(|i| {
                        let a = std::f32::consts::PI * i as f32 / 8.0;
                        (x0 + 1.8 - 1.8 * a.cos(), y + 1.8 * a.sin())
                    }).collect();
                    line(&cup, TEAL, 1.2);
                }
            }
        }
        Icon::TpMoulding => {
            plate();
            // A moulding's profile: an S, standing on its back.
            let ogee: Vec<(f32, f32)> = (0..=20).map(|i| {
                let t = i as f32 / 20.0;
                (-7.0 + 14.0 * t, -6.0 + 12.0 * (0.5 - 0.5 * (std::f32::consts::PI * t).cos()) + 2.0 * (std::f32::consts::TAU * t).sin())
            }).collect();
            let mut shape = ogee.clone();
            shape.push((7.0, 7.0));
            shape.push((-7.0, 7.0));
            soft(&shape, TEAL);
            line(&ogee, TEAL, 1.6);
            line(&[(-7.0, -6.0), (-7.0, 7.0), (7.0, 7.0)], BLACK, 0.9);
        }
        Icon::Eye | Icon::EyeOff => {
            let on = icon == Icon::Eye;
            let lid: Vec<(f32, f32)> = (0..=16).map(|i| {
                let t = i as f32 / 16.0 * 2.0 - 1.0;
                (t * 9.0, -5.0 * (1.0 - t * t))
            }).collect();
            line(&lid, if on { YELLOW } else { BLACK }, 1.3);
            let under: Vec<(f32, f32)> = lid.iter().map(|q| (q.0, -q.1)).collect();
            line(&under, if on { YELLOW } else { BLACK }, 1.3);
            if on {
                p.circle_filled(c, 2.6 * s, tone(YELLOW));
            } else {
                line(&[(-7.0, 7.0), (7.0, -7.0)], RED, 1.5);
            }
        }
        Icon::Locked | Icon::Unlocked => {
            filled(-6.0, -1.0, 6.0, 8.0, if icon == Icon::Locked { YELLOW } else { Color32::from_gray(150) });
            let shift = if icon == Icon::Locked { 0.0 } else { 7.0 };
            let top: Vec<(f32, f32)> = (0..=10).map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                (shift + 4.0 * a.cos(), -3.0 - 5.0 * a.sin())
            }).collect();
            line(&top, Color32::from_gray(190), 1.4);
            line(&[(-4.0 + shift, -3.0), (-4.0 + shift, -1.0)], Color32::from_gray(190), 1.4);
            line(&[(4.0 + shift, -3.0), (4.0 + shift, -1.0)], Color32::from_gray(190), 1.4);
        }
        Icon::RDimension => {
            line(&[(-9.0, 6.0), (9.0, 6.0)], YELLOW, 1.4);
            line(&[(-9.0, 2.0), (-9.0, 9.0)], BLACK, 1.2);
            line(&[(9.0, 2.0), (9.0, 9.0)], BLACK, 1.2);
            line(&[(-5.0, 4.0), (-9.0, 6.0), (-5.0, 8.0)], YELLOW, 1.1);
            line(&[(5.0, 4.0), (9.0, 6.0), (5.0, 8.0)], YELLOW, 1.1);
            p.text(at(0.0, -4.0), Align2::CENTER_CENTER, "1.5", FontId::proportional(10.0 * s), tone(BLACK));
        }
        Icon::RCoincident => {
            line(&[(-9.0, 7.0), (0.0, 0.0)], BLUE, 1.5);
            line(&[(9.0, 7.0), (0.0, 0.0)], BLUE, 1.5);
            p.circle_filled(at(0.0, 0.0), 3.0 * s, tone(YELLOW));
        }
        Icon::RHorizontal => {
            line(&[(-9.0, 0.0), (9.0, 0.0)], BLUE, 2.0);
            dot(-9.0, 0.0, YELLOW);
            dot(9.0, 0.0, YELLOW);
        }
        Icon::RVertical => {
            line(&[(0.0, -9.0), (0.0, 9.0)], BLUE, 2.0);
            dot(0.0, -9.0, YELLOW);
            dot(0.0, 9.0, YELLOW);
        }
        Icon::RParallel => {
            line(&[(-7.0, 7.0), (3.0, -8.0)], BLUE, 1.8);
            line(&[(-1.0, 8.0), (8.0, -7.0)], YELLOW, 1.8);
        }
        Icon::RPerpendicular => {
            line(&[(-9.0, 8.0), (9.0, 8.0)], BLUE, 1.8);
            line(&[(0.0, 8.0), (0.0, -9.0)], YELLOW, 1.8);
            line(&[(0.0, 3.0), (5.0, 3.0), (5.0, 8.0)], BLACK, 1.0);
        }
        Icon::REqual => {
            line(&[(-8.0, -3.0), (8.0, -3.0)], YELLOW, 2.2);
            line(&[(-8.0, 3.0), (8.0, 3.0)], YELLOW, 2.2);
        }
        Icon::RCollinear => {
            line(&[(-10.0, 7.0), (-2.0, 1.5)], BLUE, 2.0);
            line(&[(2.0, -1.5), (10.0, -7.0)], YELLOW, 2.0);
        }
        Icon::RTangent => {
            ring(0.0, 2.0, 6.0, BLUE, 1.5);
            line(&[(-10.0, -4.0), (10.0, -4.0)], YELLOW, 1.7);
        }
        Icon::RConcentric => {
            ring(0.0, 0.0, 8.5, BLUE, 1.4);
            ring(0.0, 0.0, 4.5, YELLOW, 1.4);
            p.circle_filled(c, 1.3 * s, tone(BLACK));
        }
        Icon::RMidpoint => {
            line(&[(-10.0, 4.0), (10.0, 4.0)], BLUE, 1.8);
            dot(-10.0, 4.0, BLUE);
            dot(10.0, 4.0, BLUE);
            p.circle_filled(at(0.0, 4.0), 3.0 * s, tone(YELLOW));
            line(&[(0.0, -8.0), (0.0, 0.0)], BLACK, 1.0);
        }
        Icon::RPointOn => {
            line(&[(-10.0, 7.0), (10.0, -7.0)], BLUE, 1.8);
            p.circle_filled(at(3.0, -2.0), 3.0 * s, tone(YELLOW));
        }
        Icon::RSymmetric => {
            line(&[(0.0, -10.0), (0.0, 10.0)], BLACK, 1.1);
            dot(-7.0, 0.0, YELLOW);
            dot(7.0, 0.0, YELLOW);
            line(&[(-4.0, -3.0), (-7.0, 0.0), (-4.0, 3.0)], BLUE, 1.1);
            line(&[(4.0, -3.0), (7.0, 0.0), (4.0, 3.0)], BLUE, 1.1);
        }
        Icon::RFixed => {
            filled(-6.0, -1.0, 6.0, 8.0, YELLOW);
            let top: Vec<(f32, f32)> = (0..=10).map(|i| {
                let a = std::f32::consts::PI * i as f32 / 10.0;
                (4.0 * a.cos(), -3.0 - 5.0 * a.sin())
            }).collect();
            line(&top, BLACK, 1.4);
        }
        Icon::RConstruction => {
            for x in [-10.0f32, -4.0, 2.0, 8.0] {
                line(&[(x, 5.0 - x * 0.8), (x + 3.5, 5.0 - (x + 3.5) * 0.8)], YELLOW, 1.8);
            }
        }
        Icon::ReliefPicture => {
            filled(-9.0, -8.0, 9.0, 8.0, Color32::from_rgb(70, 92, 110));
            let hill: Vec<(f32, f32)> = (0..=14).map(|i| {
                let t = i as f32 / 14.0;
                (-9.0 + 18.0 * t, 6.0 - 11.0 * (std::f32::consts::PI * t).sin().powi(2))
            }).collect();
            line(&hill, YELLOW, 1.6);
            boxed(-9.0, -8.0, 9.0, 8.0, BLUE, 1.2);
        }
        Icon::ImportModel => {
            fill(&[(-8.0, -3.0), (0.0, -8.0), (8.0, -3.0), (0.0, 2.0)], WOOD);
            fill(&[(-8.0, -3.0), (0.0, 2.0), (0.0, 9.0), (-8.0, 4.0)], WOOD_DARK);
            fill(&[(8.0, -3.0), (0.0, 2.0), (0.0, 9.0), (8.0, 4.0)], Color32::from_rgb(160, 118, 72));
            line(&[(-8.0, -3.0), (0.0, -8.0), (8.0, -3.0), (8.0, 4.0), (0.0, 9.0), (-8.0, 4.0), (-8.0, -3.0)], BLUE, 1.1);
        }
        // A thin flat slab with a dish sunk in it.
        Icon::ZeroPlane => {
            fill(&[(-9.0, 0.0), (0.0, -5.0), (9.0, 0.0), (0.0, 5.0)], WOOD);
            fill(&[(-9.0, 0.0), (0.0, 5.0), (0.0, 8.0), (-9.0, 3.0)], WOOD_DARK);
            fill(&[(9.0, 0.0), (0.0, 5.0), (0.0, 8.0), (9.0, 3.0)], Color32::from_rgb(160, 118, 72));
            fill(&[(-4.0, 0.0), (0.0, -2.2), (4.0, 0.0), (0.0, 2.2)], WOOD_DARK);
            line(&[(-9.0, 0.0), (0.0, -5.0), (9.0, 0.0), (9.0, 3.0), (0.0, 8.0), (-9.0, 3.0), (-9.0, 0.0)], BLUE, 1.1);
        }
        Icon::TpFlute => {
            plate();
            // Slots that taper in and out, side by side.
            for y in [-5.0, 0.0, 5.0] {
                let lens: Vec<(f32, f32)> = (0..16).map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 16.0;
                    (8.0 * a.cos(), y + 1.6 * a.sin())
                }).collect();
                soft(&lens, TEAL);
                closed(&lens, TEAL, 1.1);
            }
        }
        Icon::TpChamfer => {
            plate();
            // An edge with its corner cut at an angle.
            soft(&[(-8.0, -5.0), (2.0, -5.0), (8.0, 1.0), (8.0, 7.0), (-8.0, 7.0)], SLATE_EDGE);
            line(&[(-8.0, -5.0), (2.0, -5.0)], BLACK, 1.0);
            line(&[(2.0, -5.0), (8.0, 1.0)], TEAL, 2.2);
            line(&[(8.0, 1.0), (8.0, 7.0)], BLACK, 1.0);
        }
        Icon::SnapShapes => {
            line(&[(-8.0, 7.0), (7.0, -8.0)], BLUE, 1.3);
            boxed(-3.0, -3.0, 3.0, 3.0, YELLOW, 1.6);
        }
        Icon::JobSetup => {
            fill(&[(-9.0, 7.0), (-3.0, -6.0), (10.0, -6.0), (4.0, 7.0)], Color32::from_rgb(196, 150, 96));
            line(&[(-9.0, 7.0), (-3.0, -6.0), (10.0, -6.0), (4.0, 7.0), (-9.0, 7.0)], BLUE, 1.2);
            p.circle_filled(at(-9.0, 7.0), 2.6 * s, tone(Color32::from_rgb(230, 40, 30)));
        }
        Icon::Trace => {
            filled(-9.0, -8.0, 9.0, 8.0, Color32::from_rgb(70, 92, 110));
            fill(&[(-9.0, 8.0), (-3.0, -2.0), (2.0, 4.0), (5.0, 0.0), (9.0, 8.0)], Color32::from_rgb(130, 170, 120));
            p.circle_filled(at(4.0, -4.0), 2.0 * s, tone(YELLOW));
            boxed(-9.0, -8.0, 9.0, 8.0, BLUE, 1.2);
        }
        Icon::Distort => {
            // A grid bent into a wave.
            let bend = |x: f32, y: f32| (x, y + 2.5 * (std::f32::consts::PI * (x + 9.0) / 9.0).sin());
            for k in 0..4 {
                let y = -7.5 + k as f32 * 5.0;
                let row: Vec<(f32, f32)> = (0..=12).map(|i| bend(-9.0 + 1.5 * i as f32, y)).collect();
                line(&row, TEAL, if k == 0 || k == 3 { 1.4 } else { 0.8 });
            }
            for k in 0..4 {
                let x = -9.0 + k as f32 * 6.0;
                let col: Vec<(f32, f32)> = (0..=6).map(|i| bend(x, -7.5 + 2.5 * i as f32)).collect();
                line(&col, TEAL, if k == 0 || k == 3 { 1.4 } else { 0.8 });
            }
            let (a, b) = (bend(-9.0, -7.5), bend(9.0, 7.5));
            handle(a.0, a.1);
            handle(b.0, b.1);
        }
        Icon::Similar => {
            for (x, y, on) in [(-6.0, -6.0, true), (6.0, -6.0, true), (-6.0, 6.0, true), (6.0, 6.0, false)] {
                ring(x, y, 3.6, if on { YELLOW } else { Color32::from_gray(140) }, 1.4);
            }
        }
        Icon::Bulk => {
            for (x, y) in [(-6.0, -5.0), (6.0, -5.0), (-6.0, 6.0), (6.0, 6.0)] {
                ring(x, y, 2.4, BLUE, 1.2);
                ring(x, y, 4.2, YELLOW, 1.1);
            }
        }
        Icon::Constraint => {
            line(&[(-8.0, 7.0), (8.0, 7.0)], BLUE, 1.5);
            line(&[(-8.0, 7.0), (-8.0, -8.0)], BLUE, 1.5);
            boxed(-8.0, 3.0, -4.0, 7.0, YELLOW, 1.2);
            ring(4.0, -3.0, 4.0, SKY, 1.3);
            dot(4.0, -3.0, YELLOW);
        }
        Icon::Sample => {
            p.circle_filled(c, 9.0 * s, tone(Color32::from_gray(45)));
            fill(&[(-5.0, 3.0), (3.0, -6.0), (5.0, -4.0), (-3.0, 5.0)], Color32::from_gray(220));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon stays inside its own square, give or take a stroke's width: one that drew
    /// further would leave lines across the panel.
    #[test]
    fn every_icon_stays_in_its_square() {
        let ctx = egui::Context::default();
        let shown: Vec<Icon> = {
            use Icon::*;
            vec![
                New, Open, Save, Import, Export, SelectAll, Cut, Copy, Paste, Undo, Redo, Circle, Ellipse, Rect, Polygon, Star, Lines, Arc, Curve, Spiral, Text, Measure, Move, Scale, Rotate, Mirror, Align, Select, Nodes, Group, Ungroup, UngroupAll, MakePiece, SelectPiece, AdjustPiece, ReleasePiece, Join, Close, Weld, Subtract, Intersect, Delete, Offset, BlockCopy, CircularCopy, Dome, Layer, Up, Down, TpProfile, TpPocket, TpDrill, TpEngrave, TpVCarve, TpRough, TpFinish, Tools, Edit, Duplicate, Trash, Clock, Preview, Recalc, SaveTp, Snap, SnapObjects, Grid, Pan, ZoomBox, ZoomFit, ZoomSel, ShowVectors, ShowPaths, ShowModel, TileSide, TileStacked, FlipAroundX, FlipAroundY, SideTop, SideBottom, ViewFit, ViewIso, ViewZ, ViewX, ViewY, Play, Pause, Step, End, Rewind, Pin, Collapse, Doc, Folder, Sample, TextCurve, Dimension, Trim, Fillet, Validate, CopyAlong, Nest, TpPrism, TpTexture, TpMoulding, Eye, EyeOff, Locked, Unlocked, Similar, Bulk, Constraint, Trace, Distort, JobSetup, ReliefPicture, ImportModel, ZeroPlane, TpFlute, TpChamfer, SnapShapes,
            ]
        };
        for icon in shown {
            let square = Rect::from_min_size(egui::pos2(100.0, 100.0), vec2(24.0, 24.0));
            let out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let p = ui.painter().with_clip_rect(Rect::EVERYTHING);
                paint(&p, square, icon, true);
            });
            // The points actually drawn, after the edges are smoothed.
            let reach = square.expand(4.0);
            let shapes: Vec<_> = out.shapes.into_iter().filter(|c| {
                let r = c.shape.visual_bounding_rect();
                !r.is_positive() || r.min.x > 90.0
            }).collect();
            for prim in ctx.tessellate(shapes, 1.0) {
                if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                    for v in &mesh.vertices {
                        assert!(reach.contains(v.pos), "{icon:?} draws at {:?}, outside {square:?}", v.pos);
                    }
                }
            }
        }
    }
}
