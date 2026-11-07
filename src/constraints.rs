//! Drawing constraints: rules between shapes that stay true while the drawing changes.
//!
//! Each rule nudges the points it involves towards being satisfied, and the rules are repeated
//! until all of them hold. Points being dragged, and points fixed in place, don't move.

use crate::app::*;
use crate::bulk::as_circle;
use crate::canvas::Drag;
use crate::geom::{self, Path, P};
use crate::theme;
use egui::{pos2, vec2, Color32, FontId, Rect, Stroke};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Elem {
    Node { v: u64, path: usize, node: usize },
    Centre { v: u64 },
    Seg { v: u64, path: usize, seg: usize },
    Round { v: u64 },
}

impl Elem {
    pub fn vector_id(self) -> u64 {
        self.vector()
    }

    fn vector(self) -> u64 {
        match self {
            Elem::Node { v, .. } | Elem::Centre { v } | Elem::Seg { v, .. } | Elem::Round { v } => v,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    Coincident,
    Concentric,
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    Tangent,
    Midpoint,
    Fixed,
    Distance,
    Diameter,
    Angle,
    Collinear,
    PointOnLine,
    PointOnCircle,
    Symmetric,
    DistanceX,
    DistanceY,
    LineDistance,
    Radius,
    /// Holds a fillet's two ends the same distance apart, so the rounded corner keeps its size.
    Keep,
}

/// The rules offered as relations; dimensions have their own tool.
pub const RELATIONS: [Kind; 14] = [
    Kind::Coincident,
    Kind::Horizontal,
    Kind::Vertical,
    Kind::Parallel,
    Kind::Perpendicular,
    Kind::Equal,
    Kind::Collinear,
    Kind::Tangent,
    Kind::Concentric,
    Kind::Midpoint,
    Kind::PointOnLine,
    Kind::PointOnCircle,
    Kind::Symmetric,
    Kind::Fixed,
];

/// Which way a dimension between two points is measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimMode {
    Smart,
    Horizontal,
    Vertical,
}

/// What a click may land on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    Point,
    Line,
    Circle,
    LineOrCircle,
    PointOrLine,
    Any,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Coincident => crate::i18n::tr("Coincident"),
            Kind::Concentric => crate::i18n::tr("Concentric"),
            Kind::Horizontal => crate::i18n::tr("Horizontal"),
            Kind::Vertical => crate::i18n::tr("Vertical"),
            Kind::Parallel => crate::i18n::tr("Parallel"),
            Kind::Perpendicular => crate::i18n::tr("Perpendicular"),
            Kind::Equal => crate::i18n::tr("Equal"),
            Kind::Tangent => crate::i18n::tr("Tangent"),
            Kind::Midpoint => crate::i18n::tr("Midpoint"),
            Kind::Fixed => crate::i18n::tr("Fixed"),
            Kind::Distance => crate::i18n::tr("Distance"),
            Kind::Diameter => crate::i18n::tr("Diameter"),
            Kind::Angle => crate::i18n::tr("Angle"),
            Kind::Collinear => crate::i18n::tr("Collinear"),
            Kind::PointOnLine => crate::i18n::tr("Point on Line"),
            Kind::PointOnCircle => crate::i18n::tr("Point on Circle"),
            Kind::Symmetric => crate::i18n::tr("Symmetric"),
            Kind::DistanceX => crate::i18n::tr("Horizontal Distance"),
            Kind::DistanceY => crate::i18n::tr("Vertical Distance"),
            Kind::LineDistance => crate::i18n::tr("Distance to Line"),
            Kind::Radius => crate::i18n::tr("Radius"),
            Kind::Keep => crate::i18n::tr("Fillet Size"),
        }
    }

    pub fn prompt(self) -> &'static str {
        match self {
            Kind::Coincident => crate::i18n::tr("Click two points to join them."),
            Kind::Concentric => crate::i18n::tr("Click two circles to share one centre."),
            Kind::Horizontal => crate::i18n::tr("Click a line, or two points, to level them."),
            Kind::Vertical => crate::i18n::tr("Click a line, or two points, to line them up vertically."),
            Kind::Parallel => crate::i18n::tr("Click two lines."),
            Kind::Perpendicular => crate::i18n::tr("Click two lines to set them square."),
            Kind::Equal => crate::i18n::tr("Click two lines or two circles to make them the same size."),
            Kind::Tangent => crate::i18n::tr("Click a line and a circle, or two circles, to make them touch."),
            Kind::Midpoint => crate::i18n::tr("Click a point, then the line whose middle it should sit on."),
            Kind::Fixed => crate::i18n::tr("Click a point or a circle to lock it where it is."),
            Kind::Distance => crate::i18n::tr("Click two points, or one line, to set a distance."),
            Kind::Diameter => crate::i18n::tr("Click a circle to set its diameter."),
            Kind::Angle => crate::i18n::tr("Click two lines to set the angle between them."),
            Kind::Collinear => crate::i18n::tr("Click two lines to put them on one straight line."),
            Kind::PointOnLine => crate::i18n::tr("Click a point, then a line it should stay on."),
            Kind::PointOnCircle => crate::i18n::tr("Click a point, then a circle it should stay on."),
            Kind::Symmetric => crate::i18n::tr("Click two points, then the line they mirror across."),
            Kind::DistanceX => crate::i18n::tr("Click two points to set how far apart they are across."),
            Kind::DistanceY => crate::i18n::tr("Click two points to set how far apart they are up and down."),
            Kind::LineDistance => crate::i18n::tr("Click a point, then a line, to set the gap between them."),
            Kind::Radius => crate::i18n::tr("Click a circle to set its radius."),
            Kind::Keep => "",
        }
    }

    /// What the next click should be, with `done` elements already picked.
    pub fn want(self, done: &[Elem]) -> Option<Want> {
        match (self, done.len()) {
            (Kind::Coincident, 0 | 1) => Some(Want::Point),
            (Kind::Concentric, 0 | 1) | (Kind::Diameter | Kind::Radius, 0) => Some(Want::Circle),
            (Kind::Horizontal | Kind::Vertical | Kind::DistanceX | Kind::DistanceY, 0) => Some(Want::PointOrLine),
            (Kind::Horizontal | Kind::Vertical | Kind::DistanceX | Kind::DistanceY, 1) if !matches!(done[0], Elem::Seg { .. }) => Some(Want::Point),
            (Kind::Parallel | Kind::Perpendicular | Kind::Angle | Kind::Collinear, 0 | 1) => Some(Want::Line),
            (Kind::PointOnLine | Kind::LineDistance, 0) | (Kind::PointOnCircle, 0) | (Kind::Symmetric, 0 | 1) => Some(Want::Point),
            (Kind::PointOnLine | Kind::LineDistance, 1) | (Kind::Symmetric, 2) => Some(Want::Line),
            (Kind::PointOnCircle, 1) => Some(Want::Circle),
            (Kind::Equal, 0) => Some(Want::LineOrCircle),
            (Kind::Equal, 1) => Some(if matches!(done[0], Elem::Round { .. }) { Want::Circle } else { Want::Line }),
            (Kind::Tangent, 0) => Some(Want::LineOrCircle),
            (Kind::Tangent, 1) => Some(if matches!(done[0], Elem::Round { .. }) { Want::LineOrCircle } else { Want::Circle }),
            (Kind::Midpoint, 0) => Some(Want::Point),
            (Kind::Midpoint, 1) => Some(Want::Line),
            (Kind::Fixed, 0) => Some(Want::Point),
            (Kind::Distance, 0) => Some(Want::PointOrLine),
            (Kind::Distance, 1) if !matches!(done[0], Elem::Seg { .. }) => Some(Want::Point),
            _ => None,
        }
    }

    pub fn has_value(self) -> bool {
        matches!(self, Kind::Distance | Kind::DistanceX | Kind::DistanceY | Kind::LineDistance | Kind::Diameter | Kind::Radius | Kind::Angle)
    }

    /// The rule's elements in its own order, when `picks` are the right kinds of thing for it.
    pub fn arrange(self, picks: &[Elem]) -> Option<Vec<Elem>> {
        let is_point = |e: &&Elem| matches!(e, Elem::Node { .. } | Elem::Centre { .. });
        let points: Vec<Elem> = picks.iter().filter(is_point).copied().collect();
        let lines: Vec<Elem> = picks.iter().filter(|e| matches!(e, Elem::Seg { .. })).copied().collect();
        let rounds: Vec<Elem> = picks.iter().filter(|e| matches!(e, Elem::Round { .. })).copied().collect();
        let as_centre = |e: Elem| Elem::Centre { v: e.vector() };
        match (self, points.len(), lines.len(), rounds.len()) {
            (Kind::Coincident, 2, 0, 0) => Some(points),
            (Kind::Coincident, 1, 0, 1) => Some(vec![points[0], as_centre(rounds[0])]),
            (Kind::Coincident, 0, 0, 2) => Some(vec![as_centre(rounds[0]), as_centre(rounds[1])]),
            (Kind::Concentric, 0, 0, 2) => Some(rounds),
            (Kind::Horizontal | Kind::Vertical, 0, 1, 0) => Some(lines),
            (Kind::Horizontal | Kind::Vertical, 2, 0, 0) => Some(points),
            (Kind::Horizontal | Kind::Vertical, 0, 0, 2) => Some(vec![as_centre(rounds[0]), as_centre(rounds[1])]),
            (Kind::Parallel | Kind::Perpendicular | Kind::Collinear, 0, 2, 0) => Some(lines),
            (Kind::Equal, 0, 2, 0) => Some(lines),
            (Kind::Equal, 0, 0, 2) => Some(rounds),
            (Kind::Tangent, 0, 1, 1) => Some(vec![lines[0], rounds[0]]),
            (Kind::Tangent, 0, 0, 2) => Some(rounds),
            (Kind::Midpoint | Kind::PointOnLine, 1, 1, 0) => Some(vec![points[0], lines[0]]),
            (Kind::Midpoint | Kind::PointOnLine, 0, 1, 1) => Some(vec![as_centre(rounds[0]), lines[0]]),
            (Kind::PointOnCircle, 1, 0, 1) => Some(vec![points[0], rounds[0]]),
            (Kind::Symmetric, 2, 1, 0) => Some(vec![points[0], points[1], lines[0]]),
            (Kind::Symmetric, 0, 1, 2) => Some(vec![as_centre(rounds[0]), as_centre(rounds[1]), lines[0]]),
            (Kind::Fixed, 1, 0, 0) => Some(points),
            (Kind::Fixed, 0, 0, 1) => Some(vec![as_centre(rounds[0])]),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constraint {
    pub kind: Kind,
    pub items: Vec<Elem>,
    pub value: f64,
    /// Where a fixed point stays.
    pub at: P,
    /// How far a dimension's label was dragged from its usual place.
    #[serde(default)]
    pub place: P,
}

#[derive(Clone, Copy, PartialEq)]
enum Body {
    Round,
    /// Open arcs of one circle: a centre, a radius and the two ends of each arc.
    Arc,
    Nodes,
    Rigid,
}

fn relation_icon(kind: Kind) -> crate::icons::Icon {
    use crate::icons::Icon;
    match kind {
        Kind::Coincident => Icon::RCoincident,
        Kind::Horizontal => Icon::RHorizontal,
        Kind::Vertical => Icon::RVertical,
        Kind::Parallel => Icon::RParallel,
        Kind::Perpendicular => Icon::RPerpendicular,
        Kind::Equal => Icon::REqual,
        Kind::Collinear => Icon::RCollinear,
        Kind::Tangent => Icon::RTangent,
        Kind::Concentric => Icon::RConcentric,
        Kind::Midpoint => Icon::RMidpoint,
        Kind::PointOnLine | Kind::PointOnCircle => Icon::RPointOn,
        Kind::Symmetric => Icon::RSymmetric,
        Kind::Fixed => Icon::RFixed,
        _ => Icon::RDimension,
    }
}

/// The centre and radius when a shape is made only of open arcs of one circle.
pub fn arc_fit(v: &Vector) -> Option<(P, f64)> {
    let first = v.paths.first()?;
    if first.closed || first.pts.len() < 6 {
        return None;
    }
    let (a, b, c) = (first.pts[0], first.pts[first.pts.len() / 2], *first.pts.last().unwrap());
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-9 {
        return None;
    }
    let (sa, sb, sc) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let centre = [(sa * (b[1] - c[1]) + sb * (c[1] - a[1]) + sc * (a[1] - b[1])) / d, (sa * (c[0] - b[0]) + sb * (a[0] - c[0]) + sc * (b[0] - a[0])) / d];
    let r = (a[0] - centre[0]).hypot(a[1] - centre[1]);
    let round = v.paths.iter().all(|p| !p.closed && p.pts.len() >= 2 && p.pts.iter().all(|q| ((q[0] - centre[0]).hypot(q[1] - centre[1]) - r).abs() <= r * 0.004));
    round.then_some((centre, r))
}

/// The centre and radius of a shape that is a circle, or arcs of one.
pub fn round_shape(v: &Vector) -> Option<(P, f64)> {
    if let [only] = v.paths.as_slice() {
        if let Some(found) = as_circle(only) {
            return Some(found);
        }
    }
    arc_fit(v)
}

/// Whether a rule can hold on to this point of a shape.
fn node_usable(v: &Vector, path: usize, node: usize) -> bool {
    let Some(p) = v.paths.get(path) else { return false };
    match body_of(v) {
        Body::Nodes => node < p.pts.len(),
        Body::Arc => node == 0 || node + 1 == p.pts.len(),
        _ => false,
    }
}

fn body_of(v: &Vector) -> Body {
    if let [only] = v.paths.as_slice() {
        if as_circle(only).is_some() {
            return Body::Round;
        }
    }
    if arc_fit(v).is_some() {
        return Body::Arc;
    }
    let total: usize = v.paths.iter().map(|p| p.pts.len()).sum();
    let plain = match v.prim {
        Prim::Free | Prim::Polygon { .. } | Prim::Star { .. } => true,
        Prim::Rect { corner, .. } => corner <= 0.0,
        _ => false,
    };
    if plain && total <= 160 {
        Body::Nodes
    } else {
        Body::Rigid
    }
}

type Key = (u64, usize, usize);
const CENTRE: usize = usize::MAX;

/// The drawing, reduced to the points and radii the rules can move.
#[derive(Clone)]
struct Vars {
    pts: Vec<P>,
    free: Vec<f64>,
    index: HashMap<Key, usize>,
    radius: HashMap<u64, (f64, f64)>,
    /// The ends of arcs, which must stay on their circle.
    arc_ends: Vec<(u64, usize)>,
}

impl Vars {
    fn point(&self, e: Elem) -> Option<usize> {
        match e {
            Elem::Node { v, path, node } => self.index.get(&(v, path, node)).copied(),
            Elem::Centre { v } | Elem::Round { v } => self.index.get(&(v, CENTRE, 0)).copied(),
            Elem::Seg { .. } => None,
        }
    }

    fn seg(&self, e: Elem, doc: &Doc) -> Option<(usize, usize)> {
        let Elem::Seg { v, path, seg } = e else { return None };
        let p = doc.vectors.iter().find(|x| x.id == v)?.paths.get(path)?;
        let n = p.pts.len();
        if seg >= p.segments().count() {
            return None;
        }
        Some((*self.index.get(&(v, path, seg))?, *self.index.get(&(v, path, (seg + 1) % n))?))
    }

    fn shift(&mut self, i: usize, d: P) {
        self.pts[i][0] += d[0] * self.free[i];
        self.pts[i][1] += d[1] * self.free[i];
    }
}

fn wrap_half_turn(a: f64) -> f64 {
    let pi = std::f64::consts::PI;
    let mut x = a.rem_euclid(pi);
    if x > pi / 2.0 {
        x -= pi;
    }
    x
}

fn rotate_seg(x: &mut Vars, seg: (usize, usize), angle: f64) {
    let (a, b) = seg;
    let (fa, fb) = (x.free[a], x.free[b]);
    if fa + fb == 0.0 {
        return;
    }
    // Turn about the end that can't move, otherwise about the middle.
    let pivot = if fa == 0.0 {
        x.pts[a]
    } else if fb == 0.0 {
        x.pts[b]
    } else {
        [(x.pts[a][0] + x.pts[b][0]) / 2.0, (x.pts[a][1] + x.pts[b][1]) / 2.0]
    };
    let (s, k) = angle.sin_cos();
    for i in [a, b] {
        let (dx, dy) = (x.pts[i][0] - pivot[0], x.pts[i][1] - pivot[1]);
        x.pts[i] = if x.free[i] > 0.0 { [pivot[0] + dx * k - dy * s, pivot[1] + dx * s + dy * k] } else { x.pts[i] };
    }
}

fn seg_angle(x: &Vars, seg: (usize, usize)) -> f64 {
    let (a, b) = (x.pts[seg.0], x.pts[seg.1]);
    (b[1] - a[1]).atan2(b[0] - a[0])
}

fn seg_len(x: &Vars, seg: (usize, usize)) -> f64 {
    let (a, b) = (x.pts[seg.0], x.pts[seg.1]);
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn set_len(x: &mut Vars, seg: (usize, usize), len: f64) {
    let (a, b) = seg;
    let (pa, pb) = (x.pts[a], x.pts[b]);
    let now = (pb[0] - pa[0]).hypot(pb[1] - pa[1]).max(1e-12);
    let (ux, uy) = ((pb[0] - pa[0]) / now, (pb[1] - pa[1]) / now);
    let err = len - now;
    let total = x.free[a] + x.free[b];
    if total == 0.0 {
        return;
    }
    let (ka, kb) = (x.free[a] / total, x.free[b] / total);
    x.pts[a] = [pa[0] - ux * err * ka, pa[1] - uy * err * ka];
    x.pts[b] = [pb[0] + ux * err * kb, pb[1] + uy * err * kb];
}

/// Distance between two points to a target: moves them along the line between them.
fn pull(x: &mut Vars, a: usize, b: usize, target: f64) {
    let (pa, pb) = (x.pts[a], x.pts[b]);
    let d = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
    let total = x.free[a] + x.free[b];
    if total == 0.0 {
        return;
    }
    let (ux, uy) = if d > 1e-12 { ((pb[0] - pa[0]) / d, (pb[1] - pa[1]) / d) } else { (1.0, 0.0) };
    let err = d - target;
    x.shift(a, [ux * err / total, uy * err / total]);
    x.shift(b, [-ux * err / total, -uy * err / total]);
}

/// Where a fillet's far end belongs: at a set place measured along and across the edge before it.
fn keep_target(x: &Vars, start: usize, edge: usize, offset: P) -> P {
    let (s, e) = (x.pts[start], x.pts[edge]);
    let len = (s[0] - e[0]).hypot(s[1] - e[1]).max(1e-12);
    let (ux, uy) = ((s[0] - e[0]) / len, (s[1] - e[1]) / len);
    [s[0] + ux * offset[0] - uy * offset[1], s[1] + uy * offset[0] + ux * offset[1]]
}

/// Moves a point and a line so the point sits `target` from the line, on the side it is on.
fn to_line(x: &mut Vars, p: usize, seg: (usize, usize), target: f64) {
    let (gap, n) = line_gap(x, seg, x.pts[p]);
    let side = if gap >= 0.0 { 1.0 } else { -1.0 };
    let err = gap - side * target;
    let total = x.free[p] + (x.free[seg.0] + x.free[seg.1]) / 2.0;
    if total > 0.0 {
        let d = err / total;
        x.shift(p, [-n[0] * d, -n[1] * d]);
        x.shift(seg.0, [n[0] * d, n[1] * d]);
        x.shift(seg.1, [n[0] * d, n[1] * d]);
    }
}

/// Signed distance from a line to a point, and the line's unit normal.
fn line_gap(x: &Vars, seg: (usize, usize), p: P) -> (f64, P) {
    let (a, b) = (x.pts[seg.0], x.pts[seg.1]);
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
    let n = [-(b[1] - a[1]) / l, (b[0] - a[0]) / l];
    ((p[0] - a[0]) * n[0] + (p[1] - a[1]) * n[1], n)
}

impl Constraint {
    /// The two points a rule on "a line, or two points" works on.
    fn pair(&self, x: &Vars, doc: &Doc) -> Option<(usize, usize)> {
        match self.items.as_slice() {
            [line @ Elem::Seg { .. }] => x.seg(*line, doc),
            [p, q] => Some((x.point(*p)?, x.point(*q)?)),
            _ => None,
        }
    }

    /// Moves things towards satisfying this rule.
    fn project(&self, x: &mut Vars, doc: &Doc) {
        let items = &self.items;
        match self.kind {
            Kind::Keep => {
                let (Some(a), Some(b), Some(edge)) = (x.point(items[0]), x.point(items[1]), x.point(items[2])) else { return };
                let wanted = keep_target(x, a, edge, self.at);
                let e = [x.pts[b][0] - wanted[0], x.pts[b][1] - wanted[1]];
                let total = x.free[a] + x.free[b];
                if total > 0.0 {
                    x.shift(a, [e[0] / total, e[1] / total]);
                    x.shift(b, [-e[0] / total, -e[1] / total]);
                }
            }
            Kind::Collinear => {
                let (Some(s1), Some(s2)) = (x.seg(items[0], doc), x.seg(items[1], doc)) else { return };
                to_line(x, s2.0, s1, 0.0);
                to_line(x, s2.1, s1, 0.0);
            }
            Kind::PointOnLine | Kind::LineDistance => {
                let (Some(p), Some(s)) = (x.point(items[0]), x.seg(items[1], doc)) else { return };
                let target = if self.kind == Kind::LineDistance { self.value } else { 0.0 };
                to_line(x, p, s, target);
            }
            Kind::PointOnCircle => {
                let (Some(p), Some(c), Elem::Round { v }) = (x.point(items[0]), x.point(items[1]), items[1]) else { return };
                let r = x.radius[&v].0;
                pull(x, p, c, r);
            }
            Kind::Symmetric => {
                let (Some(a), Some(b), Some(s)) = (x.point(items[0]), x.point(items[1]), x.seg(items[2], doc)) else { return };
                let total = x.free[a] + x.free[b];
                if total == 0.0 {
                    return;
                }
                let mid = [(x.pts[a][0] + x.pts[b][0]) / 2.0, (x.pts[a][1] + x.pts[b][1]) / 2.0];
                let (gap, n) = line_gap(x, s, mid);
                let dir = [n[1], -n[0]];
                let along = (x.pts[b][0] - x.pts[a][0]) * dir[0] + (x.pts[b][1] - x.pts[a][1]) * dir[1];
                let (step, slide) = (gap * 2.0 / total, along / total);
                x.shift(a, [-n[0] * step + dir[0] * slide, -n[1] * step + dir[1] * slide]);
                x.shift(b, [-n[0] * step - dir[0] * slide, -n[1] * step - dir[1] * slide]);
            }
            Kind::DistanceX | Kind::DistanceY => {
                let Some((a, b)) = self.pair(x, doc) else { return };
                let axis = usize::from(self.kind == Kind::DistanceY);
                let now = x.pts[b][axis] - x.pts[a][axis];
                let side = if now >= 0.0 { 1.0 } else { -1.0 };
                let total = x.free[a] + x.free[b];
                if total > 0.0 {
                    let err = (now - side * self.value) / total;
                    let d = if axis == 0 { [err, 0.0] } else { [0.0, err] };
                    x.shift(a, d);
                    x.shift(b, [-d[0], -d[1]]);
                }
            }
            Kind::Radius => {
                if let Elem::Round { v } = items[0] {
                    let f = x.radius[&v].1;
                    x.radius.insert(v, (self.value, f));
                }
            }
            Kind::Coincident | Kind::Concentric => {
                if let (Some(a), Some(b)) = (x.point(items[0]), x.point(items[1])) {
                    pull(x, a, b, 0.0);
                }
            }
            Kind::Horizontal | Kind::Vertical => {
                let Some((a, b)) = self.pair(x, doc) else { return };
                // The coordinate both ends must share.
                let axis = usize::from(self.kind == Kind::Horizontal);
                let (fa, fb) = (x.free[a], x.free[b]);
                if fa + fb > 0.0 {
                    let shared = if fa == 0.0 {
                        x.pts[a][axis]
                    } else if fb == 0.0 {
                        x.pts[b][axis]
                    } else {
                        (x.pts[a][axis] + x.pts[b][axis]) / 2.0
                    };
                    x.pts[a][axis] = shared;
                    x.pts[b][axis] = shared;
                }
            }
            Kind::Parallel | Kind::Perpendicular | Kind::Angle => {
                let (Some(s1), Some(s2)) = (x.seg(items[0], doc), x.seg(items[1], doc)) else { return };
                let wanted = match self.kind {
                    Kind::Parallel => 0.0,
                    Kind::Perpendicular => std::f64::consts::FRAC_PI_2,
                    _ => self.value.to_radians(),
                };
                let err = wrap_half_turn(seg_angle(x, s2) - seg_angle(x, s1) - wanted);
                let m1 = x.free[s1.0] + x.free[s1.1];
                let m2 = x.free[s2.0] + x.free[s2.1];
                if m1 + m2 > 0.0 {
                    rotate_seg(x, s1, err * m1 / (m1 + m2));
                    rotate_seg(x, s2, -err * m2 / (m1 + m2));
                }
            }
            Kind::Equal => match (items[0], items[1]) {
                (Elem::Round { v: a }, Elem::Round { v: b }) => {
                    let ((ra, fa), (rb, fb)) = (x.radius[&a], x.radius[&b]);
                    let target = if fa + fb == 0.0 { return } else { (ra * fa + rb * fb) / (fa + fb) };
                    let target = if fa == 0.0 { ra } else if fb == 0.0 { rb } else { target };
                    x.radius.insert(a, (target, fa));
                    x.radius.insert(b, (target, fb));
                }
                _ => {
                    let (Some(s1), Some(s2)) = (x.seg(items[0], doc), x.seg(items[1], doc)) else { return };
                    let (l1, l2) = (seg_len(x, s1), seg_len(x, s2));
                    let (m1, m2) = (x.free[s1.0] + x.free[s1.1], x.free[s2.0] + x.free[s2.1]);
                    if m1 + m2 > 0.0 {
                        let target = (l1 * m1 + l2 * m2) / (m1 + m2);
                        let target = if m1 == 0.0 { l1 } else if m2 == 0.0 { l2 } else { target };
                        set_len(x, s1, target);
                        set_len(x, s2, target);
                    }
                }
            },
            Kind::Tangent => match (items[0], items[1]) {
                (Elem::Round { v: a }, Elem::Round { v: b }) => {
                    let (Some(i), Some(j)) = (x.point(items[0]), x.point(items[1])) else { return };
                    let (ra, rb) = (x.radius[&a].0, x.radius[&b].0);
                    let d = (x.pts[i][0] - x.pts[j][0]).hypot(x.pts[i][1] - x.pts[j][1]);
                    let target = if d < (ra - rb).abs().max(ra.min(rb)) { (ra - rb).abs() } else { ra + rb };
                    pull(x, i, j, target);
                }
                (line, Elem::Round { v }) | (Elem::Round { v }, line) => {
                    let Some(s) = x.seg(line, doc) else { return };
                    let Some(c) = x.point(Elem::Round { v }) else { return };
                    let r = x.radius[&v].0;
                    let (gap, n) = line_gap(x, s, x.pts[c]);
                    let err = gap.abs() - r;
                    let side = if gap >= 0.0 { 1.0 } else { -1.0 };
                    let total = x.free[c] + x.free[s.0].min(x.free[s.1]);
                    if total > 0.0 {
                        let d = err * side / total;
                        x.shift(c, [-n[0] * d, -n[1] * d]);
                        x.shift(s.0, [n[0] * d, n[1] * d]);
                        x.shift(s.1, [n[0] * d, n[1] * d]);
                    }
                }
                _ => {}
            },
            Kind::Midpoint => {
                let (Some(p), Some(s)) = (x.point(items[0]), x.seg(items[1], doc)) else { return };
                let mid = [(x.pts[s.0][0] + x.pts[s.1][0]) / 2.0, (x.pts[s.0][1] + x.pts[s.1][1]) / 2.0];
                let e = [x.pts[p][0] - mid[0], x.pts[p][1] - mid[1]];
                let line = (x.free[s.0] + x.free[s.1]) / 2.0;
                let total = x.free[p] + line;
                if total > 0.0 {
                    x.shift(p, [-e[0] / total, -e[1] / total]);
                    x.shift(s.0, [e[0] / total, e[1] / total]);
                    x.shift(s.1, [e[0] / total, e[1] / total]);
                }
            }
            Kind::Fixed => {
                if let Some(p) = x.point(items[0]) {
                    x.pts[p] = self.at;
                }
            }
            Kind::Distance => match items.as_slice() {
                [line @ Elem::Seg { .. }] => {
                    if let Some((a, b)) = x.seg(*line, doc) {
                        pull(x, a, b, self.value);
                    }
                }
                [p, q] => {
                    if let (Some(a), Some(b)) = (x.point(*p), x.point(*q)) {
                        pull(x, a, b, self.value);
                    }
                }
                _ => {}
            },
            Kind::Diameter => {
                if let Elem::Round { v } = items[0] {
                    let f = x.radius[&v].1;
                    x.radius.insert(v, (self.value / 2.0, f));
                }
            }
        }
    }

    /// How far this rule is from holding, in job units.
    fn error(&self, x: &Vars, doc: &Doc) -> f64 {
        let items = &self.items;
        let dist = |a: usize, b: usize| (x.pts[a][0] - x.pts[b][0]).hypot(x.pts[a][1] - x.pts[b][1]);
        match self.kind {
            Kind::Keep => match (x.point(items[0]), x.point(items[1]), x.point(items[2])) {
                (Some(a), Some(b), Some(edge)) => {
                    let wanted = keep_target(x, a, edge, self.at);
                    (x.pts[b][0] - wanted[0]).hypot(x.pts[b][1] - wanted[1])
                }
                _ => 0.0,
            },
            Kind::Collinear => match (x.seg(items[0], doc), x.seg(items[1], doc)) {
                (Some(s1), Some(s2)) => line_gap(x, s1, x.pts[s2.0]).0.abs().max(line_gap(x, s1, x.pts[s2.1]).0.abs()),
                _ => 0.0,
            },
            Kind::PointOnLine | Kind::LineDistance => match (x.point(items[0]), x.seg(items[1], doc)) {
                (Some(p), Some(s)) => {
                    let target = if self.kind == Kind::LineDistance { self.value } else { 0.0 };
                    (line_gap(x, s, x.pts[p]).0.abs() - target).abs()
                }
                _ => 0.0,
            },
            Kind::PointOnCircle => match (x.point(items[0]), x.point(items[1]), items[1]) {
                (Some(p), Some(c), Elem::Round { v }) => (dist(p, c) - x.radius[&v].0).abs(),
                _ => 0.0,
            },
            Kind::Symmetric => match (x.point(items[0]), x.point(items[1]), x.seg(items[2], doc)) {
                (Some(a), Some(b), Some(s)) => {
                    let mid = [(x.pts[a][0] + x.pts[b][0]) / 2.0, (x.pts[a][1] + x.pts[b][1]) / 2.0];
                    let (gap, n) = line_gap(x, s, mid);
                    let dir = [n[1], -n[0]];
                    let along = (x.pts[b][0] - x.pts[a][0]) * dir[0] + (x.pts[b][1] - x.pts[a][1]) * dir[1];
                    gap.abs() + along.abs()
                }
                _ => 0.0,
            },
            Kind::DistanceX | Kind::DistanceY => self.pair(x, doc).map_or(0.0, |(a, b)| {
                let axis = usize::from(self.kind == Kind::DistanceY);
                ((x.pts[b][axis] - x.pts[a][axis]).abs() - self.value).abs()
            }),
            Kind::Radius => match items[0] {
                Elem::Round { v } => (x.radius[&v].0 - self.value).abs(),
                _ => 0.0,
            },
            Kind::Coincident | Kind::Concentric => match (x.point(items[0]), x.point(items[1])) {
                (Some(a), Some(b)) => dist(a, b),
                _ => 0.0,
            },
            Kind::Horizontal | Kind::Vertical => self.pair(x, doc).map_or(0.0, |(a, b)| {
                let axis = usize::from(self.kind == Kind::Horizontal);
                (x.pts[a][axis] - x.pts[b][axis]).abs()
            }),
            Kind::Parallel | Kind::Perpendicular | Kind::Angle => match (x.seg(items[0], doc), x.seg(items[1], doc)) {
                (Some(s1), Some(s2)) => {
                    let wanted = match self.kind {
                        Kind::Parallel => 0.0,
                        Kind::Perpendicular => std::f64::consts::FRAC_PI_2,
                        _ => self.value.to_radians(),
                    };
                    wrap_half_turn(seg_angle(x, s2) - seg_angle(x, s1) - wanted).abs() * seg_len(x, s1).max(seg_len(x, s2))
                }
                _ => 0.0,
            },
            Kind::Equal => match (items[0], items[1]) {
                (Elem::Round { v: a }, Elem::Round { v: b }) => (x.radius[&a].0 - x.radius[&b].0).abs(),
                _ => match (x.seg(items[0], doc), x.seg(items[1], doc)) {
                    (Some(s1), Some(s2)) => (seg_len(x, s1) - seg_len(x, s2)).abs(),
                    _ => 0.0,
                },
            },
            Kind::Tangent => match (items[0], items[1]) {
                (Elem::Round { v: a }, Elem::Round { v: b }) => match (x.point(items[0]), x.point(items[1])) {
                    (Some(i), Some(j)) => {
                        let (ra, rb) = (x.radius[&a].0, x.radius[&b].0);
                        let d = dist(i, j);
                        (d - (ra + rb)).abs().min((d - (ra - rb).abs()).abs())
                    }
                    _ => 0.0,
                },
                (line, Elem::Round { v }) | (Elem::Round { v }, line) => match (x.seg(line, doc), x.point(Elem::Round { v })) {
                    (Some(s), Some(c)) => (line_gap(x, s, x.pts[c]).0.abs() - x.radius[&v].0).abs(),
                    _ => 0.0,
                },
                _ => 0.0,
            },
            Kind::Midpoint => match (x.point(items[0]), x.seg(items[1], doc)) {
                (Some(p), Some(s)) => {
                    let mid = [(x.pts[s.0][0] + x.pts[s.1][0]) / 2.0, (x.pts[s.0][1] + x.pts[s.1][1]) / 2.0];
                    (x.pts[p][0] - mid[0]).hypot(x.pts[p][1] - mid[1])
                }
                _ => 0.0,
            },
            Kind::Fixed => x.point(items[0]).map_or(0.0, |p| (x.pts[p][0] - self.at[0]).hypot(x.pts[p][1] - self.at[1])),
            Kind::Distance => match items.as_slice() {
                [line @ Elem::Seg { .. }] => x.seg(*line, doc).map_or(0.0, |(a, b)| (dist(a, b) - self.value).abs()),
                [p, q] => match (x.point(*p), x.point(*q)) {
                    (Some(a), Some(b)) => (dist(a, b) - self.value).abs(),
                    _ => 0.0,
                },
                _ => 0.0,
            },
            Kind::Diameter => match items[0] {
                Elem::Round { v } => (x.radius[&v].0 * 2.0 - self.value).abs(),
                _ => 0.0,
            },
        }
    }

    fn valid(&self, doc: &Doc) -> bool {
        self.items.iter().all(|e| {
            let Some(v) = doc.vectors.iter().find(|v| v.id == e.vector()) else { return false };
            match *e {
                Elem::Node { path, node, .. } => node_usable(v, path, node),
                Elem::Seg { path, seg, .. } => body_of(v) == Body::Nodes && v.paths.get(path).is_some_and(|p| seg < p.segments().count()),
                Elem::Centre { .. } => body_of(v) != Body::Nodes,
                Elem::Round { .. } => matches!(body_of(v), Body::Round | Body::Arc),
            }
        })
    }
}

/// Which points stay where they are during a solve.
#[derive(Clone, Debug, Default)]
pub struct Pins {
    pub vectors: Vec<u64>,
    pub node: Option<(u64, usize, usize)>,
    /// More points that stay put, for a line dragged by its middle.
    pub also: Vec<(u64, usize, usize)>,
}

fn build_vars(doc: &Doc, rules: &[Constraint], pins: &Pins, snap_fixed: bool) -> Vars {
    let mut x = Vars { pts: Vec::new(), free: Vec::new(), index: HashMap::new(), radius: HashMap::new(), arc_ends: Vec::new() };
    let mut involved: Vec<u64> = rules.iter().flat_map(|c| c.items.iter().map(|e| e.vector())).collect();
    involved.sort_unstable();
    involved.dedup();
    for id in involved {
        let Some(v) = doc.vectors.iter().find(|v| v.id == id) else { continue };
        let pinned = pins.vectors.contains(&id);
        match body_of(v) {
            Body::Round => {
                let (c, r) = as_circle(&v.paths[0]).unwrap();
                x.index.insert((id, CENTRE, 0), x.pts.len());
                x.pts.push(c);
                x.free.push(if pinned { 0.0 } else { 1.0 });
                x.radius.insert(id, (r, if pinned { 0.0 } else { 1.0 }));
            }
            Body::Arc => {
                let (c, r) = arc_fit(v).unwrap();
                x.index.insert((id, CENTRE, 0), x.pts.len());
                x.pts.push(c);
                x.free.push(if pinned { 0.0 } else { 1.0 });
                x.radius.insert(id, (r, if pinned { 0.0 } else { 1.0 }));
                for (pi, path) in v.paths.iter().enumerate() {
                    for ni in [0, path.pts.len() - 1] {
                        let i = x.pts.len();
                        x.index.insert((id, pi, ni), i);
                        x.pts.push(path.pts[ni]);
                        let held = pinned || pins.node == Some((id, pi, ni)) || pins.also.contains(&(id, pi, ni));
                        x.free.push(if held { 0.0 } else { 1.0 });
                        x.arc_ends.push((id, i));
                    }
                }
            }
            Body::Rigid => {
                let (lo, hi) = geom::bbox(&v.paths).unwrap_or(([0.0, 0.0], [0.0, 0.0]));
                x.index.insert((id, CENTRE, 0), x.pts.len());
                x.pts.push([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
                x.free.push(if pinned { 0.0 } else { 1.0 });
            }
            Body::Nodes => {
                for (pi, path) in v.paths.iter().enumerate() {
                    for (ni, p) in path.pts.iter().enumerate() {
                        x.index.insert((id, pi, ni), x.pts.len());
                        x.pts.push(*p);
                        let held = pinned || pins.node == Some((id, pi, ni)) || pins.also.contains(&(id, pi, ni));
                        x.free.push(if held { 0.0 } else { 1.0 });
                    }
                }
            }
        }
    }
    for c in rules.iter().filter(|c| c.kind == Kind::Fixed) {
        if let Some(p) = x.point(c.items[0]) {
            x.free[p] = 0.0;
            if snap_fixed {
                x.pts[p] = c.at;
            }
        }
    }
    x
}

/// Writes the solved points back; returns whether any shape actually changed.
fn write_back(doc: &mut Doc, x: &Vars, rules: &[Constraint]) -> bool {
    let mut changed = false;
    let eps = 1e-9;
    let arc_ids: Vec<u64> = x.arc_ends.iter().map(|e| e.0).collect();
    // The points rules hold on to; the rest of a curve follows them.
    let mut held: std::collections::HashSet<Key> = std::collections::HashSet::new();
    for e in rules.iter().flat_map(|c| c.items.iter()) {
        match *e {
            Elem::Node { v, path, node } => {
                held.insert((v, path, node));
            }
            Elem::Seg { v, path, seg } => {
                let n = doc.vectors.iter().find(|x| x.id == v).and_then(|x| x.paths.get(path)).map_or(1, |p| p.pts.len());
                held.insert((v, path, seg));
                held.insert((v, path, (seg + 1) % n));
            }
            _ => {}
        }
    }
    for v in &mut doc.vectors {
        let id = v.id;
        if arc_ids.contains(&id) {
            let c = x.pts[x.index[&(id, CENTRE, 0)]];
            let r = x.radius[&id].0;
            let mut moved = false;
            for (pi, path) in v.paths.iter_mut().enumerate() {
                let n = path.pts.len();
                let (a, b) = (x.pts[x.index[&(id, pi, 0)]], x.pts[x.index[&(id, pi, n - 1)]]);
                if (path.pts[0][0] - a[0]).abs() + (path.pts[0][1] - a[1]).abs() + (path.pts[n - 1][0] - b[0]).abs() + (path.pts[n - 1][1] - b[1]).abs() <= eps
                    && arc_fit_path(path).is_some_and(|(oc, or)| (oc[0] - c[0]).abs() + (oc[1] - c[1]).abs() + (or - r).abs() <= eps)
                {
                    continue;
                }
                path.pts = arc_points(&path.pts, c, r, a, b);
                moved = true;
            }
            if moved {
                v.prim = Prim::Free;
                changed = true;
            }
        } else if x.index.contains_key(&(id, CENTRE, 0)) {
            let c = x.pts[x.index[&(id, CENTRE, 0)]];
            if let Some((r, _)) = x.radius.get(&id) {
                let (old_c, old_r) = as_circle(&v.paths[0]).unwrap_or((c, *r));
                if (old_c[0] - c[0]).abs() + (old_c[1] - c[1]).abs() + (old_r - r).abs() > eps {
                    v.paths = vec![Path::closed(geom::circle(c, *r))];
                    v.prim = Prim::Circle { c, r: *r };
                    changed = true;
                }
            } else if let Some((lo, hi)) = geom::bbox(&v.paths) {
                let (dx, dy) = (c[0] - (lo[0] + hi[0]) / 2.0, c[1] - (lo[1] + hi[1]) / 2.0);
                if dx.abs() + dy.abs() > eps {
                    for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                        p[0] += dx;
                        p[1] += dy;
                    }
                    v.prim.translate(dx, dy);
                    changed = true;
                }
            }
        } else {
            let mut moved = false;
            for (pi, path) in v.paths.iter_mut().enumerate() {
                let old = path.pts.clone();
                let mut path_moved = false;
                for (ni, p) in path.pts.iter_mut().enumerate() {
                    if let Some(i) = x.index.get(&(id, pi, ni)) {
                        if (p[0] - x.pts[*i][0]).abs() + (p[1] - x.pts[*i][1]).abs() > eps {
                            *p = x.pts[*i];
                            path_moved = true;
                        }
                    }
                }
                if path_moved {
                    let held_here: Vec<bool> = (0..old.len()).map(|n| held.contains(&(id, pi, n))).collect();
                    carry_curves(&old, &mut path.pts, path.closed, &held_here);
                    moved = true;
                }
            }
            if moved {
                v.prim = Prim::Free;
                changed = true;
            }
        }
    }
    changed
}

fn arc_fit_path(path: &Path) -> Option<(P, f64)> {
    let probe = Vector { id: 0, paths: vec![path.clone()], layer: 0, prim: Prim::Free, construction: false, members: Vec::new(), piece: None };
    arc_fit(&probe)
}

/// An arc redrawn on circle `(c, r)` from `a` to `b`, turning the same way and with as many points.
fn arc_points(old: &[P], c: P, r: f64, a: P, b: P) -> Vec<P> {
    let tau = std::f64::consts::TAU;
    let old_c = arc_fit_path(&Path::open(old.to_vec())).map_or(c, |f| f.0);
    let angle = |p: P, o: P| (p[1] - o[1]).atan2(p[0] - o[0]);
    let mut old_sweep = 0.0;
    for w in old.windows(2) {
        let mut d = angle(w[1], old_c) - angle(w[0], old_c);
        d -= tau * (d / tau).round();
        old_sweep += d;
    }
    let start = angle(a, c);
    let mut sweep = (angle(b, c) - start).rem_euclid(tau);
    if old_sweep < 0.0 {
        sweep -= tau;
    }
    let n = old.len();
    let mut pts: Vec<P> = (0..n)
        .map(|i| {
            let t = start + sweep * i as f64 / (n - 1) as f64;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect();
    pts[0] = a;
    pts[n - 1] = b;
    pts
}

/// Curved stretches no rule holds keep their shape: they move and turn with the points beside them.
fn carry_curves(old: &[P], pts: &mut [P], closed: bool, held: &[bool]) {
    let n = old.len();
    if n < 4 {
        return;
    }
    let bends = |i: usize| -> bool {
        if !closed && (i == 0 || i == n - 1) {
            return true;
        }
        let (a, b, c) = (old[(i + n - 1) % n], old[i], old[(i + 1) % n]);
        let (u, v) = ((b[1] - a[1]).atan2(b[0] - a[0]), (c[1] - b[1]).atan2(c[0] - b[0]));
        let mut d = (v - u).abs();
        if d > std::f64::consts::PI {
            d = std::f64::consts::TAU - d;
        }
        d > 0.5
    };
    let anchors: Vec<usize> = (0..n).filter(|i| held[*i] || bends(*i)).collect();
    if anchors.len() < 2 {
        return;
    }
    let count = if closed { anchors.len() } else { anchors.len() - 1 };
    for k in 0..count {
        let (a, b) = (anchors[k], anchors[(k + 1) % anchors.len()]);
        let span = (b + n - a) % n;
        if span < 3 {
            continue;
        }
        let (oa, ob, na, nb) = (old[a], old[b], pts[a], pts[b]);
        let (ox, oy) = (ob[0] - oa[0], ob[1] - oa[1]);
        let (nx, ny) = (nb[0] - na[0], nb[1] - na[1]);
        let len = ox * ox + oy * oy;
        if len < 1e-18 {
            continue;
        }
        // The turn and stretch that carries the old ends onto the new ones.
        let (zr, zi) = ((nx * ox + ny * oy) / len, (ny * ox - nx * oy) / len);
        for s in 1..span {
            let i = (a + s) % n;
            let (px, py) = (old[i][0] - oa[0], old[i][1] - oa[1]);
            pts[i] = [na[0] + px * zr - py * zi, na[1] + px * zi + py * zr];
        }
    }
}

/// Applies the rules to the drawing. False, with nothing changed, when they can't all hold.
pub fn solve(doc: &mut Doc, pins: &Pins) -> bool {
    let rules = doc.constraints.clone();
    if rules.is_empty() {
        return true;
    }
    let tol = tolerance(doc);
    let mut x = build_vars(doc, &rules, pins, true);
    for _ in 0..1500 {
        if all_hold(&rules, &x, doc, tol) {
            write_back(doc, &x, &rules);
            return true;
        }
        project_all(&rules, &mut x, doc);
    }
    false
}

fn all_hold(rules: &[Constraint], x: &Vars, doc: &Doc, tol: f64) -> bool {
    let on_circle = x.arc_ends.iter().all(|(id, end)| {
        let c = x.pts[x.index[&(*id, CENTRE, 0)]];
        ((x.pts[*end][0] - c[0]).hypot(x.pts[*end][1] - c[1]) - x.radius[id].0).abs() <= tol
    });
    on_circle && rules.iter().all(|c| c.error(x, doc) <= tol)
}

fn project_all(rules: &[Constraint], x: &mut Vars, doc: &Doc) {
    for c in rules {
        c.project(x, doc);
    }
    for (id, end) in x.arc_ends.clone() {
        let c = x.index[&(id, CENTRE, 0)];
        let r = x.radius[&id].0;
        pull(x, end, c, r);
    }
}

/// For each shape under a rule: true when the rules leave it no freedom to move.
pub fn definition(doc: &Doc) -> HashMap<u64, bool> {
    let rules = &doc.constraints;
    let mut out = HashMap::new();
    if rules.is_empty() {
        return out;
    }
    let base = build_vars(doc, rules, &Pins::default(), true);
    if base.pts.len() > 240 {
        return out;
    }
    let tol = tolerance(doc) * 2.0;
    let delta = doc.job.width.max(doc.job.height) * 0.002;
    let settles = |mut x: Vars, check: &dyn Fn(&Vars) -> bool| -> bool {
        for _ in 0..500 {
            if all_hold(rules, &x, doc, tol) {
                return check(&x);
            }
            project_all(rules, &mut x, doc);
        }
        false
    };
    let can_move = |i: usize, axis: usize| -> bool {
        let mut x = base.clone();
        if x.free[i] == 0.0 {
            return false;
        }
        x.pts[i][axis] += delta;
        x.free[i] = 0.0;
        let want = x.pts[i][axis];
        settles(x, &|s: &Vars| (s.pts[i][axis] - want).abs() < delta * 0.25)
    };
    let radius_moves = |id: u64| -> bool {
        let mut x = base.clone();
        let (r, _) = x.radius[&id];
        x.radius.insert(id, (r + delta, 0.0));
        settles(x, &|s: &Vars| (s.radius[&id].0 - r - delta).abs() < delta * 0.25)
    };
    let mut free_by_vector: HashMap<u64, bool> = HashMap::new();
    for (key, &i) in &base.index {
        let moves = can_move(i, 0) || can_move(i, 1);
        let entry = free_by_vector.entry(key.0).or_insert(false);
        *entry = *entry || moves;
    }
    for id in base.radius.keys() {
        let moves = radius_moves(*id);
        let entry = free_by_vector.entry(*id).or_insert(false);
        *entry = *entry || moves;
    }
    for (id, free) in free_by_vector {
        out.insert(id, !free);
    }
    out
}

/// A rectangle keeps its square corners once a rule holds on to it.
fn square_rectangles(doc: &mut Doc, ids: &[u64]) {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        let is_rect = doc.vectors.iter().any(|v| v.id == id && matches!(v.paths.as_slice(), [only] if crate::bulk::is_rectangle(only)) && matches!(v.prim, Prim::Rect { .. } | Prim::Free));
        if is_rect && !doc.constraints.iter().any(|c| matches!(c.kind, Kind::Horizontal | Kind::Vertical) && c.items.len() == 1 && c.items[0].vector() == id) {
            for (seg, k) in [(0, Kind::Horizontal), (1, Kind::Vertical), (2, Kind::Horizontal), (3, Kind::Vertical)] {
                doc.constraints.push(Constraint { kind: k, items: vec![Elem::Seg { v: id, path: 0, seg }], value: 0.0, at: [0.0, 0.0], place: [0.0, 0.0] });
            }
        }
    }
}

/// Nudges every shape under a rule, each point differently, to see what the rules then restore.
fn jiggle(doc: &mut Doc, seed: f64) {
    let ids: Vec<u64> = doc.constraints.iter().flat_map(|c| c.items.iter().map(|e| e.vector())).collect();
    let size = doc.job.width.max(doc.job.height) * 0.01;
    let wobble = |k: f64| (k * 12.9898 + seed * 78.233).sin() * size;
    for (n, v) in doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)).enumerate() {
        let k = n as f64 + 1.0;
        let (dx, dy) = (wobble(k), wobble(k + 0.37));
        match body_of(v) {
            Body::Round => {
                let (c, r) = as_circle(&v.paths[0]).unwrap();
                let (c, r) = ([c[0] + dx, c[1] + dy], r * (1.0 + 0.02 * (k + seed).sin()));
                v.paths = vec![Path::closed(geom::circle(c, r))];
                v.prim = Prim::Circle { c, r };
            }
            Body::Nodes => {
                for (i, q) in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()).enumerate() {
                    let j = k * 7.0 + i as f64 * 1.61;
                    *q = [q[0] + wobble(j), q[1] + wobble(j + 0.53)];
                }
                v.prim = Prim::Free;
            }
            _ => {
                for q in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                    *q = [q[0] + dx, q[1] + dy];
                }
                v.prim.translate(dx, dy);
            }
        }
    }
}

fn this_rule_holds(doc: &Doc, rule: &Constraint) -> bool {
    let rules = std::slice::from_ref(rule);
    let x = build_vars(doc, rules, &Pins::default(), false);
    all_hold(rules, &x, doc, tolerance(doc) * 50.0)
}

/// Whether the rules already there make this one true anyway, wherever the shapes start from.
fn is_implied(doc: &Doc, rule: &Constraint) -> bool {
    if doc.constraints.is_empty() || !this_rule_holds(doc, rule) {
        return false;
    }
    // Only rules on these shapes can imply anything about them.
    if !rule.items.iter().all(|e| doc.constraints.iter().any(|c| c.items.iter().any(|o| o.vector() == e.vector()))) {
        return false;
    }
    [1.0, 2.7].iter().all(|seed| {
        let mut t = doc.clone();
        jiggle(&mut t, *seed);
        solve(&mut t, &Pins::default()) && this_rule_holds(&t, rule)
    })
}

/// Whether a change shrank a side of some shape down to nothing.
fn collapses(before: &Doc, after: &Doc) -> bool {
    let small = before.job.width.max(before.job.height) * 1e-4;
    before.vectors.iter().any(|old| {
        let Some(new) = after.vectors.iter().find(|v| v.id == old.id) else { return false };
        old.paths.iter().zip(&new.paths).any(|(p, q)| {
            p.pts.len() == q.pts.len() && p.segments().zip(q.segments()).any(|((a, b), (c, d))| (a[0] - b[0]).hypot(a[1] - b[1]) > small * 5.0 && (c[0] - d[0]).hypot(c[1] - d[1]) < small)
        })
    })
}

fn tolerance(doc: &Doc) -> f64 {
    doc.job.width.max(doc.job.height) * 1e-6
}

fn holds(doc: &Doc) -> bool {
    let tol = tolerance(doc) * 2.0;
    let x = build_vars(doc, &doc.constraints, &Pins::default(), false);
    all_hold(&doc.constraints, &x, doc, tol)
}

impl App {
    fn current_pins(&self) -> Pins {
        if let Some(e) = self.dim_anchor {
            return match e {
                Elem::Node { v, path, node } => Pins { node: Some((v, path, node)), ..Pins::default() },
                Elem::Seg { v, path, seg } => Pins { node: Some((v, path, seg)), ..Pins::default() },
                Elem::Centre { v } | Elem::Round { v } => Pins { vectors: vec![v], ..Pins::default() },
            };
        }
        match self.drag {
            Drag::Node { id, path, index } => Pins { node: Some((id, path, index)), ..Pins::default() },
            _ => Pins { vectors: self.selected.clone(), ..Pins::default() },
        }
    }

    /// Remembers which end of a dimension stays put when its value changes.
    fn anchor_dimension(&mut self, i: usize) {
        self.dim_anchor = self.doc.constraints.get(i).and_then(|c| c.items.first().copied());
    }

    /// Keeps the drawing true to its rules after any change. Runs once per frame.
    pub fn maintain_constraints(&mut self) {
        if self.doc.constraints.is_empty() {
            self.constraint_conflict = false;
            self.defined.clear();
            self.defined_print = None;
            return;
        }
        let before = self.doc.constraints.len();
        let doc = &self.doc;
        let kept: Vec<Constraint> = doc.constraints.iter().filter(|c| c.valid(doc)).cloned().collect();
        self.doc.constraints = kept;
        if self.doc.constraints.len() != before {
            self.status = tf!("Removed {} rule(s) whose shapes changed.", before - self.doc.constraints.len());
        }
        let dragging = !matches!(self.drag, Drag::None);
        if !dragging {
            let print = fingerprint(&self.doc);
            if self.defined_print != Some(print) {
                self.defined_print = Some(print);
                self.defined = definition(&self.doc);
            }
        }
        if holds(&self.doc) {
            self.constraint_conflict = false;
        } else {
            let print = fingerprint(&self.doc);
            if self.failed_print == Some(print) {
                return;
            }
            let pins = self.current_pins();
            self.dim_anchor = None;
            let solved = solve(&mut self.doc, &pins) || solve(&mut self.doc, &Pins::default());
            self.constraint_conflict = !solved;
            if solved {
                self.failed_print = None;
            } else {
                self.failed_print = Some(print);
                self.status = crate::i18n::tr("The rules can't all be met. Undo the last change, or remove a rule.").into();
            }
        }
    }

    pub fn start_constraint_form(&mut self) {
        self.picks.clear();
        self.start_xform(Xform::Constraints);
    }

    /// Opens the sketch form with one tool ready.
    pub fn start_sketch_tool(&mut self, kind: Option<Kind>, smart: bool) {
        self.start_constraint_form();
        if let LeftForm::Xform(f) = &mut self.left {
            f.constraint = kind;
            f.smart = smart;
        }
    }

    /// What a click on the sheet does while the sketch form is open.
    pub fn sketch_click(&mut self, f: &XformForm, at: P, reach: f64) {
        if f.smart {
            let hit = self.pick_element(at, reach, Want::Any);
            match hit {
                Some(e) if !self.picks.contains(&e) => {
                    self.picks.push(e);
                    if self.picks.len() == 2 {
                        let picks = std::mem::take(&mut self.picks);
                        self.make_dimension(&picks, f.dim_mode);
                    }
                }
                Some(_) => {}
                None if self.picks.len() == 1 => {
                    // A click on empty sheet settles for the one thing picked.
                    let picks = std::mem::take(&mut self.picks);
                    self.make_dimension(&picks, f.dim_mode);
                }
                None => self.status = crate::i18n::tr("Click a line, a circle, or two points to dimension them.").into(),
            }
        } else if let Some(kind) = f.constraint {
            self.pick_for_constraint(kind, at, reach);
        } else {
            match self.pick_element(at, reach, Want::Any) {
                Some(e) => {
                    if let Some(i) = self.picks.iter().position(|p| *p == e) {
                        self.picks.remove(i);
                    } else {
                        self.picks.push(e);
                    }
                }
                None => self.picks.clear(),
            }
        }
    }

    /// Adds the dimension that suits what was clicked, then asks for its value.
    pub fn make_dimension(&mut self, picks: &[Elem], mode: DimMode) -> bool {
        let centre = |e: Elem| if let Elem::Round { v } = e { Elem::Centre { v } } else { e };
        let linear = match mode {
            DimMode::Smart => Kind::Distance,
            DimMode::Horizontal => Kind::DistanceX,
            DimMode::Vertical => Kind::DistanceY,
        };
        let is_line = |e: Elem| matches!(e, Elem::Seg { .. });
        let (kind, items) = match picks {
            [e] if is_line(*e) => (linear, vec![*e]),
            [e @ Elem::Round { .. }] => (Kind::Diameter, vec![*e]),
            [_] => {
                self.status = crate::i18n::tr("Pick a second point to measure to.").into();
                return false;
            }
            [a, b] if is_line(*a) && is_line(*b) => (Kind::Angle, vec![*a, *b]),
            [a, b] if is_line(*a) => (Kind::LineDistance, vec![centre(*b), *a]),
            [a, b] if is_line(*b) => (Kind::LineDistance, vec![centre(*a), *b]),
            [a, b] => (linear, vec![centre(*a), centre(*b)]),
            _ => return false,
        };
        if !self.add_constraint(kind, items) {
            return false;
        }
        let i = self.doc.constraints.len() - 1;
        self.open_dimension(i);
        true
    }

    /// Starts typing a new value for a dimension.
    pub fn open_dimension(&mut self, i: usize) {
        if let Some(c) = self.doc.constraints.get(i) {
            self.dim_edit = Some(i);
            self.dim_text = crate::toolpath::trim((c.value * 10_000.0).round() / 10_000.0);
            self.dim_focus = true;
        }
    }

    /// Sets a dimension's value; the drawing follows on the next frame.
    pub fn set_dimension(&mut self, i: usize, value: f64) -> bool {
        let Some(kind) = self.doc.constraints.get(i).map(|c| c.kind) else { return false };
        let lowest = if kind == Kind::Angle { 0.0 } else { 1e-6 };
        if !value.is_finite() || value < lowest {
            self.status = crate::i18n::tr("A dimension needs a value above zero.").into();
            return false;
        }
        // Try it on a copy first: a value the other rules can't allow is simply refused.
        let mut trial = self.doc.clone();
        trial.constraints[i].value = value;
        let anchor = trial.constraints[i].items.first().copied();
        let saved = std::mem::replace(&mut self.dim_anchor, anchor);
        let pins = self.current_pins();
        self.dim_anchor = saved;
        if !solve(&mut trial, &pins) && !solve(&mut trial, &Pins::default()) {
            self.status = tf!("{} can't be {} with the other rules. Remove a rule to free it.", kind.name(), crate::toolpath::trim(value));
            return false;
        }
        self.checkpoint();
        self.doc.constraints[i].value = value;
        self.failed_print = None;
        self.anchor_dimension(i);
        true
    }

    /// Switches the picked and selected shapes between normal and construction geometry.
    pub fn set_construction(&mut self, on: bool) {
        let mut ids: Vec<u64> = self.picks.iter().map(|e| e.vector()).collect();
        ids.extend(self.selected.iter().copied());
        if ids.is_empty() {
            self.status = crate::i18n::tr("Pick the shapes to change first.").into();
            return;
        }
        self.checkpoint();
        let mut n = 0;
        for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
            v.construction = on;
            n += 1;
        }
        self.status = if on { tf!("{n} shape(s) are now construction: they guide the drawing but are never cut.") } else { tf!("{n} shape(s) are now normal.") };
    }

    /// A dimension's lines and label, in screen positions.
    pub fn dim_parts(&self, c: &Constraint) -> Option<DimParts> {
        let unit_value = crate::toolpath::trim((c.value * 10_000.0).round() / 10_000.0);
        let place = vec2(c.place[0] as f32, -c.place[1] as f32) * self.scale;
        match c.kind {
            Kind::Distance | Kind::DistanceX | Kind::DistanceY | Kind::LineDistance => {
                let (a, b) = match (c.kind, c.items.as_slice()) {
                    (Kind::LineDistance, [p, line]) => {
                        let p = self.element_point(*p)?;
                        let (s, e) = self.seg_points(*line)?;
                        let (dx, dy) = (e[0] - s[0], e[1] - s[1]);
                        let t = ((p[0] - s[0]) * dx + (p[1] - s[1]) * dy) / (dx * dx + dy * dy).max(1e-12);
                        (p, [s[0] + dx * t, s[1] + dy * t])
                    }
                    (_, [line @ Elem::Seg { .. }]) => self.seg_points(*line)?,
                    (_, [p, q]) => (self.element_point(*p)?, self.element_point(*q)?),
                    _ => return None,
                };
                let (p, q) = match c.kind {
                    Kind::DistanceX => (a, [b[0], a[1]]),
                    Kind::DistanceY => (a, [a[0], b[1]]),
                    _ => (a, b),
                };
                let (sp, sq, real_b) = (self.to_screen(p), self.to_screen(q), self.to_screen(b));
                let d = sq - sp;
                let len = d.length();
                let dir = if len > 1e-3 { d / len } else { vec2(1.0, 0.0) };
                let mut n = vec2(dir.y, -dir.x);
                if n.y > 0.0 {
                    n = -n;
                }
                // The side of a shape gets its dimension outside the shape.
                if let [Elem::Seg { v, .. }] = c.items.as_slice() {
                    if let Some((lo, hi)) = self.doc.vectors.iter().find(|x| x.id == *v).and_then(|x| geom::bbox(&x.paths)) {
                        let middle = self.to_screen([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
                        if (sp + d * 0.5 - middle).dot(n) < -1.0 {
                            n = -n;
                        }
                    }
                }
                let lift = 22.0 + place.dot(n);
                let (lp, lq) = (sp + n * lift, sq + n * lift);
                let label = lp + d * 0.5 + dir * place.dot(dir);
                let mut lines = vec![(sp, lp), (real_b, lq), (lp, lq)];
                for (end, way) in [(lp, dir), (lq, -dir)] {
                    let side = vec2(-way.y, way.x);
                    lines.push((end, end + way * 7.0 + side * 2.5));
                    lines.push((end, end + way * 7.0 - side * 2.5));
                }
                Some(DimParts { lines, label, text: unit_value })
            }
            Kind::Diameter | Kind::Radius => {
                let Elem::Round { v } = c.items[0] else { return None };
                let (centre, r) = round_shape(self.doc.vectors.iter().find(|x| x.id == v)?)?;
                let s = self.to_screen(centre);
                let way = vec2(0.7071, -0.7071);
                let edge = s + way * (r as f32 * self.scale);
                let label = edge + way * 22.0 + place;
                let text = if c.kind == Kind::Radius { format!("R {unit_value}") } else { format!("\u{00D8} {unit_value}") };
                let mut lines = vec![(edge, label)];
                if c.kind == Kind::Radius {
                    lines.push((s, edge));
                }
                let side = vec2(-way.y, way.x);
                lines.push((edge, edge + way * 7.0 + side * 2.5));
                lines.push((edge, edge + way * 7.0 - side * 2.5));
                Some(DimParts { lines, label, text })
            }
            Kind::Angle => {
                let (a, b) = self.seg_points(c.items[0])?;
                let (e, f) = self.seg_points(c.items[1])?;
                let (dx1, dy1, dx2, dy2) = (b[0] - a[0], b[1] - a[1], f[0] - e[0], f[1] - e[1]);
                let cross = dx1 * dy2 - dy1 * dx2;
                let pivot = if cross.abs() > 1e-9 {
                    let t = ((e[0] - a[0]) * dy2 - (e[1] - a[1]) * dx2) / cross;
                    [a[0] + dx1 * t, a[1] + dy1 * t]
                } else {
                    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
                };
                let s = self.to_screen(pivot);
                let start = dy1.atan2(dx1) as f32;
                let sweep = (c.value as f32).to_radians();
                let radius = 34.0;
                let at = |t: f32| s + vec2(t.cos(), -t.sin()) * radius;
                let mut lines = Vec::new();
                for i in 0..16 {
                    let (t0, t1) = (start + sweep * i as f32 / 16.0, start + sweep * (i + 1) as f32 / 16.0);
                    lines.push((at(t0), at(t1)));
                }
                let middle = start + sweep / 2.0;
                let label = s + vec2(middle.cos(), -middle.sin()) * (radius + 16.0) + place;
                Some(DimParts { lines, label, text: format!("{:.2}\u{00B0}", c.value) })
            }
            _ => None,
        }
    }

    /// The dimension whose label is under a screen position.
    pub fn dim_at(&self, at: egui::Pos2) -> Option<usize> {
        if !self.show_constraints {
            return None;
        }
        self.doc.constraints.iter().enumerate().find_map(|(i, c)| {
            let parts = self.dim_parts(c)?;
            let size = vec2(parts.text.chars().count() as f32 * 7.0 + 10.0, 18.0);
            Rect::from_center_size(parts.label, size).contains(at).then_some(i)
        })
    }

    /// The small box for typing a dimension's value, next to its label.
    pub fn dim_editor(&mut self, ctx: &egui::Context) {
        let Some(i) = self.dim_edit else { return };
        let Some(c) = self.doc.constraints.get(i) else {
            self.dim_edit = None;
            return;
        };
        let kind = c.kind;
        let at = self.dim_parts(c).map_or(pos2(400.0, 300.0), |p| p.label + vec2(14.0, 12.0));
        let unit = if kind == Kind::Angle { crate::i18n::tr("degrees") } else { self.doc.job.unit() };
        let mut done: Option<bool> = None;
        egui::Window::new(crate::i18n::tr("Modify")).title_bar(false).resizable(false).fixed_pos(at).show(ctx, |ui| {
            crate::ui::bold(ui, kind.name());
            ui.horizontal(|ui| {
                let edit = ui.add(egui::TextEdit::singleline(&mut self.dim_text).desired_width(70.0));
                if self.dim_focus {
                    edit.request_focus();
                    self.dim_focus = false;
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                ui.label(unit);
            });
            if let Some(why) = &self.dim_error {
                ui.set_max_width(210.0);
                ui.colored_label(theme::ERROR, why);
            }
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Set Value")).clicked() {
                    done = Some(true);
                }
                if ui.button(crate::i18n::tr("Keep As Is")).clicked() {
                    done = Some(false);
                }
            });
        });
        match done {
            Some(true) => match self.dim_text.trim().replace(',', ".").parse::<f64>() {
                Ok(v) if self.set_dimension(i, v) => {
                    self.dim_edit = None;
                    self.dim_error = None;
                }
                Ok(_) => self.dim_error = Some(self.status.clone()),
                Err(_) => self.dim_error = Some(crate::i18n::tr("Type a number, for example 4 or 12.5.").into()),
            },
            Some(false) => {
                self.dim_edit = None;
                self.dim_error = None;
            }
            None => {}
        }
    }

    /// The element under the pointer that the current rule can use.
    pub fn pick_element(&self, at: P, reach: f64, want: Want) -> Option<Elem> {
        let mut best: Option<(f64, Elem)> = None;
        let mut consider = |d: f64, e: Elem| {
            if d <= reach && best.is_none_or(|b| d < b.0) {
                best = Some((d, e));
            }
        };
        let points = matches!(want, Want::Point | Want::PointOrLine | Want::Any);
        let lines = matches!(want, Want::Line | Want::LineOrCircle | Want::PointOrLine | Want::Any);
        let circles = matches!(want, Want::Circle | Want::LineOrCircle | Want::Any);
        for v in self.doc.vectors.iter().filter(|v| self.doc.layers.get(v.layer).is_none_or(|l| l.visible && !l.locked)) {
            match body_of(v) {
                Body::Round => {
                    let (c, r) = as_circle(&v.paths[0]).unwrap();
                    let d = (at[0] - c[0]).hypot(at[1] - c[1]);
                    if points {
                        consider(d * 0.5, Elem::Centre { v: v.id });
                    }
                    if circles {
                        // Anywhere inside counts, so small holes are easy to hit;
                        // when the centre can be picked too, a click right on it means the centre.
                        let inside = if points { reach * 0.4 } else { 0.0 };
                        consider(if d <= r { inside } else { d - r }, Elem::Round { v: v.id });
                    }
                }
                Body::Arc => {
                    let (c, _) = arc_fit(v).unwrap();
                    if points {
                        consider((at[0] - c[0]).hypot(at[1] - c[1]) * 0.5, Elem::Centre { v: v.id });
                        for (pi, path) in v.paths.iter().enumerate() {
                            for ni in [0, path.pts.len() - 1] {
                                let q = path.pts[ni];
                                consider((q[0] - at[0]).hypot(q[1] - at[1]) * 0.5, Elem::Node { v: v.id, path: pi, node: ni });
                            }
                        }
                    }
                    if circles {
                        let gap = v.paths.iter().flat_map(|p| p.segments()).map(|(a, b)| geom::dist2_seg(at, a, b)).fold(f64::MAX, f64::min).sqrt();
                        consider(gap, Elem::Round { v: v.id });
                    }
                }
                Body::Rigid => {
                    if points {
                        if let Some((lo, hi)) = geom::bbox(&v.paths) {
                            consider((at[0] - (lo[0] + hi[0]) / 2.0).hypot(at[1] - (lo[1] + hi[1]) / 2.0), Elem::Centre { v: v.id });
                        }
                    }
                }
                Body::Nodes => {
                    for (pi, path) in v.paths.iter().enumerate() {
                        if points {
                            for (ni, q) in path.pts.iter().enumerate() {
                                consider((q[0] - at[0]).hypot(q[1] - at[1]) * 0.5, Elem::Node { v: v.id, path: pi, node: ni });
                            }
                        }
                        if lines {
                            for (si, (a, b)) in path.segments().enumerate() {
                                consider(geom::dist2_seg(at, a, b).sqrt(), Elem::Seg { v: v.id, path: pi, seg: si });
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Records a click for the rule being made, and adds the rule once it has what it needs.
    pub fn pick_for_constraint(&mut self, kind: Kind, at: P, reach: f64) {
        let Some(want) = kind.want(&self.picks) else { return };
        let Some(e) = self.pick_element(at, reach, want) else {
            self.status = kind.prompt().into();
            return;
        };
        if self.picks.contains(&e) {
            return;
        }
        self.picks.push(e);
        if kind.want(&self.picks).is_none() {
            let items = std::mem::take(&mut self.picks);
            self.add_constraint(kind, items);
        }
    }

    fn element_point(&self, e: Elem) -> Option<P> {
        let v = self.doc.vectors.iter().find(|v| v.id == e.vector())?;
        match e {
            Elem::Node { path, node, .. } => v.paths.get(path)?.pts.get(node).copied(),
            Elem::Centre { .. } | Elem::Round { .. } => {
                if let Some((c, _)) = round_shape(v) {
                    return Some(c);
                }
                let (lo, hi) = geom::bbox(&v.paths)?;
                Some([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
            }
            Elem::Seg { path, seg, .. } => {
                let (a, b) = v.paths.get(path)?.segments().nth(seg)?;
                Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0])
            }
        }
    }

    fn seg_points(&self, e: Elem) -> Option<(P, P)> {
        let Elem::Seg { v, path, seg } = e else { return None };
        self.doc.vectors.iter().find(|x| x.id == v)?.paths.get(path)?.segments().nth(seg)
    }

    /// What a dimension's value is in the drawing as it stands.
    fn measure(&self, kind: Kind, items: &[Elem]) -> f64 {
        match (kind, items) {
            (Kind::Distance, [line @ Elem::Seg { .. }]) => self.seg_points(*line).map_or(0.0, |(a, b)| (b[0] - a[0]).hypot(b[1] - a[1])),
            (Kind::Distance, [p, q]) => match (self.element_point(*p), self.element_point(*q)) {
                (Some(a), Some(b)) => (b[0] - a[0]).hypot(b[1] - a[1]),
                _ => 0.0,
            },
            (Kind::DistanceX | Kind::DistanceY, _) => {
                let ends = match items {
                    [line @ Elem::Seg { .. }] => self.seg_points(*line),
                    [p, q] => self.element_point(*p).zip(self.element_point(*q)),
                    _ => None,
                };
                let axis = usize::from(kind == Kind::DistanceY);
                ends.map_or(0.0, |(a, b)| (b[axis] - a[axis]).abs())
            }
            (Kind::LineDistance, [p, line]) => match (self.element_point(*p), self.seg_points(*line)) {
                (Some(q), Some((a, b))) => geom_line_gap(a, b, q),
                _ => 0.0,
            },
            (Kind::Radius, [Elem::Round { v }]) => self.doc.vectors.iter().find(|x| x.id == *v).and_then(round_shape).map_or(0.0, |(_, r)| r),
            (Kind::Diameter, [Elem::Round { v }]) => self.doc.vectors.iter().find(|x| x.id == *v).and_then(round_shape).map_or(0.0, |(_, r)| r * 2.0),
            (Kind::Angle, [s1, s2]) => match (self.seg_points(*s1), self.seg_points(*s2)) {
                (Some((a, b)), Some((c, d))) => {
                    let turn = (d[1] - c[1]).atan2(d[0] - c[0]) - (b[1] - a[1]).atan2(b[0] - a[0]);
                    turn.rem_euclid(std::f64::consts::PI).to_degrees()
                }
                _ => 0.0,
            },
            _ => 0.0,
        }
    }

    /// Runs an edit that may renumber a shape's points or split it, and keeps every rule on
    /// the same edge, point or circle as before, wherever that has gone.
    pub fn keep_rules<R>(&mut self, edit: impl FnOnce(&mut App) -> R) -> R {
        if self.doc.constraints.is_empty() {
            return edit(self);
        }
        let before = self.doc.vectors.clone();
        let result = edit(self);
        self.follow_rules(&before);
        result
    }

    fn follow_rules(&mut self, before: &[Vector]) {
        let changed: Vec<u64> = before
            .iter()
            .filter(|old| self.doc.vectors.iter().find(|v| v.id == old.id).is_none_or(|v| v.paths != old.paths))
            .map(|v| v.id)
            .collect();
        if changed.is_empty() {
            return;
        }
        let tol = self.doc.job.width.max(self.doc.job.height) * 1e-4;
        let mut kept: Vec<Constraint> = Vec::new();
        let mut dropped = 0;
        let mut splits: Vec<Vec<Elem>> = Vec::new();
        for rule in std::mem::take(&mut self.doc.constraints) {
            if !rule.items.iter().any(|e| changed.contains(&e.vector())) {
                kept.push(rule);
                continue;
            }
            let mut items = Vec::new();
            let mut shortened = false;
            let mut pieces_of_single: Vec<Elem> = Vec::new();
            let mut lost = false;
            for e in &rule.items {
                if !changed.contains(&e.vector()) {
                    items.push(*e);
                    continue;
                }
                match self.find_again(before, *e, tol) {
                    Found::One(new) => items.push(new),
                    Found::Pieces(pieces, was_shortened) => {
                        items.push(pieces[0]);
                        shortened |= was_shortened;
                        if pieces.len() > 1 {
                            splits.push(pieces.clone());
                        }
                        if rule.items.len() == 1 {
                            pieces_of_single = pieces;
                        }
                    }
                    Found::Gone => {
                        lost = true;
                        break;
                    }
                }
            }
            // Keeping these would stretch the other line back to its old length.
            if lost || (shortened && matches!(rule.kind, Kind::Equal | Kind::Midpoint)) {
                dropped += 1;
                continue;
            }
            let along_one_line = matches!(rule.kind, Kind::Horizontal | Kind::Vertical | Kind::Distance | Kind::DistanceX | Kind::DistanceY);
            let targets: Vec<Vec<Elem>> = if along_one_line && pieces_of_single.len() > 1 { pieces_of_single.iter().map(|p| vec![*p]).collect() } else { vec![items] };
            for items in targets {
                let mut next = Constraint { items, ..rule.clone() };
                if shortened && matches!(rule.kind, Kind::Distance | Kind::DistanceX | Kind::DistanceY) {
                    next.value = self.measure(rule.kind, &next.items);
                }
                kept.push(next);
            }
        }
        // The pieces of a split line stay on one straight line.
        for pieces in splits {
            for other in &pieces[1..] {
                let pair = vec![pieces[0], *other];
                if !kept.iter().any(|c| c.kind == Kind::Collinear && c.items == pair) {
                    kept.push(Constraint { kind: Kind::Collinear, items: pair, value: 0.0, at: [0.0, 0.0], place: [0.0, 0.0] });
                }
            }
        }
        self.doc.constraints = kept;
        self.failed_print = None;
        if dropped > 0 {
            self.status = tf!("{dropped} rule(s) on the part that changed were removed.");
        }
    }

    /// Finds, after an edit, what a rule's element pointed at before it.
    fn find_again(&self, before: &[Vector], e: Elem, tol: f64) -> Found {
        let Some(old) = before.iter().find(|v| v.id == e.vector()) else { return Found::Gone };
        // Its own shape first, then wherever the edit may have moved it.
        // A shape that still exists keeps its own rules: another one may just happen to line up.
        let own: Vec<&Vector> = self.doc.vectors.iter().filter(|v| v.id == old.id).collect();
        let search: Vec<&Vector> = if own.is_empty() { self.doc.vectors.iter().collect() } else { own };
        match e {
            Elem::Node { path, node, .. } => {
                let Some(p) = old.paths.get(path).and_then(|q| q.pts.get(node)).copied() else { return Found::Gone };
                for v in search.iter().filter(|v| matches!(body_of(v), Body::Nodes | Body::Arc)) {
                    for (pi, q) in v.paths.iter().enumerate() {
                        let spot = q.pts.iter().enumerate().position(|(ni, x)| node_usable(v, pi, ni) && (x[0] - p[0]).hypot(x[1] - p[1]) <= tol);
                        if let Some(ni) = spot {
                            return Found::One(Elem::Node { v: v.id, path: pi, node: ni });
                        }
                    }
                }
                Found::Gone
            }
            Elem::Seg { path, seg, .. } => {
                let Some((a, b)) = old.paths.get(path).and_then(|q| q.segments().nth(seg)) else { return Found::Gone };
                let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                if len <= tol {
                    return Found::Gone;
                }
                let dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
                let mut pieces: Vec<(f64, Elem)> = Vec::new();
                for v in search.iter().filter(|v| body_of(v) == Body::Nodes) {
                    for (pi, q) in v.paths.iter().enumerate() {
                        for (si, (c, d)) in q.segments().enumerate() {
                            if geom_line_gap(a, b, c) > tol || geom_line_gap(a, b, d) > tol {
                                continue;
                            }
                            let t = |p: P| (p[0] - a[0]) * dir[0] + (p[1] - a[1]) * dir[1];
                            let (lo, hi) = (t(c).min(t(d)), t(c).max(t(d)));
                            let overlap = hi.min(len) - lo.max(0.0);
                            if overlap > tol {
                                pieces.push((overlap, Elem::Seg { v: v.id, path: pi, seg: si }));
                            }
                        }
                    }
                    // Pieces found on this shape are the answer; another shape may only share the line.
                    if !pieces.is_empty() {
                        break;
                    }
                }
                if pieces.is_empty() {
                    return Found::Gone;
                }
                let total: f64 = pieces.iter().map(|p| p.0).sum();
                pieces.sort_by(|x, y| y.0.total_cmp(&x.0));
                Found::Pieces(pieces.into_iter().map(|p| p.1).collect(), total < len - tol)
            }
            Elem::Round { .. } | Elem::Centre { .. } => {
                if let Some((c, r)) = round_shape(old) {
                    for v in search.iter().filter(|v| matches!(body_of(v), Body::Round | Body::Arc)) {
                        if let Some((c2, r2)) = round_shape(v) {
                            if (c[0] - c2[0]).hypot(c[1] - c2[1]) <= tol && (r - r2).abs() <= tol {
                                return Found::One(if matches!(e, Elem::Round { .. }) { Elem::Round { v: v.id } } else { Elem::Centre { v: v.id } });
                            }
                        }
                    }
                    return Found::Gone;
                }
                // A rigid shape's centre only means the same thing if the shape is unchanged.
                let Some(now) = self.doc.vectors.iter().find(|v| v.id == old.id) else { return Found::Gone };
                match (geom::bbox(&old.paths), geom::bbox(&now.paths)) {
                    (Some(a), Some(b)) if body_of(now) == Body::Rigid && (a.0[0] - b.0[0]).abs() + (a.0[1] - b.0[1]).abs() + (a.1[0] - b.1[0]).abs() + (a.1[1] - b.1[1]).abs() <= tol => Found::One(e),
                    _ => Found::Gone,
                }
            }
        }
    }

    /// Ties the ends a trim just made to the lines that cut them, so the outline stays closed
    /// when the rules change later. A drawing with no rules on these shapes stays without any.
    pub fn attach_trim_ends(&mut self) {
        let pending = std::mem::take(&mut self.trim_ends);
        let tol = self.doc.job.width.max(self.doc.job.height) * 1e-4;
        for (trimmed, point, cutter) in pending {
            let cutter_id = cutter.vector();
            if !self.doc.constraints.iter().any(|c| c.items.iter().any(|e| e.vector() == trimmed || e.vector() == cutter_id)) {
                continue;
            }
            let node_at = |id: u64| -> Option<Elem> {
                let v = self.doc.vectors.iter().find(|x| x.id == id)?;
                for (pi, q) in v.paths.iter().enumerate() {
                    for (ni, x) in q.pts.iter().enumerate() {
                        if node_usable(v, pi, ni) && (x[0] - point[0]).hypot(x[1] - point[1]) <= tol {
                            return Some(Elem::Node { v: id, path: pi, node: ni });
                        }
                    }
                }
                None
            };
            let Some(end) = node_at(trimmed) else { continue };
            let rule = match (node_at(cutter_id), cutter) {
                (Some(other), _) => Some((Kind::Coincident, vec![end, other])),
                (None, Elem::Seg { .. }) => Some((Kind::PointOnLine, vec![end, cutter])),
                (None, Elem::Round { .. }) => Some((Kind::PointOnCircle, vec![end, cutter])),
                _ => None,
            };
            if let Some((kind, items)) = rule {
                let exists = self.doc.constraints.iter().any(|c| c.kind == kind && c.items == items);
                let valid = Constraint { kind, items: items.clone(), value: 0.0, at: [0.0, 0.0], place: [0.0, 0.0] };
                if !exists && valid.valid(&self.doc) {
                    // Two ends now joined don't also need one to stay on the other's line.
                    if let (Kind::Coincident, Some(other)) = (kind, items.get(1).copied()) {
                        self.doc.constraints.retain(|c| !(c.kind == Kind::PointOnLine && c.items[0] == other && c.items[1].vector() == trimmed));
                    }
                    self.doc.constraints.push(valid);
                }
            }
        }
        self.failed_print = None;
    }

    /// Keeps a new fillet's size when the shape it is on is under rules.
    pub fn hold_fillet(&mut self, id: u64, start: P, end: P) {
        if !self.doc.constraints.iter().any(|c| c.items.iter().any(|e| e.vector() == id)) {
            return;
        }
        let tol = self.doc.job.width.max(self.doc.job.height) * 1e-6;
        let Some(v) = self.doc.vectors.iter().find(|x| x.id == id) else { return };
        // The first point of the fillet, the point before it, and its last point.
        let spot = v.paths.iter().enumerate().find_map(|(pi, q)| {
            let n = q.pts.len();
            let near = |x: P, p: P| (x[0] - p[0]).hypot(x[1] - p[1]) <= tol;
            let si = q.pts.iter().position(|x| near(*x, start))?;
            let ei = q.pts.iter().position(|x| near(*x, end))?;
            let before = if si > 0 { si - 1 } else if q.closed { n - 1 } else { return None };
            Some((pi, si, ei, before, q.pts[before]))
        });
        let Some((pi, si, ei, before, edge)) = spot else { return };
        let len = (start[0] - edge[0]).hypot(start[1] - edge[1]);
        if si == ei || len <= tol {
            return;
        }
        let (ux, uy) = ((start[0] - edge[0]) / len, (start[1] - edge[1]) / len);
        let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
        let at = [dx * ux + dy * uy, -dx * uy + dy * ux];
        let node = |node| Elem::Node { v: id, path: pi, node };
        self.doc.constraints.push(Constraint { kind: Kind::Keep, items: vec![node(si), node(ei), node(before)], value: 0.0, at, place: [0.0, 0.0] });
    }

    /// Adds a rule, measuring its value from the drawing as it stands.
    pub fn add_constraint(&mut self, kind: Kind, items: Vec<Elem>) -> bool {
        let value = self.measure(kind, &items);
        let at = items.first().and_then(|e| self.element_point(*e)).unwrap_or([0.0, 0.0]);
        let mut trial = self.doc.clone();
        let ids: Vec<u64> = items.iter().map(|e| e.vector()).collect();
        square_rectangles(&mut trial, &ids);
        let same = |c: &Constraint| c.kind == kind && c.items.len() == items.len() && c.items.iter().all(|e| items.contains(e));
        let refuse = |app: &mut App, why: String| {
            app.sketch_note = Some(why.clone());
            app.status = why;
            false
        };
        if self.doc.constraints.iter().any(same) {
            return refuse(self, tf!("This {} rule is already there.", kind.name().to_lowercase()));
        }
        // Keeping a rectangle square may have just made this very rule.
        if !trial.constraints.iter().any(same) {
            let rule = Constraint { kind, items: items.clone(), value, at, place: [0.0, 0.0] };
            if is_implied(&trial, &rule) {
                return refuse(self, tf!("No need for a {} rule: the others already make this true.", kind.name().to_lowercase()));
            }
            let before = trial.clone();
            trial.constraints.push(rule);
            if !solve(&mut trial, &Pins::default()) {
                return refuse(self, tf!("A {} rule can't hold together with the rules already there.", kind.name().to_lowercase()));
            }
            if collapses(&before, &trial) {
                return refuse(self, tf!("A {} rule here would shrink a line to nothing.", kind.name().to_lowercase()));
            }
        }
        self.checkpoint();
        self.doc = trial;
        self.sketch_note = None;
        self.status = tf!("Added the {} rule.", kind.name().to_lowercase());
        true
    }

    /// Starts dragging something on the sheet while the rules are being set.
    pub fn begin_sketch_drag(&mut self, e: Elem, at: P) {
        self.checkpoint();
        square_rectangles(&mut self.doc, &[e.vector()]);
        self.drag = Drag::Sketch { elem: e, last: at };
    }

    /// Moves what is being dragged, as far as the rules let it go.
    pub fn sketch_drag(&mut self, e: Elem, from: P, to: P) {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let mut trial = self.doc.clone();
        let Some(v) = trial.vectors.iter_mut().find(|v| v.id == e.vector()) else { return };
        let mut pins = Pins::default();
        match e {
            Elem::Node { v: id, path, node } => {
                let Some(p) = v.paths.get_mut(path).and_then(|p| p.pts.get_mut(node)) else { return };
                *p = to;
                v.prim = Prim::Free;
                pins.node = Some((id, path, node));
            }
            Elem::Seg { v: id, path, seg } => {
                let Some(p) = v.paths.get_mut(path) else { return };
                let n = p.pts.len();
                if n < 2 {
                    return;
                }
                let next = (seg + 1) % n;
                for i in [seg, next] {
                    p.pts[i] = [p.pts[i][0] + dx, p.pts[i][1] + dy];
                    pins.also.push((id, path, i));
                }
                v.prim = Prim::Free;
            }
            Elem::Centre { v: id } => {
                for q in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                    *q = [q[0] + dx, q[1] + dy];
                }
                v.prim.translate(dx, dy);
                pins.vectors.push(id);
            }
            Elem::Round { v: id } => {
                // Dragging the edge of a circle changes its size.
                let [only] = v.paths.as_slice() else { return };
                let Some((c, _)) = as_circle(only) else { return };
                let r = (to[0] - c[0]).hypot(to[1] - c[1]).max(1e-6);
                v.paths = vec![Path::closed(geom::circle(c, r))];
                v.prim = Prim::Circle { c, r };
                pins.vectors.push(id);
            }
        }
        // First with the dragged part exactly under the pointer; failing that, the rules place it
        // as close as they allow. If nothing works it stays where it was.
        let mut exact = trial.clone();
        if solve(&mut exact, &pins) && !collapses(&self.doc, &exact) {
            self.doc = exact;
        } else if solve(&mut trial, &Pins::default()) && !collapses(&self.doc, &trial) {
            self.doc = trial;
        }
    }

    /// Ends a drag; one that changed nothing leaves no step of undo.
    pub fn end_sketch_drag(&mut self) {
        self.drag = Drag::None;
        let same = self.undo.last().is_some_and(|before| before.vectors.len() == self.doc.vectors.len() && before.vectors.iter().zip(&self.doc.vectors).all(|(a, b)| a.paths == b.paths));
        if same {
            self.undo.pop();
        } else {
            }
    }

    /// Where a rule's badge goes for one of the things it holds, and which way to shift when crowded.
    fn chip_spot(&self, e: Elem) -> Option<(egui::Pos2, egui::Vec2)> {
        let v = self.doc.vectors.iter().find(|x| x.id == e.vector())?;
        match e {
            Elem::Seg { .. } => {
                let (a, b) = self.seg_points(e)?;
                let (pa, pb) = (self.to_screen(a), self.to_screen(b));
                let mid = pa + (pb - pa) * 0.5;
                let along = (pb - pa).normalized();
                let mut out = vec2(along.y, -along.x);
                // Off to the outside of the shape, away from what is drawn inside it.
                if let Some((lo, hi)) = geom::bbox(&v.paths) {
                    let centre = self.to_screen([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
                    if (mid - centre).dot(out) < 0.0 {
                        out = -out;
                    }
                }
                Some((mid + out * 13.0, along))
            }
            Elem::Round { .. } => {
                let (c, r) = round_shape(v)?;
                let at = self.to_screen([c[0] + r * 0.707, c[1] - r * 0.707]);
                Some((at + vec2(11.0, 11.0), vec2(1.0, 0.0)))
            }
            Elem::Node { .. } | Elem::Centre { .. } => {
                let p = self.element_point(e)?;
                Some((self.to_screen(p) + vec2(12.0, -12.0), vec2(1.0, 0.0)))
            }
        }
    }

    /// The badges on the sheet: which rule, on which thing, and where.
    pub fn rule_chips(&self) -> Vec<(usize, Rect)> {
        let size = vec2(18.0, 18.0);
        let labels: Vec<Rect> = self
            .doc
            .constraints
            .iter()
            .filter_map(|c| self.dim_parts(c))
            .map(|p| Rect::from_center_size(p.label, vec2(p.text.chars().count() as f32 * 7.0 + 12.0, 18.0)))
            .collect();
        let mut out: Vec<(usize, Rect)> = Vec::new();
        for (i, c) in self.doc.constraints.iter().enumerate() {
            if c.kind.has_value() || c.kind == Kind::Keep {
                continue;
            }
            for e in &c.items {
                let Some((spot, shift)) = self.chip_spot(*e) else { continue };
                let mut r = Rect::from_center_size(spot, size);
                // Side by side rather than on top of a dimension or of each other.
                for _ in 0..12 {
                    let taken = labels.iter().any(|l| l.intersects(r)) || out.iter().any(|(_, o)| o.expand(1.0).intersects(r));
                    if !taken {
                        break;
                    }
                    r = r.translate(shift * 20.0);
                }
                out.push((i, r));
            }
        }
        out
    }

    /// Draws the rules' small labels, and highlights what is being picked.
    pub fn paint_constraints(&self, painter: &egui::Painter) {
        let accent = Stroke::new(2.5, theme::ACCENT);
        let highlight = |e: Elem| {
            match e {
                Elem::Node { .. } | Elem::Centre { .. } => {
                    if let Some(p) = self.element_point(e) {
                        painter.circle_filled(self.to_screen(p), 5.0, theme::ACCENT);
                    }
                }
                Elem::Seg { .. } => {
                    if let Some((a, b)) = self.seg_points(e) {
                        painter.line_segment([self.to_screen(a), self.to_screen(b)], accent);
                    }
                }
                Elem::Round { v } => {
                    if let Some((c, r)) = self.doc.vectors.iter().find(|x| x.id == v).and_then(round_shape) {
                        painter.circle_stroke(self.to_screen(c), r as f32 * self.scale, accent);
                    }
                }
            };
        };
        if let Some(e) = self.hover_elem {
            highlight(e);
        }
        for e in &self.picks {
            highlight(*e);
        }
        if !self.show_constraints {
            return;
        }
        let conflict = self.constraint_conflict;
        let dim_colour = if conflict { Color32::from_rgb(200, 50, 40) } else { Color32::from_rgb(28, 92, 196) };
        let lit_rule = self.hover_rule.or(self.selected_rule);
        for (i, c) in self.doc.constraints.iter().enumerate() {
            if c.kind.has_value() {
                if let Some(parts) = self.dim_parts(c) {
                    let colour = if lit_rule == Some(i) { theme::PICK } else { dim_colour };
                    for (a, b) in &parts.lines {
                        painter.line_segment([*a, *b], Stroke::new(1.0, colour));
                    }
                    let galley = painter.layout_no_wrap(parts.text.clone(), FontId::proportional(12.0), colour);
                    let rect = Rect::from_center_size(parts.label, galley.size() + vec2(6.0, 2.0));
                    painter.rect_filled(rect, 2.0, theme::SHEET);
                    painter.galley(rect.min + vec2(3.0, 1.0), galley, colour);
                }
            }
        }
        // What the rule under the pointer or chosen in the list holds on to.
        if let Some(rule) = lit_rule.and_then(|i| self.doc.constraints.get(i)) {
            let lit = Stroke::new(2.5, theme::PICK);
            for e in &rule.items {
                match *e {
                    Elem::Node { .. } | Elem::Centre { .. } => {
                        if let Some(p) = self.element_point(*e) {
                            painter.circle_filled(self.to_screen(p), 5.0, theme::PICK);
                        }
                    }
                    Elem::Seg { .. } => {
                        if let Some((a, b)) = self.seg_points(*e) {
                            painter.line_segment([self.to_screen(a), self.to_screen(b)], lit);
                        }
                    }
                    Elem::Round { v } => {
                        if let Some((c, r)) = self.doc.vectors.iter().find(|x| x.id == v).and_then(round_shape) {
                            painter.circle_stroke(self.to_screen(c), r as f32 * self.scale, lit);
                        }
                    }
                }
            }
        }
        for (i, r) in self.rule_chips() {
            let Some(rule) = self.doc.constraints.get(i) else { continue };
            let on = lit_rule == Some(i);
            let fill = if conflict {
                Color32::from_rgb(170, 46, 38)
            } else if on {
                theme::PICK
            } else {
                theme::PANEL
            };
            painter.rect_filled(r, 4.0, fill);
            crate::icons::paint(painter, r.shrink(2.0), relation_icon(rule.kind), true);
        }
        if let Some(i) = self.hover_rule {
            let name = self.doc.constraints.get(i).map_or("", |c| c.kind.name());
            if let Some((_, r)) = self.rule_chips().into_iter().find(|(n, _)| *n == i) {
                let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(12.0), Color32::WHITE);
                let tip = Rect::from_min_size(r.right_top() + vec2(4.0, -2.0), galley.size() + vec2(8.0, 4.0));
                painter.rect_filled(tip, 3.0, theme::PANEL);
                painter.galley(tip.min + vec2(4.0, 2.0), galley, Color32::WHITE);
            }
        }
    }

    /// How defined the drawing is, in words.
    pub fn definition_text(&self) -> Option<(String, Color32)> {
        if self.doc.constraints.is_empty() {
            return None;
        }
        if self.constraint_conflict {
            return Some((crate::i18n::tr("Over defined: the rules disagree").into(), theme::ERROR));
        }
        let open = self.defined.values().filter(|fully| !**fully).count();
        Some(if open == 0 && !self.defined.is_empty() {
            (crate::i18n::tr("Fully defined").into(), Color32::from_rgb(120, 214, 140))
        } else {
            (tf!("Under defined: {open} shape(s) can still move"), Color32::from_rgb(120, 170, 250))
        })
    }

    /// The form with the dimension tool, line types, relations, and the rules in the drawing.
    pub fn constraint_form(&mut self, ui: &mut egui::Ui, f: &mut XformForm) {
        use crate::icons;
        use crate::ui::{bold, card, number};
        let unit = self.doc.job.unit();
        let mut list_hover = None;
        if let Some((text, colour)) = self.definition_text() {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(10.0, 16.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, colour);
                ui.colored_label(colour, text);
            });
        }
        // What to do next, where it can't be missed.
        let prompt = match (f.smart, f.constraint) {
            (true, _) if self.picks.is_empty() => crate::i18n::tr("Click a line or a circle, or two things to measure between.").to_string(),
            (true, _) => crate::i18n::tr("Click empty sheet for this one, or a second thing.").to_string(),
            (false, Some(kind)) => kind.prompt().to_string(),
            _ if self.picks.is_empty() => crate::i18n::tr("Click things on the sheet, or drag them to see what can still move. The rules that fit stay lit.").to_string(),
            _ => tf!("{} picked. Choose a rule, or click empty sheet to start over.", self.picks.len()),
        };
        let note = self.sketch_note.clone();
        egui::Frame::new().fill(if note.is_some() { Color32::from_rgb(92, 44, 40) } else { theme::ACCENT_SOFT }).corner_radius(4).inner_margin(egui::Margin::symmetric(7, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(note.as_deref().unwrap_or(&prompt)).size(12.0));
        });
        card(ui, |ui| {
            let wide = ui.available_width();
            if ui.add(egui::Button::new(crate::i18n::tr("Smart Dimension")).selected(f.smart).min_size(vec2(wide, 24.0))).on_hover_text(crate::i18n::tr("Line: length. Circle: diameter. Two points: distance. Two lines: angle.")).clicked() {
                f.smart = !f.smart;
                f.constraint = None;
                self.picks.clear();
                self.sketch_note = None;
            }
            if f.smart {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.dim_mode, DimMode::Smart, crate::i18n::tr("Aligned"));
                    ui.radio_value(&mut f.dim_mode, DimMode::Horizontal, crate::i18n::tr("Across"));
                    ui.radio_value(&mut f.dim_mode, DimMode::Vertical, crate::i18n::tr("Up"));
                });
            }
        });
        let mut apply: Option<(Kind, Vec<Elem>)> = None;
        card(ui, |ui| {
            bold(ui, crate::i18n::tr("Relations"));
            ui.spacing_mut().button_padding.x = 3.0;
            egui::Grid::new("constraint_kinds").num_columns(2).spacing([2.0, 2.0]).show(ui, |ui| {
                for (i, kind) in RELATIONS.iter().enumerate() {
                    let fits = kind.arrange(&self.picks);
                    let usable = self.picks.is_empty() || fits.is_some();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), egui::Sense::hover());
                        icons::paint(ui.painter(), r, relation_icon(*kind), usable);
                        let response = ui.add_enabled(usable, egui::Button::selectable(f.constraint == Some(*kind), kind.name()));
                        if response.clicked() {
                            self.sketch_note = None;
                            match fits {
                                Some(items) if !self.picks.is_empty() => apply = Some((*kind, items)),
                                _ => {
                                    f.constraint = Some(*kind);
                                    f.smart = false;
                                    self.picks.clear();
                                }
                            }
                        }
                    });
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            if f.constraint.is_some() && ui.small_button(crate::i18n::tr("Stop this relation")).clicked() {
                f.constraint = None;
                self.picks.clear();
            }
        });
        if let Some((kind, items)) = apply {
            self.picks.clear();
            self.add_constraint(kind, items);
        }
        let mut remove = None;
        let mut changed = false;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                bold(ui, &tf!("Rules in this drawing: {}", self.doc.constraints.len()));
            });
            ui.checkbox(&mut self.show_constraints, crate::i18n::tr("Show the rules on the sheet"));
            if self.constraint_conflict {
                ui.colored_label(theme::ERROR, crate::i18n::tr("These rules can't all be met."));
            }
            if self.doc.constraints.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::tr("None yet. Pick a rule above, then click on the drawing.")).size(11.5).color(theme::MUTED));
            }
            // Dimensions first: those are the numbers worth reaching for.
            let mut order: Vec<usize> = (0..self.doc.constraints.len()).filter(|i| self.doc.constraints[*i].kind != Kind::Keep).collect();
            order.sort_by_key(|i| !self.doc.constraints[*i].kind.has_value());
            egui::ScrollArea::vertical().max_height(230.0).id_salt("rule_list").show(ui, |ui| {
                for i in order {
                    let on = self.selected_rule == Some(i) || self.hover_rule == Some(i);
                    let row = egui::Frame::new().fill(if on { theme::ACCENT_SOFT } else { Color32::TRANSPARENT }).corner_radius(3).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let kind = self.doc.constraints[i].kind;
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 18.0), egui::Sense::hover());
                            icons::paint(ui.painter(), r, relation_icon(kind), true);
                            if ui.add(egui::Label::new(kind.name()).sense(egui::Sense::click())).clicked() {
                                self.selected_rule = if self.selected_rule == Some(i) { None } else { Some(i) };
                            }
                            if kind.has_value() {
                                if number(ui, &mut self.doc.constraints[i].value, 48.0) {
                                    changed = true;
                                    self.dim_anchor = self.doc.constraints[i].items.first().copied();
                                }
                                ui.label(if kind == Kind::Angle { "deg" } else { unit });
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let response = ui.small_button("x").on_hover_text(crate::i18n::tr("Remove this rule"));
                                crate::ui::label_widget(&response, &tf!("Remove rule {}", i + 1));
                                if response.clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });
                    if ui.rect_contains_pointer(row.response.rect) {
                        list_hover = Some(i);
                    }
                }
            });
            if !self.doc.constraints.is_empty() && ui.button(crate::i18n::tr("Remove All Rules")).clicked() {
                self.checkpoint();
                self.doc.constraints.clear();
                self.selected_rule = None;
            }
        });
        self.hover_rule = list_hover;
        if let Some(i) = remove {
            self.remove_rule(i);
        }
        if changed {
            self.failed_print = None;
            for c in &mut self.doc.constraints {
                if c.kind.has_value() && c.kind != Kind::Angle {
                    c.value = c.value.max(1e-6);
                }
            }
        }
        card(ui, |ui| {
            bold(ui, crate::i18n::tr("Line Type"));
            let some = !self.picks.is_empty() || !self.selected.is_empty();
            ui.horizontal(|ui| {
                if ui.add_enabled(some, egui::Button::new(crate::i18n::tr("Normal"))).clicked() {
                    self.set_construction(false);
                }
                if ui.add_enabled(some, egui::Button::new(crate::i18n::tr("Construction"))).on_hover_text(crate::i18n::tr("Construction lines guide the drawing and are never cut.")).clicked() {
                    self.set_construction(true);
                }
            });
            ui.checkbox(&mut self.doc.new_construction, crate::i18n::tr("Draw new shapes as construction"));
        });
    }

    /// Removes one rule, in one step of undo.
    pub fn remove_rule(&mut self, i: usize) {
        if i < self.doc.constraints.len() {
            self.checkpoint();
            self.doc.constraints.remove(i);
            self.failed_print = None;
            self.selected_rule = None;
            self.hover_rule = None;
            self.sketch_note = None;
        }
    }
}

/// Where a rule's element went after an edit.
enum Found {
    One(Elem),
    /// The pieces of a line, longest first, and whether they cover less than the old line.
    Pieces(Vec<Elem>, bool),
    Gone,
}

/// The lines and label of a dimension as drawn on the sheet.
pub struct DimParts {
    pub lines: Vec<(egui::Pos2, egui::Pos2)>,
    pub label: egui::Pos2,
    pub text: String,
}

fn geom_line_gap(a: P, b: P, p: P) -> f64 {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-12);
    (((p[0] - a[0]) * -(b[1] - a[1]) + (p[1] - a[1]) * (b[0] - a[0])) / l).abs()
}

fn fingerprint(doc: &Doc) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |v: f64| {
        h ^= v.to_bits();
        h = h.wrapping_mul(0x100_0000_01b3);
    };
    for c in &doc.constraints {
        eat(c.value);
        for v in c.items.iter().filter_map(|e| doc.vectors.iter().find(|v| v.id == e.vector())) {
            for p in v.paths.iter().flat_map(|p| p.pts.iter()) {
                eat(p[0]);
                eat(p[1]);
            }
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{circle, rect};

    fn app() -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        a
    }

    fn line(a: &mut App, p: P, q: P) -> u64 {
        a.doc.add_vector(vec![Path::open(vec![p, q])], Prim::Free)
    }

    fn disc(a: &mut App, c: P, r: f64) -> u64 {
        a.doc.add_vector(vec![Path::closed(circle(c, r))], Prim::Circle { c, r })
    }

    fn seg(v: u64) -> Elem {
        Elem::Seg { v, path: 0, seg: 0 }
    }

    fn pts(a: &App, id: u64) -> Vec<P> {
        a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths[0].pts.clone()
    }

    fn circle_of(a: &App, id: u64) -> (P, f64) {
        as_circle(&a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths[0]).unwrap()
    }

    #[test]
    fn horizontal_and_vertical_straighten_a_line() {
        let mut a = app();
        let l = line(&mut a, [1.0, 1.0], [4.0, 2.0]);
        assert!(a.add_constraint(Kind::Horizontal, vec![seg(l)]));
        let p = pts(&a, l);
        assert!((p[0][1] - p[1][1]).abs() < 1e-5, "{p:?}");
        let m = line(&mut a, [5.0, 1.0], [6.0, 4.0]);
        assert!(a.add_constraint(Kind::Vertical, vec![seg(m)]));
        let p = pts(&a, m);
        assert!((p[0][0] - p[1][0]).abs() < 1e-5);
    }

    #[test]
    fn coincident_joins_two_lines_and_they_stay_joined_when_dragged() {
        let mut a = app();
        let l1 = line(&mut a, [0.0, 0.0], [2.0, 0.0]);
        let l2 = line(&mut a, [3.0, 1.0], [5.0, 1.0]);
        assert!(a.add_constraint(Kind::Coincident, vec![Elem::Node { v: l1, path: 0, node: 1 }, Elem::Node { v: l2, path: 0, node: 0 }]));
        let (p, q) = (pts(&a, l1)[1], pts(&a, l2)[0]);
        assert!((p[0] - q[0]).hypot(p[1] - q[1]) < 1e-4);
        // Move the second line: the first one's end comes along.
        a.selected = vec![l2];
        a.translate_selected(1.0, 2.0);
        a.maintain_constraints();
        let (p, q) = (pts(&a, l1)[1], pts(&a, l2)[0]);
        assert!((p[0] - q[0]).hypot(p[1] - q[1]) < 1e-4, "{p:?} {q:?}");
        assert!((q[0] - (2.5 + 1.0)).abs() < 1.0, "the moved line stays where it was put");
    }

    #[test]
    fn concentric_holes_follow_each_other() {
        let mut a = app();
        let inner = disc(&mut a, [2.0, 2.0], 0.5);
        let outer = disc(&mut a, [3.0, 3.0], 1.0);
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: inner }, Elem::Round { v: outer }]));
        a.selected = vec![outer];
        a.translate_selected(4.0, 1.0);
        a.maintain_constraints();
        let (c1, r1) = circle_of(&a, inner);
        let (c2, r2) = circle_of(&a, outer);
        assert!((c1[0] - c2[0]).hypot(c1[1] - c2[1]) < 1e-4, "{c1:?} {c2:?}");
        assert!((r1 - 0.5).abs() < 1e-3 && (r2 - 1.0).abs() < 1e-3, "sizes are untouched");
        assert!(matches!(a.doc.vectors[0].prim, Prim::Circle { .. }), "still an editable circle");
    }

    #[test]
    fn equal_circles_share_a_resize() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.5);
        let d2 = disc(&mut a, [5.0, 5.0], 0.5);
        assert!(a.add_constraint(Kind::Equal, vec![Elem::Round { v: d1 }, Elem::Round { v: d2 }]));
        a.selected = vec![d1];
        a.resize_each(Some(2.0), None, None, Datum::Centre);
        a.maintain_constraints();
        assert!((circle_of(&a, d2).1 - 1.0).abs() < 1e-3, "{:?}", circle_of(&a, d2));
        assert!((circle_of(&a, d1).1 - 1.0).abs() < 1e-3);
    }

    #[test]
    fn parallel_perpendicular_equal_length_and_angle() {
        let mut a = app();
        let l1 = line(&mut a, [0.0, 0.0], [4.0, 0.0]);
        let l2 = line(&mut a, [0.0, 2.0], [3.0, 3.0]);
        assert!(a.add_constraint(Kind::Parallel, vec![seg(l1), seg(l2)]));
        let (p, q) = (pts(&a, l1), pts(&a, l2));
        let cross = (p[1][0] - p[0][0]) * (q[1][1] - q[0][1]) - (p[1][1] - p[0][1]) * (q[1][0] - q[0][0]);
        assert!(cross.abs() < 1e-3, "{cross}");

        let mut b = app();
        let l1 = line(&mut b, [0.0, 0.0], [4.0, 0.0]);
        let l2 = line(&mut b, [5.0, 0.0], [7.0, 1.0]);
        assert!(b.add_constraint(Kind::Perpendicular, vec![seg(l1), seg(l2)]));
        let (p, q) = (pts(&b, l1), pts(&b, l2));
        let dot = (p[1][0] - p[0][0]) * (q[1][0] - q[0][0]) + (p[1][1] - p[0][1]) * (q[1][1] - q[0][1]);
        assert!(dot.abs() < 1e-3, "{dot}");
        assert!(b.add_constraint(Kind::Equal, vec![seg(l1), seg(l2)]));
        let (p, q) = (pts(&b, l1), pts(&b, l2));
        assert!(((p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]) - (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1])).abs() < 1e-3);

        let mut c = app();
        let l1 = line(&mut c, [0.0, 0.0], [4.0, 0.0]);
        let l2 = line(&mut c, [0.0, 2.0], [2.0, 4.0]);
        assert!(c.add_constraint(Kind::Angle, vec![seg(l1), seg(l2)]));
        assert!((c.doc.constraints[0].value - 45.0).abs() < 1e-6);
        c.doc.constraints[0].value = 30.0;
        c.maintain_constraints();
        let (p, q) = (pts(&c, l1), pts(&c, l2));
        let turn = ((q[1][1] - q[0][1]).atan2(q[1][0] - q[0][0]) - (p[1][1] - p[0][1]).atan2(p[1][0] - p[0][0])).to_degrees();
        assert!((turn - 30.0).abs() < 0.05, "{turn}");
    }

    #[test]
    fn tangent_makes_a_line_touch_a_circle() {
        let mut a = app();
        let d = disc(&mut a, [5.0, 5.0], 1.0);
        let l = line(&mut a, [1.0, 8.0], [9.0, 8.0]);
        assert!(a.add_constraint(Kind::Tangent, vec![seg(l), Elem::Round { v: d }]));
        let (c, r) = circle_of(&a, d);
        let p = pts(&a, l);
        let gap = geom::dist2_seg(c, p[0], p[1]).sqrt();
        assert!((gap - r).abs() < 1e-3, "{gap} vs {r}");

        let mut b = app();
        let d1 = disc(&mut b, [2.0, 5.0], 1.0);
        let d2 = disc(&mut b, [7.0, 5.0], 0.5);
        assert!(b.add_constraint(Kind::Tangent, vec![Elem::Round { v: d1 }, Elem::Round { v: d2 }]));
        let (c1, c2) = (circle_of(&b, d1).0, circle_of(&b, d2).0);
        assert!(((c1[0] - c2[0]).hypot(c1[1] - c2[1]) - 1.5).abs() < 1e-3);
    }

    #[test]
    fn midpoint_centres_a_point_on_a_line() {
        let mut a = app();
        let l = line(&mut a, [0.0, 0.0], [4.0, 0.0]);
        let d = disc(&mut a, [1.0, 3.0], 0.25);
        assert!(a.add_constraint(Kind::Midpoint, vec![Elem::Centre { v: d }, seg(l)]));
        let (c, _) = circle_of(&a, d);
        let p = pts(&a, l);
        assert!((c[0] - (p[0][0] + p[1][0]) / 2.0).abs() < 1e-3 && (c[1] - (p[0][1] + p[1][1]) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn fixed_shapes_stay_put_and_others_adjust() {
        let mut a = app();
        let anchor = disc(&mut a, [2.0, 2.0], 0.5);
        let other = disc(&mut a, [6.0, 6.0], 0.5);
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: anchor }]));
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: anchor }, Elem::Round { v: other }]));
        let (c, _) = circle_of(&a, anchor);
        assert!((c[0] - 2.0).abs() < 1e-6 && (c[1] - 2.0).abs() < 1e-6, "the fixed one didn't move");
        assert!((circle_of(&a, other).0[0] - 2.0).abs() < 1e-4);
        // Dragging the fixed shape away: it goes back.
        a.selected = vec![anchor];
        a.translate_selected(3.0, 0.0);
        a.maintain_constraints();
        assert!((circle_of(&a, anchor).0[0] - 2.0).abs() < 1e-4);
    }

    #[test]
    fn distance_and_diameter_values_can_be_changed_later() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 5.0], 0.5);
        let d2 = disc(&mut a, [6.0, 5.0], 0.5);
        assert!(a.add_constraint(Kind::Distance, vec![Elem::Centre { v: d1 }, Elem::Centre { v: d2 }]));
        assert!((a.doc.constraints[0].value - 4.0).abs() < 1e-3);
        a.doc.constraints[0].value = 6.0;
        a.maintain_constraints();
        let (c1, c2) = (circle_of(&a, d1).0, circle_of(&a, d2).0);
        assert!(((c1[0] - c2[0]).hypot(c1[1] - c2[1]) - 6.0).abs() < 1e-3);
        assert!(a.add_constraint(Kind::Diameter, vec![Elem::Round { v: d1 }]));
        let n = a.doc.constraints.len() - 1;
        a.doc.constraints[n].value = 2.0;
        a.maintain_constraints();
        assert!((circle_of(&a, d1).1 - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_rectangle_stays_square_when_tied_to_something() {
        let mut a = app();
        let r = a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 4.0, 2.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 4.0, h: 2.0, corner: 0.0 });
        let d = disc(&mut a, [1.0, 1.0], 0.5);
        assert!(a.add_constraint(Kind::Coincident, vec![Elem::Node { v: r, path: 0, node: 0 }, Elem::Centre { v: d }]));
        let p = pts(&a, r);
        assert!((p[0][1] - p[1][1]).abs() < 1e-3 && (p[1][0] - p[2][0]).abs() < 1e-3, "{p:?}");
        assert!((p[2][1] - p[3][1]).abs() < 1e-3 && (p[3][0] - p[0][0]).abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn impossible_rules_are_refused_and_change_nothing() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.5);
        let d2 = disc(&mut a, [6.0, 2.0], 0.5);
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d1 }]));
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d2 }]));
        let count = a.doc.constraints.len();
        assert!(!a.add_constraint(Kind::Coincident, vec![Elem::Centre { v: d1 }, Elem::Centre { v: d2 }]));
        assert_eq!(a.doc.constraints.len(), count);
        assert!((circle_of(&a, d2).0[0] - 6.0).abs() < 1e-6);
        assert!(a.status.contains("can't hold"));
    }

    #[test]
    fn a_conflict_from_a_value_change_is_flagged_without_damage() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.5);
        let d2 = disc(&mut a, [6.0, 2.0], 0.5);
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d1 }]));
        assert!(a.add_constraint(Kind::Distance, vec![Elem::Centre { v: d1 }, Elem::Centre { v: d2 }]));
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d2 }]));
        a.doc.constraints[1].value = 9.0;
        a.maintain_constraints();
        assert!(a.constraint_conflict);
        assert!((circle_of(&a, d2).0[0] - 6.0).abs() < 1e-6, "shapes stay as they were");
        a.doc.constraints[1].value = 4.0;
        a.maintain_constraints();
        assert!(!a.constraint_conflict, "fixing the value clears the warning");
    }

    #[test]
    fn deleting_a_shape_removes_its_rules_and_undo_brings_both_back() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.5);
        let d2 = disc(&mut a, [6.0, 2.0], 0.5);
        assert!(a.add_constraint(Kind::Equal, vec![Elem::Round { v: d1 }, Elem::Round { v: d2 }]));
        a.selected = vec![d1];
        a.delete_selected();
        a.maintain_constraints();
        assert!(a.doc.constraints.is_empty());
        a.do_undo();
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.len(), 1);
    }

    #[test]
    fn picking_finds_small_holes_and_completes_the_rule() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.08);
        let d2 = disc(&mut a, [6.0, 2.0], 0.08);
        a.pick_for_constraint(Kind::Concentric, [2.01, 2.01], 0.05);
        assert_eq!(a.picks, vec![Elem::Round { v: d1 }]);
        a.pick_for_constraint(Kind::Concentric, [9.0, 9.0], 0.05);
        assert_eq!(a.picks.len(), 1, "a click on nothing changes nothing");
        a.pick_for_constraint(Kind::Concentric, [6.0, 2.0], 0.05);
        assert!(a.picks.is_empty());
        assert_eq!(a.doc.constraints.len(), 1);
        assert_eq!(a.doc.constraints[0].items[1], Elem::Round { v: d2 });
    }

    #[test]
    fn rules_are_saved_with_the_project() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 2.0], 0.5);
        let d2 = disc(&mut a, [6.0, 2.0], 0.5);
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: d1 }, Elem::Round { v: d2 }]));
        let text = serde_json::to_string(&a.doc).unwrap();
        let back: Doc = serde_json::from_str(&text).unwrap();
        assert_eq!(back.constraints.len(), 1);
        assert_eq!(back.constraints[0].kind, Kind::Concentric);
        assert_eq!(back.constraints[0].items, a.doc.constraints[0].items);
    }

    #[test]
    fn toolpaths_wait_for_recalculate_when_a_rule_moves_their_shapes() {
        let mut a = app();
        let d1 = disc(&mut a, [2.0, 5.0], 0.5);
        let d2 = disc(&mut a, [7.0, 5.0], 0.5);
        a.selected = vec![d2];
        a.start_op(crate::toolpath::OpKind::Drill);
        a.calculate_op();
        a.selected = vec![d1];
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d1 }]));
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: d2 }, Elem::Round { v: d1 }]));
        a.maintain_constraints();
        let drilled = a.doc.toolpaths[0].moves.iter().find(|m| !m.rapid).unwrap().p;
        assert!((drilled[0] - 7.0).abs() < 1e-3, "not on its own: {drilled:?}");
        a.recalculate_all();
        let drilled = a.doc.toolpaths[0].moves.iter().find(|m| !m.rapid).unwrap().p;
        assert!((drilled[0] - 2.0).abs() < 1e-3, "the drill went with the hole: {drilled:?}");
    }

    #[test]
    fn collinear_point_on_line_and_circle_and_symmetric() {
        let mut a = app();
        let l1 = line(&mut a, [0.0, 0.0], [3.0, 0.0]);
        let l2 = line(&mut a, [5.0, 1.0], [8.0, 1.5]);
        assert!(a.add_constraint(Kind::Collinear, vec![seg(l1), seg(l2)]));
        let (p, q) = (pts(&a, l1), pts(&a, l2));
        assert!(geom_line_gap(p[0], p[1], q[0]) < 1e-3 && geom_line_gap(p[0], p[1], q[1]) < 1e-3);

        let mut b = app();
        let l = line(&mut b, [0.0, 5.0], [10.0, 5.0]);
        let d = disc(&mut b, [3.0, 8.0], 0.25);
        assert!(b.add_constraint(Kind::PointOnLine, vec![Elem::Centre { v: d }, seg(l)]));
        let p = pts(&b, l);
        assert!(geom_line_gap(p[0], p[1], circle_of(&b, d).0) < 1e-3);

        let mut e = app();
        let big = disc(&mut e, [5.0, 5.0], 2.0);
        let dot = disc(&mut e, [9.0, 5.0], 0.1);
        assert!(e.add_constraint(Kind::PointOnCircle, vec![Elem::Centre { v: dot }, Elem::Round { v: big }]));
        let (c1, r1) = circle_of(&e, big);
        let c2 = circle_of(&e, dot).0;
        assert!(((c1[0] - c2[0]).hypot(c1[1] - c2[1]) - r1).abs() < 1e-3);

        let mut g = app();
        let axis = line(&mut g, [5.0, 0.0], [5.0, 10.0]);
        let (d1, d2) = (disc(&mut g, [2.0, 3.0], 0.2), disc(&mut g, [7.0, 5.0], 0.2));
        assert!(g.add_constraint(Kind::Vertical, vec![seg(axis)]));
        assert!(g.add_constraint(Kind::Symmetric, vec![Elem::Centre { v: d1 }, Elem::Centre { v: d2 }, seg(axis)]));
        let (c1, c2) = (circle_of(&g, d1).0, circle_of(&g, d2).0);
        let x = pts(&g, axis)[0][0];
        assert!(((c1[0] + c2[0]) / 2.0 - x).abs() < 1e-3 && (c1[1] - c2[1]).abs() < 1e-3, "{c1:?} {c2:?} {x}");
    }

    #[test]
    fn horizontal_and_vertical_also_work_on_two_points() {
        let mut a = app();
        let (d1, d2) = (disc(&mut a, [2.0, 3.0], 0.2), disc(&mut a, [7.0, 5.0], 0.2));
        assert!(a.add_constraint(Kind::Horizontal, vec![Elem::Centre { v: d1 }, Elem::Centre { v: d2 }]));
        assert!((circle_of(&a, d1).0[1] - circle_of(&a, d2).0[1]).abs() < 1e-3);
    }

    #[test]
    fn smart_dimension_picks_the_right_measure() {
        let mut a = app();
        let l = line(&mut a, [1.0, 1.0], [4.0, 5.0]);
        let m = line(&mut a, [6.0, 1.0], [9.0, 1.0]);
        let d = disc(&mut a, [5.0, 8.0], 1.0);
        let e = disc(&mut a, [8.0, 8.0], 0.5);
        // Line alone: its length.
        assert!(a.make_dimension(&[seg(l)], DimMode::Smart));
        assert_eq!(a.doc.constraints.last().unwrap().kind, Kind::Distance);
        assert!((a.doc.constraints.last().unwrap().value - 5.0).abs() < 1e-6);
        assert!(a.dim_edit.is_some(), "the value box opens straight away");
        // Across only.
        assert!(a.make_dimension(&[seg(l)], DimMode::Horizontal));
        assert!((a.doc.constraints.last().unwrap().value - 3.0).abs() < 1e-6);
        // Circle alone: its diameter.
        assert!(a.make_dimension(&[Elem::Round { v: d }], DimMode::Smart));
        assert_eq!(a.doc.constraints.last().unwrap().kind, Kind::Diameter);
        // Two lines: the angle between them.
        assert!(a.make_dimension(&[seg(l), seg(m)], DimMode::Smart));
        assert_eq!(a.doc.constraints.last().unwrap().kind, Kind::Angle);
        // Two circles: centre to centre.
        assert!(a.make_dimension(&[Elem::Round { v: d }, Elem::Round { v: e }], DimMode::Smart));
        assert!((a.doc.constraints.last().unwrap().value - 3.0).abs() < 1e-6);
        // A point and a line: the square gap between them.
        assert!(a.make_dimension(&[Elem::Round { v: e }, seg(m)], DimMode::Smart));
        assert_eq!(a.doc.constraints.last().unwrap().kind, Kind::LineDistance);
        assert!((a.doc.constraints.last().unwrap().value - 7.0).abs() < 1e-6);
    }

    #[test]
    fn typing_a_dimension_changes_the_drawing() {
        let mut a = app();
        let l = line(&mut a, [1.0, 2.0], [4.0, 2.0]);
        assert!(a.make_dimension(&[seg(l)], DimMode::Smart));
        assert!(a.set_dimension(0, 6.0));
        a.maintain_constraints();
        let p = pts(&a, l);
        assert!(((p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]) - 6.0).abs() < 1e-3);
        assert!(!a.set_dimension(0, -1.0), "negative sizes are refused");
        assert!(!a.set_dimension(0, 0.0));
    }

    #[test]
    fn across_and_gap_dimensions_hold_after_a_change() {
        let mut a = app();
        let (d1, d2) = (disc(&mut a, [2.0, 3.0], 0.2), disc(&mut a, [7.0, 5.0], 0.2));
        assert!(a.make_dimension(&[Elem::Centre { v: d1 }, Elem::Centre { v: d2 }], DimMode::Vertical));
        assert!(a.set_dimension(0, 4.0));
        a.maintain_constraints();
        assert!(((circle_of(&a, d1).0[1] - circle_of(&a, d2).0[1]).abs() - 4.0).abs() < 1e-3);
        let l = line(&mut a, [0.0, 0.0], [10.0, 0.0]);
        assert!(a.make_dimension(&[Elem::Centre { v: d1 }, seg(l)], DimMode::Smart));
        let i = a.doc.constraints.len() - 1;
        assert!(a.set_dimension(i, 2.5));
        a.maintain_constraints();
        let p = pts(&a, l);
        assert!((geom_line_gap(p[0], p[1], circle_of(&a, d1).0) - 2.5).abs() < 1e-3);
    }

    #[test]
    fn relation_buttons_only_fit_what_is_picked() {
        let (p, q) = (Elem::Centre { v: 1 }, Elem::Node { v: 2, path: 0, node: 0 });
        let (l, m) = (seg(3), seg(4));
        let (r, s) = (Elem::Round { v: 5 }, Elem::Round { v: 6 });
        assert!(Kind::Parallel.arrange(&[l, m]).is_some());
        assert!(Kind::Parallel.arrange(&[l, r]).is_none());
        assert!(Kind::Equal.arrange(&[r, s]).is_some());
        assert!(Kind::Equal.arrange(&[l, r]).is_none());
        assert_eq!(Kind::Midpoint.arrange(&[l, p]).unwrap(), vec![p, l], "point first, whatever the click order");
        assert_eq!(Kind::Tangent.arrange(&[r, l]).unwrap(), vec![l, r]);
        assert_eq!(Kind::Symmetric.arrange(&[l, p, q]).unwrap(), vec![p, q, l]);
        assert_eq!(Kind::Fixed.arrange(&[r]).unwrap(), vec![Elem::Centre { v: 5 }]);
        assert!(Kind::Coincident.arrange(&[p]).is_none());
    }

    #[test]
    fn definition_goes_from_under_to_fully_defined() {
        let mut a = app();
        let l = line(&mut a, [1.0, 1.0], [4.0, 1.0]);
        assert!(a.add_constraint(Kind::Horizontal, vec![seg(l)]));
        assert_eq!(definition(&a.doc).get(&l), Some(&false), "it can still slide");
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Node { v: l, path: 0, node: 0 }]));
        assert_eq!(definition(&a.doc).get(&l), Some(&false), "it can still stretch");
        assert!(a.make_dimension(&[seg(l)], DimMode::Smart));
        assert_eq!(definition(&a.doc).get(&l), Some(&true));
        a.maintain_constraints();
        assert_eq!(a.definition_text().unwrap().0, "Fully defined");

        let mut b = app();
        let d = disc(&mut b, [5.0, 5.0], 1.0);
        assert!(b.add_constraint(Kind::Fixed, vec![Elem::Centre { v: d }]));
        assert_eq!(definition(&b.doc).get(&d), Some(&false), "its size can change");
        assert!(b.make_dimension(&[Elem::Round { v: d }], DimMode::Smart));
        assert_eq!(definition(&b.doc).get(&d), Some(&true));
    }

    #[test]
    fn construction_shapes_are_never_cut() {
        let mut a = app();
        let keep = disc(&mut a, [3.0, 3.0], 1.0);
        let guide = disc(&mut a, [7.0, 7.0], 1.0);
        a.selected = vec![guide];
        a.set_construction(true);
        assert!(a.doc.vectors.iter().find(|v| v.id == guide).unwrap().construction);
        // Alone, it can't make a toolpath.
        a.start_op(crate::toolpath::OpKind::Profile);
        a.calculate_op();
        assert!(a.doc.toolpaths.is_empty(), "construction only: nothing to cut");
        a.selected = vec![keep, guide];
        a.start_op(crate::toolpath::OpKind::Profile);
        a.calculate_op();
        let far = a.doc.toolpaths[0].moves.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p[0]).fold(0.0, f64::max);
        assert!(far < 5.0, "only the normal circle is cut: {far}");
        a.set_construction(false);
        assert!(!a.doc.vectors.iter().find(|v| v.id == guide).unwrap().construction);
        // It still takes part in the rules.
        a.doc.vectors.iter_mut().for_each(|v| v.construction = v.id == guide);
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: keep }, Elem::Round { v: guide }]));
    }

    #[test]
    fn new_shapes_can_start_as_construction() {
        let mut a = app();
        a.doc.new_construction = true;
        let g = disc(&mut a, [5.0, 5.0], 1.0);
        assert!(a.doc.vectors.iter().find(|v| v.id == g).unwrap().construction);
    }

    #[test]
    fn empty_clicks_clear_picks_and_a_second_click_on_a_pick_drops_it() {
        let mut a = app();
        let d = disc(&mut a, [5.0, 5.0], 1.0);
        let f = XformForm::new(Xform::Constraints, &a.doc.job);
        a.sketch_click(&f, [5.5, 5.0], 0.1);
        assert_eq!(a.picks, vec![Elem::Round { v: d }]);
        a.sketch_click(&f, [5.5, 5.0], 0.1);
        assert!(a.picks.is_empty());
        a.sketch_click(&f, [5.5, 5.0], 0.1);
        a.sketch_click(&f, [1.0, 1.0], 0.1);
        assert!(a.picks.is_empty(), "empty sheet starts over");
        a.sketch_click(&f, [5.0, 5.01], 0.1);
        assert_eq!(a.picks, vec![Elem::Centre { v: d }], "a click on the middle means the centre");
    }


    /// A rectangle with its rules, crossed by a line, as in a real drawing.
    fn ruled_plate(a: &mut App) -> (u64, u64) {
        let plate = a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 6.0, 4.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 6.0, h: 4.0, corner: 0.0 });
        let cutter = line(a, [5.0, 0.5], [5.0, 9.5]);
        line(a, [0.5, 5.0], [9.5, 5.0]);
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Node { v: plate, path: 0, node: 0 }]));
        assert!(a.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 0 }], DimMode::Smart));
        assert!(a.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 1 }], DimMode::Smart));
        a.dim_edit = None;
        a.selected.clear();
        (plate, cutter)
    }

    fn all_points(a: &App, id: u64) -> Vec<P> {
        a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths.iter().flat_map(|p| p.pts.iter().copied()).collect()
    }

    fn on_plate_outline(p: P) -> bool {
        let on_x = (p[0] - 2.0).abs() < 1e-3 || (p[0] - 8.0).abs() < 1e-3;
        let on_y = (p[1] - 3.0).abs() < 1e-3 || (p[1] - 7.0).abs() < 1e-3;
        (on_x && (3.0 - 1e-3..=7.0 + 1e-3).contains(&p[1])) || (on_y && (2.0 - 1e-3..=8.0 + 1e-3).contains(&p[0]))
    }

    #[test]
    fn trimming_a_ruled_rectangle_leaves_the_rest_exactly_where_it_was() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        // Remove the bottom side's left half: its corner is fixed, and its length had a dimension.
        assert!(a.trim_at([3.0, 3.0], 0.1, false));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        for p in all_points(&a, plate) {
            assert!(on_plate_outline(p), "the drawing moved: {p:?}");
        }
    }

    #[test]
    fn trimming_each_side_in_turn_never_moves_what_is_left() {
        for at in [[6.5, 3.0], [8.0, 4.0], [6.5, 7.0], [3.0, 7.0], [2.0, 4.0]] {
            let mut a = app();
            let (plate, _) = ruled_plate(&mut a);
            assert!(a.trim_at(at, 0.1, false), "{at:?}");
            for _ in 0..3 {
                a.maintain_constraints();
            }
            assert!(!a.constraint_conflict, "{at:?}: {}", a.status);
            for p in all_points(&a, plate) {
                assert!(on_plate_outline(p), "trim at {at:?} moved a point to {p:?}");
            }
        }
    }

    #[test]
    fn rules_follow_their_edge_after_a_trim() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        assert!(a.trim_at([3.0, 7.0], 0.1, false));
        a.maintain_constraints();
        // The right side keeps its upright rule: moving its top corner must keep it upright.
        let (mut top, mut bottom) = (None, None);
        for (pi, path) in a.doc.vectors.iter().find(|v| v.id == plate).unwrap().paths.iter().enumerate() {
            for (ni, q) in path.pts.iter().enumerate() {
                if (q[0] - 8.0).abs() < 1e-6 && (q[1] - 7.0).abs() < 1e-6 {
                    top = Some((pi, ni));
                }
                if (q[0] - 8.0).abs() < 1e-6 && (q[1] - 3.0).abs() < 1e-6 {
                    bottom = Some((pi, ni));
                }
            }
        }
        let (tp, tn) = top.unwrap();
        let (bp, bn) = bottom.unwrap();
        a.doc.vectors.iter_mut().find(|v| v.id == plate).unwrap().paths[tp].pts[tn][0] = 9.0;
        a.maintain_constraints();
        let v = a.doc.vectors.iter().find(|v| v.id == plate).unwrap();
        assert!((v.paths[tp].pts[tn][0] - v.paths[bp].pts[bn][0]).abs() < 1e-3, "the side stayed upright");
    }

    #[test]
    fn a_rule_on_a_removed_edge_goes_away_and_says_so() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [3.0, 5.0]);
        let keep = line(&mut a, [4.0, 5.0], [9.0, 5.0]);
        assert!(a.add_constraint(Kind::Parallel, vec![seg(l), seg(keep)]));
        assert!(a.trim_at([2.0, 5.0], 0.1, true), "shift-click deletes the line");
        a.maintain_constraints();
        assert!(a.doc.constraints.is_empty());
        assert!(a.status.contains("rule"), "{}", a.status);
        assert_eq!(pts(&a, keep), vec![[4.0, 5.0], [9.0, 5.0]]);
    }

    #[test]
    fn a_cut_circle_keeps_its_circle_rules_without_moving() {
        let mut a = app();
        let d = disc(&mut a, [5.0, 5.0], 2.0);
        let e = disc(&mut a, [5.0, 5.0], 1.0);
        let cutter = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: d }, Elem::Round { v: e }]));
        assert!(a.make_dimension(&[Elem::Round { v: d }], DimMode::Smart));
        a.dim_edit = None;
        let before = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == e).unwrap().paths).unwrap();
        assert!(a.trim_at([5.0, 7.0], 0.1, false));
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.iter().filter(|c| matches!(c.kind, Kind::Concentric | Kind::Diameter)).count(), 2, "circle rules carry over to the arc: {:?}", a.doc.constraints);
        assert_eq!(geom::bbox(&a.doc.vectors.iter().find(|v| v.id == e).unwrap().paths).unwrap(), before);
        let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == d).unwrap().paths).unwrap();
        assert!((hi[1] - 5.0).abs() < 1e-3 && (lo[1] - 3.0).abs() < 1e-3 && (hi[0] - lo[0] - 4.0).abs() < 0.01, "half a circle: {lo:?} {hi:?}");
        let _ = cutter;
    }

    #[test]
    fn splitting_a_line_keeps_both_pieces_in_line_and_level() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        let _c1 = line(&mut a, [4.0, 1.0], [4.0, 9.0]);
        let _c2 = line(&mut a, [6.0, 1.0], [6.0, 9.0]);
        assert!(a.add_constraint(Kind::Horizontal, vec![seg(l)]));
        assert!(a.trim_at([5.0, 5.0], 0.1, false));
        a.maintain_constraints();
        let v = a.doc.vectors.iter().find(|v| v.id == l).unwrap();
        assert_eq!(v.paths.len(), 2);
        assert!(a.doc.constraints.iter().filter(|c| c.kind == Kind::Horizontal).count() >= 2, "{:?}", a.doc.constraints);
        assert!(a.doc.constraints.iter().any(|c| c.kind == Kind::Collinear), "{:?}", a.doc.constraints);
        // Lifting one piece lifts the other.
        a.doc.vectors.iter_mut().find(|v| v.id == l).unwrap().paths[0].pts[0][1] = 6.0;
        a.maintain_constraints();
        let v = a.doc.vectors.iter().find(|v| v.id == l).unwrap();
        let ys: Vec<f64> = v.paths.iter().flat_map(|p| p.pts.iter().map(|q| q[1])).collect();
        assert!(ys.iter().all(|y| (y - ys[0]).abs() < 1e-3), "{ys:?}");
    }

    #[test]
    fn a_dimension_on_a_trimmed_line_shows_its_new_length() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        let _c = line(&mut a, [6.0, 1.0], [6.0, 9.0]);
        assert!(a.make_dimension(&[seg(l)], DimMode::Smart));
        a.dim_edit = None;
        assert!(a.trim_at([8.0, 5.0], 0.1, false));
        a.maintain_constraints();
        assert_eq!(pts(&a, l), vec![[1.0, 5.0], [6.0, 5.0]], "stays where the cut left it");
        let d = a.doc.constraints.iter().find(|c| c.kind == Kind::Distance).unwrap();
        assert!((d.value - 5.0).abs() < 1e-6, "{}", d.value);
    }

    #[test]
    fn equal_on_a_shortened_line_is_dropped_rather_than_stretching_the_other() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        let m = line(&mut a, [1.0, 8.0], [9.0, 8.0]);
        let _c = line(&mut a, [6.0, 1.0], [6.0, 6.0]);
        assert!(a.add_constraint(Kind::Equal, vec![seg(l), seg(m)]));
        assert!(a.trim_at([8.0, 5.0], 0.1, false));
        a.maintain_constraints();
        assert!(a.doc.constraints.is_empty());
        assert_eq!(pts(&a, m), vec![[1.0, 8.0], [9.0, 8.0]]);
    }

    #[test]
    fn closing_the_trim_form_rejoins_without_breaking_rules() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        assert!(a.trim_at([3.0, 3.0], 0.1, false));
        let rules = a.doc.constraints.len();
        a.rejoin_all(0.01);
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert_eq!(a.doc.constraints.len(), rules);
        assert!(!a.constraint_conflict);
        for p in all_points(&a, plate) {
            assert!(on_plate_outline(p), "{p:?}");
        }
    }

    #[test]
    fn a_fillet_keeps_the_rules_on_the_other_edges() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        // Round the far corner, away from the fixed one.
        assert!(a.fillet_at([8.0, 7.0], 0.1, 0.5, crate::edit::FilletKind::Normal));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == plate).unwrap().paths).unwrap();
        assert!((lo[0] - 2.0).abs() < 1e-3 && (hi[0] - 8.0).abs() < 1e-3 && (lo[1] - 3.0).abs() < 1e-3 && (hi[1] - 7.0).abs() < 1e-3, "{lo:?} {hi:?}");
    }

    #[test]
    fn group_ungroup_and_join_keep_rules_on_the_same_edges() {
        let mut a = app();
        let l1 = line(&mut a, [1.0, 1.0], [3.0, 1.0]);
        let l2 = line(&mut a, [3.0, 1.0], [5.0, 4.0]);
        assert!(a.add_constraint(Kind::Horizontal, vec![seg(l1)]));
        a.selected = vec![l1, l2];
        a.group_selected();
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.len(), 1, "{}", a.status);
        a.ungroup_selected();
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.len(), 1, "{}", a.status);
        a.join_with(0.01);
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.len(), 1, "{}", a.status);
        let v = &a.doc.vectors[0];
        assert_eq!(v.paths[0].pts.len(), 3);
        assert!(v.paths[0].pts.iter().any(|p| p[0] == 5.0 && p[1] == 4.0), "the slanted part is untouched");
    }

    #[test]
    fn undo_after_a_trim_brings_shape_and_rules_back() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        let rules = a.doc.constraints.clone();
        let shape = all_points(&a, plate);
        assert!(a.trim_at([3.0, 3.0], 0.1, false));
        a.do_undo();
        a.maintain_constraints();
        assert_eq!(a.doc.constraints, rules);
        assert_eq!(all_points(&a, plate), shape);
    }


    /// Two dimensioned rectangles trimmed into one T-shaped outline, as in a real part.
    fn t_shape(a: &mut App) -> (u64, u64) {
        let rect_at = |a: &mut App, c: P, w: f64, h: f64| a.doc.add_vector(vec![Path::closed(rect(c, w, h, 0.0))], Prim::Rect { c, w, h, corner: 0.0 });
        let base = rect_at(a, [5.0, 3.0], 6.0, 2.0);
        let post = rect_at(a, [5.0, 5.5], 2.0, 5.0);
        assert!(a.add_constraint(Kind::Fixed, vec![Elem::Node { v: base, path: 0, node: 0 }]));
        assert!(a.make_dimension(&[Elem::Seg { v: base, path: 0, seg: 0 }], DimMode::Smart));
        assert!(a.make_dimension(&[Elem::Seg { v: base, path: 0, seg: 1 }], DimMode::Smart));
        assert!(a.make_dimension(&[Elem::Seg { v: post, path: 0, seg: 2 }], DimMode::Smart));
        assert!(a.make_dimension(&[Elem::Seg { v: post, path: 0, seg: 1 }], DimMode::Smart));
        a.dim_edit = None;
        // The base's top between the post's sides, then the post's bottom inside the base.
        assert!(a.trim_at([5.0, 4.0], 0.05, false));
        assert!(a.trim_at([5.0, 3.0], 0.05, false));
        a.rejoin_all(0.01);
        for _ in 0..3 {
            a.maintain_constraints();
        }
        (base, post)
    }

    fn dim_on(a: &App, kind: Kind, wanted: f64) -> usize {
        a.doc.constraints.iter().position(|c| c.kind == kind && (c.value - wanted).abs() < 1e-3).unwrap_or_else(|| panic!("no {kind:?} of {wanted}: {:?}", a.doc.constraints))
    }

    fn ends(a: &App, id: u64) -> (P, P) {
        let p = &a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths[0];
        (p.pts[0], *p.pts.last().unwrap())
    }

    fn joined(a: &App, x: u64, y: u64) -> bool {
        let (p, q) = ends(a, x);
        let (r, s) = ends(a, y);
        let near = |u: P, v: P| (u[0] - v[0]).hypot(u[1] - v[1]) < 1e-3;
        (near(p, r) && near(q, s)) || (near(p, s) && near(q, r))
    }

    fn square_corners(a: &App, id: u64) -> bool {
        a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths.iter().all(|p| p.segments().all(|(u, v)| (u[0] - v[0]).abs() < 1e-3 || (u[1] - v[1]).abs() < 1e-3))
    }

    #[test]
    fn after_trimming_the_outline_stays_closed_when_a_dimension_changes() {
        let mut a = app();
        let (base, post) = t_shape(&mut a);
        assert!(joined(&a, base, post), "starts joined");
        // Make the post wider: the base's cut ends must follow it.
        let i = dim_on(&a, Kind::Distance, 2.0);
        let post_width = a.doc.constraints.iter().position(|c| c.kind == Kind::Distance && (c.value - 2.0).abs() < 1e-3 && c.items.iter().any(|e| e.vector() == post)).unwrap_or(i);
        assert!(a.set_dimension(post_width, 3.0));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        assert!(joined(&a, base, post), "the outline opened: {:?} {:?}", ends(&a, base), ends(&a, post));
        assert!(square_corners(&a, base) && square_corners(&a, post));
        // And the base wider too.
        let i = dim_on(&a, Kind::Distance, 6.0);
        assert!(a.set_dimension(i, 8.0));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        assert!(joined(&a, base, post), "the outline opened: {:?} {:?}", ends(&a, base), ends(&a, post));
        assert!(square_corners(&a, base) && square_corners(&a, post));
        let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == base).unwrap().paths).unwrap();
        assert!((hi[0] - lo[0] - 8.0).abs() < 1e-3, "{lo:?} {hi:?}");
    }

    #[test]
    fn a_line_cut_by_another_stays_on_it_when_that_line_moves() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        let wall = line(&mut a, [6.0, 1.0], [6.0, 9.0]);
        assert!(a.add_constraint(Kind::Horizontal, vec![seg(l)]));
        assert!(a.add_constraint(Kind::Vertical, vec![seg(wall)]));
        assert!(a.trim_at([8.0, 5.0], 0.05, false));
        a.maintain_constraints();
        a.selected = vec![wall];
        a.translate_selected(1.5, 0.0);
        a.maintain_constraints();
        let (_, end) = ends(&a, l);
        assert!((end[0] - 7.5).abs() < 1e-3, "the cut end followed the wall: {end:?}");
    }

    #[test]
    fn trimming_without_any_rules_adds_none() {
        let mut a = app();
        line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        line(&mut a, [6.0, 1.0], [6.0, 9.0]);
        assert!(a.trim_at([8.0, 5.0], 0.05, false));
        assert!(a.doc.constraints.is_empty());
    }

    #[test]
    fn a_cut_circle_stays_a_true_arc_when_its_rules_change() {
        let mut a = app();
        let d = disc(&mut a, [5.0, 5.0], 2.0);
        line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        assert!(a.make_dimension(&[Elem::Round { v: d }], DimMode::Smart));
        a.dim_edit = None;
        assert!(a.trim_at([5.0, 7.0], 0.05, false));
        a.maintain_constraints();
        assert_eq!(a.doc.constraints.iter().filter(|c| c.kind == Kind::Diameter).count(), 1, "{:?}", a.doc.constraints);
        let i = dim_on(&a, Kind::Diameter, 4.0);
        assert!(a.set_dimension(i, 6.0));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        let v = a.doc.vectors.iter().find(|v| v.id == d).unwrap();
        let (c, r) = crate::constraints::arc_fit(v).expect("still an arc");
        assert!((r - 3.0).abs() < 1e-2, "{r}");
        for p in &v.paths[0].pts {
            assert!(((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() < 1e-3, "every point is on the arc: {p:?}");
        }
    }

    #[test]
    fn an_arc_can_take_new_rules_after_a_trim() {
        let mut a = app();
        let d = disc(&mut a, [5.0, 5.0], 2.0);
        let e = disc(&mut a, [9.0, 9.0], 0.5);
        let cutter = line(&mut a, [1.0, 5.0], [9.0, 5.0]);
        a.doc.constraints.clear();
        assert!(a.trim_at([5.0, 7.0], 0.05, false));
        let _ = cutter;
        assert_eq!(a.pick_element([5.0, 3.01], 0.05, Want::Circle), Some(Elem::Round { v: d }), "the arc still counts as round");
        assert!(a.add_constraint(Kind::Concentric, vec![Elem::Round { v: d }, Elem::Round { v: e }]));
        a.maintain_constraints();
        let v = a.doc.vectors.iter().find(|v| v.id == d).unwrap();
        let (c, _) = crate::constraints::arc_fit(v).unwrap();
        assert!((c[0] - circle_of(&a, e).0[0]).abs() < 1e-3);
    }

    #[test]
    fn a_fillet_keeps_its_shape_when_a_dimension_moves_its_edges() {
        let mut a = app();
        let (plate, _) = ruled_plate(&mut a);
        assert!(a.fillet_at([8.0, 7.0], 0.05, 0.5, crate::edit::FilletKind::Normal));
        a.maintain_constraints();
        let i = dim_on(&a, Kind::Distance, 6.0);
        assert!(a.set_dimension(i, 7.0));
        for _ in 0..3 {
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
        let pts = all_points(&a, plate);
        // Every point of the rounded corner stays half an inch from one centre.
        let arc: Vec<P> = pts.iter().copied().filter(|p| p[0] > 8.5 && p[1] > 6.5).collect();
        assert!(arc.len() >= 10, "{pts:?}");
        for p in arc {
            assert!(((p[0] - 8.5).hypot(p[1] - 6.5) - 0.5).abs() < 0.01, "kinked: {p:?}");
        }
    }

    #[test]
    fn changing_a_dimension_keeps_its_first_end_where_it_is() {
        let mut a = app();
        let l = line(&mut a, [1.0, 5.0], [4.0, 5.0]);
        assert!(a.make_dimension(&[seg(l)], DimMode::Smart));
        a.dim_edit = None;
        assert!(a.set_dimension(0, 6.0));
        a.maintain_constraints();
        assert_eq!(pts(&a, l)[0], [1.0, 5.0], "only the far end moves");
        assert!((pts(&a, l)[1][0] - 7.0).abs() < 1e-3);
    }


    #[test]
    fn new_rules_and_more_changes_keep_working_after_a_trim() {
        let mut a = app();
        let (base, post) = t_shape(&mut a);
        // Swap the post's right-side height for one on its left side, picked as the mouse would.
        let right = a.doc.constraints.iter().position(|c| c.kind == Kind::Distance && (c.value - 4.0).abs() < 1e-3 && c.items.iter().any(|e| e.vector_id() == post)).unwrap();
        a.doc.constraints.remove(right);
        let left = a.pick_element([4.0, 6.0], 0.05, Want::Line).expect("the side can still be picked");
        assert_eq!(left.vector_id(), post);
        assert!(a.make_dimension(&[left], DimMode::Smart));
        a.dim_edit = None;
        let i = a.doc.constraints.len() - 1;
        assert!((a.doc.constraints[i].value - 4.0).abs() < 1e-3, "{}", a.doc.constraints[i].value);
        for (value, width) in [(6.0, 2.5), (3.0, 1.5), (5.0, 3.0)] {
            assert!(a.set_dimension(i, value));
            a.maintain_constraints();
            let w = a.doc.constraints.iter().position(|c| c.kind == Kind::Distance && c.items.iter().any(|e| e.vector_id() == post) && c.items != [left]).unwrap();
            assert!(a.set_dimension(w, width));
            for _ in 0..3 {
                a.maintain_constraints();
            }
            assert!(!a.constraint_conflict, "{value} {width}: {}", a.status);
            assert!(joined(&a, base, post), "{value} {width}: {:?} {:?}", ends(&a, base), ends(&a, post));
            assert!(square_corners(&a, base) && square_corners(&a, post), "{value} {width}");
            let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == post).unwrap().paths).unwrap();
            assert!((hi[1] - lo[1] - value).abs() < 1e-3, "height {value}: {lo:?} {hi:?}");
            assert!((hi[0] - lo[0] - width).abs() < 1e-3, "width {width}: {lo:?} {hi:?}");
        }
        // Undo goes back through it all without leaving anything open.
        while a.undo.len() > 1 {
            a.do_undo();
            a.maintain_constraints();
        }
        assert!(!a.constraint_conflict, "{}", a.status);
    }

    #[test]
    fn joined_ends_do_not_keep_a_second_rule_for_the_same_thing() {
        let mut a = app();
        let (base, post) = t_shape(&mut a);
        let coincident = a.doc.constraints.iter().filter(|c| c.kind == Kind::Coincident).count();
        assert_eq!(coincident, 2, "{:?}", a.doc.constraints);
        let on_line = a.doc.constraints.iter().filter(|c| c.kind == Kind::PointOnLine && c.items[0].vector_id() == base && c.items[1].vector_id() == post).count();
        assert_eq!(on_line, 0, "{:?}", a.doc.constraints);
    }


    #[test]
    fn a_dimension_that_follows_from_the_others_is_refused_and_nothing_changes() {
        let mut a = app();
        let (_, post) = t_shape(&mut a);
        // The post's height is set by a dimension on its right side; its left side can't differ.
        let left = a.pick_element([4.0, 6.0], 0.05, Want::Line).unwrap();
        let (rules, shape) = (a.doc.constraints.len(), all_points(&a, post));
        assert!(!a.make_dimension(&[left], DimMode::Smart));
        assert!(a.status.contains("already make this true"), "{}", a.status);
        assert_eq!(a.doc.constraints.len(), rules);
        a.maintain_constraints();
        assert!(!a.constraint_conflict, "nothing turns red");
        assert_eq!(all_points(&a, post), shape);
    }

}
