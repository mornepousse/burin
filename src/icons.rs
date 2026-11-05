//! Small tool icons drawn with lines, so no picture files are needed.

use egui::{vec2, Align2, Color32, FontId, Painter, Rect, Shape, Stroke, StrokeKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    New, Open, Save, Import, Export,
    SelectAll, Cut, Copy, Paste, Undo, Redo,
    Circle, Ellipse, Rect, Polygon, Star,
    Lines, Arc, Curve, Spiral, Text, Measure,
    Move, Scale, Rotate, Mirror, Align,
    Select, Nodes, Group, Ungroup, Join, Close, Weld, Subtract, Intersect, Delete,
    Offset, BlockCopy, CircularCopy,
    Dome, Layer, Up, Down,
    TpProfile, TpPocket, TpDrill, TpEngrave, TpVCarve, TpRough, TpFinish,
    Tools, Edit, Duplicate, Trash, Clock, Preview, Recalc, SaveTp,
    Snap, SnapObjects, Grid, Pan, ZoomBox, ZoomFit, ZoomSel, ShowVectors, ShowPaths, ShowModel, PanelLeft, PanelRight,
    ViewFit, ViewIso, ViewZ, ViewX, ViewY,
    Play, Pause, Step, End, Rewind, Pin, Collapse,
    Doc, Folder, Sample,
    TextCurve, Dimension, Trim, Fillet, Validate, CopyAlong, Nest,
    TpPrism, TpTexture, TpMoulding, Eye, EyeOff, Locked, Unlocked,
    Similar, Bulk, Constraint, Trace, Distort, JobSetup, ReliefPicture, ImportModel, TpFlute, TpChamfer, SnapShapes,
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
const FOLDER: Color32 = Color32::from_rgb(244, 208, 96);

fn faded(c: Color32) -> Color32 {
    // Blended towards the panel grey, still tinted so it reads as a dimmed icon.
    let mix = |v: u8, panel: f32| (v as f32 * 0.3 + panel * 0.7) as u8;
    Color32::from_rgb(mix(c.r(), 54.0), mix(c.g(), 58.0), mix(c.b(), 66.0))
}

pub fn paint(p: &Painter, rect: Rect, icon: Icon, enabled: bool) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 24.0;
    let tone = |col: Color32| if enabled { col } else { faded(col) };
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
            fill(&[(-6.0, -9.0), (3.0, -9.0), (7.0, -5.0), (7.0, 9.0), (-6.0, 9.0)], PAPER);
            closed(&[(-6.0, -9.0), (3.0, -9.0), (7.0, -5.0), (7.0, 9.0), (-6.0, 9.0)], SKY, 1.1);
            line(&[(3.0, -9.0), (3.0, -5.0), (7.0, -5.0)], SKY, 1.0);
        }
        Icon::Open | Icon::Folder => {
            fill(&[(-9.0, -6.0), (-3.0, -6.0), (-1.0, -3.0), (8.0, -3.0), (8.0, 7.0), (-9.0, 7.0)], FOLDER);
            fill(&[(-7.0, 0.0), (10.0, 0.0), (8.0, 7.0), (-9.0, 7.0)], Color32::from_rgb(252, 228, 140));
            closed(&[(-9.0, -6.0), (-3.0, -6.0), (-1.0, -3.0), (8.0, -3.0), (8.0, 7.0), (-9.0, 7.0)], Color32::from_rgb(170, 130, 40), 0.9);
        }
        Icon::Save | Icon::SaveTp => {
            filled(-8.0, -8.0, 8.0, 8.0, SKY);
            filled(-5.0, -8.0, 5.0, -2.0, PAPER);
            filled(-4.0, 2.0, 4.0, 8.0, Color32::from_rgb(235, 90, 80));
            if icon == Icon::SaveTp {
                line(&[(-10.0, 10.0), (10.0, 10.0)], WOOD_DARK, 2.0);
            }
        }
        Icon::Import => {
            fill(&[(-9.0, -6.0), (-3.0, -6.0), (-1.0, -3.0), (8.0, -3.0), (8.0, 7.0), (-9.0, 7.0)], FOLDER);
            line(&[(0.0, -9.0), (0.0, 3.0)], BLUE, 1.8);
            line(&[(-3.0, 0.0), (0.0, 3.5), (3.0, 0.0)], BLUE, 1.8);
        }
        Icon::Export => {
            fill(&[(-9.0, -6.0), (-3.0, -6.0), (-1.0, -3.0), (8.0, -3.0), (8.0, 7.0), (-9.0, 7.0)], FOLDER);
            line(&[(0.0, 5.0), (0.0, -8.0)], BLUE, 1.8);
            line(&[(-3.0, -5.0), (0.0, -8.5), (3.0, -5.0)], BLUE, 1.8);
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
            fill(&[(-8.0, -8.0), (2.0, -8.0), (2.0, 4.0), (-8.0, 4.0)], PAPER);
            closed(&[(-8.0, -8.0), (2.0, -8.0), (2.0, 4.0), (-8.0, 4.0)], BLUE, 1.0);
            fill(&[(-2.0, -3.0), (8.0, -3.0), (8.0, 9.0), (-2.0, 9.0)], PAPER);
            closed(&[(-2.0, -3.0), (8.0, -3.0), (8.0, 9.0), (-2.0, 9.0)], BLUE, 1.0);
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
            ring(0.0, 0.0, 8.0, BLUE, 1.5);
            dot(0.0, 0.0, YELLOW);
            dot(8.0, 0.0, YELLOW);
        }
        Icon::Ellipse => {
            let pts: Vec<(f32, f32)> = (0..32)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 32.0;
                    (9.0 * a.cos(), 5.5 * a.sin())
                })
                .collect();
            closed(&pts, BLUE, 1.5);
            dot(0.0, 0.0, YELLOW);
            dot(9.0, 0.0, YELLOW);
        }
        Icon::Rect => boxed(-9.0, -7.0, 9.0, 7.0, BLUE, 1.6),
        Icon::Polygon => {
            let pts: Vec<(f32, f32)> = (0..5)
                .map(|i| {
                    let a = std::f32::consts::TAU * (i as f32 / 5.0 - 0.25);
                    (9.0 * a.cos(), 9.0 * a.sin())
                })
                .collect();
            closed(&pts, BLUE, 1.5);
            dot(0.0, 0.0, YELLOW);
        }
        Icon::Star => {
            let pts: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let a = std::f32::consts::TAU * (i as f32 / 10.0 - 0.25);
                    let r = if i % 2 == 0 { 10.0 } else { 4.2 };
                    (r * a.cos(), r * a.sin())
                })
                .collect();
            closed(&pts, BLUE, 1.5);
            dot(0.0, 0.0, YELLOW);
        }
        Icon::Lines => {
            line(&[(-8.0, -7.0), (7.0, -7.0), (-6.0, 1.0), (8.0, 7.0)], BLUE, 1.6);
            dot(-8.0, -7.0, YELLOW);
            dot(8.0, 7.0, YELLOW);
        }
        Icon::Arc => {
            let pts: Vec<(f32, f32)> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 24.0);
                    (-9.0 + 18.0 * (a.cos() + 1.0).abs(), 9.0 + 18.0 * a.sin())
                })
                .collect();
            line(&pts, BLUE, 1.6);
            dot(-9.0, 9.0, YELLOW);
            dot(9.0, -9.0, YELLOW);
        }
        Icon::Curve => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    (8.0 - 16.0 * t, 8.0 * (std::f32::consts::TAU * t).cos() * if t < 0.5 { -1.0 } else { 1.0 } * (1.0 - t * 0.2))
                })
                .collect();
            line(&pts, BLUE, 2.0);
        }
        Icon::Spiral => {
            let pts: Vec<(f32, f32)> = (0..=60)
                .map(|i| {
                    let t = i as f32 / 60.0;
                    let a = std::f32::consts::TAU * 3.0 * t;
                    (9.5 * t * a.cos(), 9.5 * t * a.sin())
                })
                .collect();
            line(&pts, BLUE, 1.5);
        }
        Icon::Text => word("T", 24.0, BLUE),
        Icon::Measure => {
            line(&[(-9.0, 6.0), (9.0, 6.0)], BLACK, 1.5);
            line(&[(-9.0, 3.0), (-9.0, 9.0)], BLACK, 1.5);
            line(&[(9.0, 3.0), (9.0, 9.0)], BLACK, 1.5);
            word("?", 14.0, BLACK);
        }
        Icon::Move => {
            boxed(-9.0, 0.0, 0.0, 9.0, SKY, 1.2);
            boxed(-3.0, -8.0, 7.0, 2.0, BLUE, 1.5);
            line(&[(-5.0, 5.0), (3.0, -4.0)], RED, 1.2);
        }
        Icon::Scale => {
            boxed(-9.0, -3.0, 2.0, 8.0, SKY, 1.2);
            boxed(-9.0, -9.0, 9.0, 8.0, BLUE, 1.2);
            line(&[(-1.0, 1.0), (7.0, -7.0)], RED, 1.2);
        }
        Icon::Rotate => {
            boxed(-5.0, -5.0, 5.0, 5.0, BLUE, 1.3);
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::TAU * (0.05 + 0.7 * i as f32 / 20.0);
                    (9.5 * a.cos(), 9.5 * a.sin())
                })
                .collect();
            line(&pts, RED, 1.2);
        }
        Icon::Mirror => {
            line(&[(0.0, -10.0), (0.0, 10.0)], RED, 1.1);
            closed(&[(-3.0, -7.0), (-3.0, 7.0), (-9.0, 5.0), (-9.0, -5.0)], BLUE, 1.3);
            closed(&[(3.0, -7.0), (3.0, 7.0), (9.0, 5.0), (9.0, -5.0)], SKY, 1.3);
        }
        Icon::Align => {
            line(&[(0.0, -10.0), (0.0, 10.0)], RED, 1.1);
            boxed(-7.0, -8.0, 7.0, -3.0, BLUE, 1.3);
            boxed(-4.0, 2.0, 4.0, 7.0, BLUE, 1.3);
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
            boxed(-9.0, -9.0, 9.0, 9.0, Color32::from_gray(190), 1.0);
            boxed(-6.0, -6.0, 1.0, 1.0, BLUE, 1.3);
            ring(3.0, 3.0, 3.5, BLUE, 1.3);
        }
        Icon::Ungroup => {
            boxed(-9.0, -9.0, -1.0, -1.0, BLUE, 1.3);
            ring(5.0, 5.0, 4.0, BLUE, 1.3);
            line(&[(-9.0, 9.0), (9.0, -9.0)], RED, 1.0);
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
            line(&[(-8.0, -8.0), (8.0, 8.0)], RED, 2.4);
            line(&[(8.0, -8.0), (-8.0, 8.0)], RED, 2.4);
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
            tile();
            closed(&[(-6.0, -3.0), (4.0, -5.5), (7.0, 3.0), (-3.0, 5.5)], INK, 1.8);
        }
        Icon::TpPocket => {
            tile();
            fill(&[(-6.0, -2.5), (4.0, -5.0), (6.5, 2.5), (-3.5, 5.0)], WOOD_DARK);
        }
        Icon::TpDrill => {
            tile();
            for (x, y) in [(-5.0, -2.0), (0.0, -3.5), (5.0, -4.5), (-3.0, 3.0), (2.0, 2.0), (6.0, 1.0)] {
                p.circle_filled(at(x, y), 1.4 * s, tone(Color32::from_rgb(70, 45, 25)));
            }
        }
        Icon::TpEngrave => {
            word("T", 26.0, WOOD);
        }
        Icon::TpVCarve => {
            tile();
            line(&[(-4.0, -4.0), (0.0, 5.0), (4.0, -5.0)], INK, 1.8);
        }
        Icon::TpRough => {
            tile();
            for i in 0..4 {
                let o = i as f32 * 3.2 - 5.0;
                line(&[(-6.0 + o * 0.25, o - 1.0), (6.0 + o * 0.25, o - 4.0)], WOOD_DARK, 1.4);
            }
        }
        Icon::TpFinish => {
            p.circle_filled(at(0.0, 0.0), 8.0 * s, tone(Color32::from_rgb(120, 110, 100)));
            p.circle_filled(at(-2.5, -3.0), 3.5 * s, tone(Color32::from_rgb(226, 220, 210)));
        }
        Icon::Tools => {
            for x in [-7.0f32, 0.0, 7.0] {
                filled(x - 1.5, -9.0, x + 1.5, 1.0, Color32::from_gray(150));
                filled(x - 1.5, 1.0, x + 1.5, 9.0, Color32::from_rgb(220, 180, 80));
            }
        }
        Icon::Edit => {
            fill(&[(-8.0, -9.0), (5.0, -9.0), (5.0, 9.0), (-8.0, 9.0)], PAPER);
            closed(&[(-8.0, -9.0), (5.0, -9.0), (5.0, 9.0), (-8.0, 9.0)], BLUE, 1.0);
            line(&[(-2.0, 6.0), (9.0, -6.0)], RED, 2.2);
        }
        Icon::Clock => {
            p.circle_filled(c, 9.0 * s, tone(PAPER));
            ring(0.0, 0.0, 9.0, RED, 1.6);
            line(&[(0.0, -6.0), (0.0, 0.0), (4.0, 2.0)], BLACK, 1.4);
        }
        Icon::Preview => {
            tile();
            p.circle_filled(at(0.0, -3.0), 4.0 * s, tone(Color32::from_rgb(120, 76, 36)));
            filled(-1.0, -11.0, 1.0, -4.0, Color32::from_rgb(214, 180, 70));
        }
        Icon::Recalc => {
            let pts: Vec<(f32, f32)> = (0..=22)
                .map(|i| {
                    let a = std::f32::consts::TAU * (0.1 + 0.8 * i as f32 / 22.0);
                    (8.0 * a.cos(), 8.0 * a.sin())
                })
                .collect();
            line(&pts, BLUE, 2.0);
            fill(&[(8.0, -7.0), (10.0, 1.0), (3.0, -2.0)], BLUE);
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
        Icon::PanelLeft | Icon::PanelRight => {
            boxed(-10.0, -8.0, 10.0, 8.0, Color32::from_gray(190), 1.0);
            let (a, b) = if icon == Icon::PanelLeft { (-10.0, -2.0) } else { (2.0, 10.0) };
            filled(a, -8.0, b, 8.0, Color32::from_rgb(120, 150, 210));
            line(&[(-2.0, -5.0), (-2.0, 5.0)], Color32::from_gray(190), 1.0);
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
            let curve: Vec<(f32, f32)> = (0..=16).map(|i| {
                let t = i as f32 / 16.0 * 2.0 - 1.0;
                (t * 10.0, 7.0 - 6.0 * (1.0 - t * t))
            }).collect();
            line(&curve, RED, 1.2);
            word("abc", 13.0, BLUE);
        }
        Icon::Dimension => {
            line(&[(-9.0, 5.0), (9.0, 5.0)], BLACK, 1.3);
            line(&[(-9.0, 2.0), (-9.0, 8.0)], BLACK, 1.3);
            line(&[(9.0, 2.0), (9.0, 8.0)], BLACK, 1.3);
            line(&[(-6.0, 3.0), (-9.0, 5.0), (-6.0, 7.0)], BLACK, 1.1);
            line(&[(6.0, 3.0), (9.0, 5.0), (6.0, 7.0)], BLACK, 1.1);
            p.text(at(0.0, -4.0), Align2::CENTER_CENTER, "12", FontId::proportional(11.0 * s), tone(BLUE));
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
            tile();
            fill(&[(-4.0, 3.0), (-1.0, -5.0), (5.0, -1.0)], WOOD_DARK);
            closed(&[(-4.0, 3.0), (-1.0, -5.0), (5.0, -1.0)], INK, 1.1);
        }
        Icon::TpTexture => {
            tile();
            for (x, y) in [(-6.0, -2.0), (-1.0, -4.0), (4.0, -5.0), (-4.0, 2.0), (1.0, 1.0), (6.0, 0.0), (-1.0, 5.0), (4.0, 4.0)] {
                line(&[(x, y), (x + 3.0, y - 0.8)], INK, 1.3);
            }
        }
        Icon::TpMoulding => {
            fill(&[(-9.0, 7.0), (-3.0, -7.0), (4.0, -7.0), (-2.0, 7.0)], WOOD);
            fill(&[(-2.0, 7.0), (4.0, -7.0), (9.0, -3.0), (4.0, 9.0)], WOOD_DARK);
            line(&[(-9.0, 7.0), (-3.0, -7.0)], INK, 1.0);
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
        Icon::TpFlute => {
            tile();
            for x in [-5.0, 0.0, 5.0] {
                fill(&[(x, -7.0), (x + 1.7, 0.0), (x, 7.0), (x - 1.7, 0.0)], INK);
            }
        }
        Icon::TpChamfer => {
            fill(&[(-9.0, 8.0), (-9.0, -3.0), (2.0, -3.0), (9.0, 4.0), (9.0, 8.0)], WOOD);
            line(&[(2.0, -3.0), (9.0, 4.0)], YELLOW, 2.0);
            line(&[(-9.0, -3.0), (2.0, -3.0)], INK, 1.0);
            line(&[(9.0, 4.0), (9.0, 8.0)], INK, 1.0);
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
            let top: Vec<(f32, f32)> = (0..=12).map(|i| {
                let t = i as f32 / 12.0;
                (-9.0 + 18.0 * t, -8.0 + 4.0 * (std::f32::consts::PI * t).sin())
            }).collect();
            line(&top, BLUE, 1.5);
            let under: Vec<(f32, f32)> = top.iter().map(|q| (q.0, -q.1)).collect();
            line(&under, BLUE, 1.5);
            line(&[(-9.0, -8.0), (-9.0, 8.0)], BLUE, 1.5);
            line(&[(9.0, -8.0), (9.0, 8.0)], BLUE, 1.5);
            for (x, y) in [(-9.0, -8.0), (9.0, -8.0), (-9.0, 8.0), (9.0, 8.0)] {
                dot(x, y, YELLOW);
            }
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
