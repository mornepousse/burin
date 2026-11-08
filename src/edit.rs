//! Shape editing behind the Drawing tab's forms: transforms, copies, fillets, trimming and more.

use crate::app::*;
use crate::geom::{self, Path, P};

/// A problem found by the vector validator, with where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IssueKind {
    ZeroLength,
    Overlap,
    Intersection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Issue {
    pub kind: IssueKind,
    pub at: P,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilletKind {
    Normal,
    DogBone,
    TBone,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DimKind {
    Length,
    Vertical,
    Horizontal,
}

/// Which way an edge alignment goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

pub fn anchor_point(lo: P, hi: P, anchor: Datum) -> P {
    match anchor {
        Datum::BottomLeft => lo,
        Datum::BottomRight => [hi[0], lo[1]],
        Datum::TopLeft => [lo[0], hi[1]],
        Datum::TopRight => hi,
        Datum::Centre => [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0],
    }
}

fn segment_cross(a: P, b: P, c: P, d: P) -> Option<f64> {
    let (rx, ry) = (b[0] - a[0], b[1] - a[1]);
    let (sx, sy) = (d[0] - c[0], d[1] - c[1]);
    let denom = rx * sy - ry * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = ((c[0] - a[0]) * sy - (c[1] - a[1]) * sx) / denom;
    let u = ((c[0] - a[0]) * ry - (c[1] - a[1]) * rx) / denom;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

fn lerp(a: P, b: P, t: f64) -> P {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn length(pts: &[P]) -> f64 {
    pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// The point and direction `s` along a polyline.
fn along(pts: &[P], s: f64) -> (P, P) {
    let mut left = s.max(0.0);
    for w in pts.windows(2) {
        let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if left <= d || std::ptr::eq(w.as_ptr(), pts[pts.len() - 2..].as_ptr()) {
            let t = if d > 0.0 { (left / d).min(1.0) } else { 0.0 };
            let k = d.max(1e-12);
            return (lerp(w[0], w[1], t), [(w[1][0] - w[0][0]) / k, (w[1][1] - w[0][1]) / k]);
        }
        left -= d;
    }
    (pts[0], [1.0, 0.0])
}

/// Shapes that move as one, with the box around them.
struct Block {
    ids: Vec<u64>,
    lo: P,
    hi: P,
}

impl App {
    /// What these shapes move as: each on its own, but a whole piece only once, by the box around
    /// all of it. Shapes in the piece `within`, or any with adjusting on, are arranged inside their
    /// piece and move alone.
    fn blocks(&self, items: &[Vector], within: Option<u64>) -> Vec<Block> {
        let mut done = Vec::new();
        let mut blocks = Vec::new();
        for v in items {
            let ids = match v.piece.filter(|p| !self.adjust_piece && Some(*p) != within) {
                Some(piece) if done.contains(&piece) => continue,
                Some(piece) => {
                    done.push(piece);
                    self.doc.vectors.iter().filter(|o| o.piece == Some(piece)).map(|o| o.id).collect()
                }
                None => vec![v.id],
            };
            let lines = self.doc.vectors.iter().filter(|o| ids.contains(&o.id)).flat_map(|o| o.paths.iter());
            if let Some((lo, hi)) = geom::bbox(lines) {
                blocks.push(Block { ids, lo, hi });
            }
        }
        blocks
    }

    fn selection_in_click_order(&self) -> Vec<Vector> {
        self.selected.iter().filter_map(|id| self.doc.vectors.iter().find(|v| v.id == *id).cloned()).collect()
    }

    fn copy_selection(&mut self, f: impl Fn(P) -> P, prim_kept: bool) -> Vec<u64> {
        let items = self.selection_in_click_order();
        let mut made = Vec::new();
        let mut pairs = Vec::new();
        for mut v in items {
            let original = v.id;
            v.id = self.doc.new_id();
            pairs.push((original, v.id));
            let before = v.paths.clone();
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                *p = f(*p);
            }
            if !prim_kept {
                // A copy that's still the same kind of shape stays one.
                v.prim = v.prim.following(&before, &v.paths);
            }
            made.push(v.id);
            self.doc.vectors.push(v);
        }
        self.doc.sort_out_copied_pieces(&made);
        self.doc.join_copies_to_toolpaths(&pairs);
        made
    }

    pub fn apply_xform(&mut self) {
        let LeftForm::Xform(f) = &self.left else { return };
        let f = f.clone();
        let Some((lo, hi)) = self.selected_bbox() else {
            self.status = crate::i18n::tr("Select one or more shapes first.").into();
            return;
        };
        let at = anchor_point(lo, hi, f.anchor);
        match f.kind {
            Xform::Move => {
                self.checkpoint();
                let (dx, dy) = if f.absolute { (f.dx - at[0], f.dy - at[1]) } else { (f.dx, f.dy) };
                self.translate_selected(dx, dy);
            }
            Xform::Scale if f.each => {
                self.checkpoint();
                let (sx, sy) = (f.sx / 100.0, if f.keep_ratio { f.sx } else { f.sy } / 100.0);
                for id in self.selected.clone() {
                    let Some((a, b)) = self.doc.vectors.iter().find(|v| v.id == id).and_then(|v| geom::bbox(&v.paths)) else { continue };
                    let own = anchor_point(a, b, f.anchor);
                    let keep = std::mem::replace(&mut self.selected, vec![id]);
                    self.map_selected(|p| [own[0] + (p[0] - own[0]) * sx, own[1] + (p[1] - own[1]) * sy]);
                    self.selected = keep;
                }
            }
            Xform::Rotate if f.each => {
                self.checkpoint();
                let (s, k) = f.angle.to_radians().sin_cos();
                let items = self.selection_in_click_order();
                for block in self.blocks(&items, None) {
                    let c = anchor_point(block.lo, block.hi, f.anchor);
                    let keep = std::mem::replace(&mut self.selected, block.ids);
                    self.map_selected(|p| {
                        let (x, y) = (p[0] - c[0], p[1] - c[1]);
                        [c[0] + x * k - y * s, c[1] + x * s + y * k]
                    });
                    self.selected = keep;
                }
            }
            Xform::Scale => {
                self.checkpoint();
                let (sx, sy) = (f.sx / 100.0, if f.keep_ratio { f.sx } else { f.sy } / 100.0);
                self.map_selected(|p| [at[0] + (p[0] - at[0]) * sx, at[1] + (p[1] - at[1]) * sy]);
            }
            Xform::Rotate => {
                self.checkpoint();
                let (s, k) = f.angle.to_radians().sin_cos();
                let c = f.pivot;
                self.map_moving(|p| {
                    let (x, y) = (p[0] - c[0], p[1] - c[1]);
                    [c[0] + x * k - y * s, c[1] + x * s + y * k]
                });
            }
            Xform::Offset => self.offset_selected(f.distance, f.outward),
            Xform::BlockCopy => self.array_copy(&f, lo, hi),
            Xform::CircularCopy => self.circular_copy(&f),
            Xform::CopyAlong => self.copy_along(&f),
            Xform::Join => self.join_with(f.distance),
            Xform::Nest => self.nest_selected(&f),
            Xform::TextOnCurve => self.text_on_curve(&f),
            _ => {}
        }
    }

    fn array_copy(&mut self, f: &XformForm, lo: P, hi: P) {
        self.checkpoint();
        let (w, h) = if f.use_offset { (f.dx, f.dy) } else { (hi[0] - lo[0] + f.dx, hi[1] - lo[1] + f.dy) };
        let mut made = Vec::new();
        for row in 0..f.rows.max(1) {
            for col in 0..f.cols.max(1) {
                if row == 0 && col == 0 {
                    continue;
                }
                let dx = col as f64 * w + row as f64 * f.shift[0];
                let dy = row as f64 * h + col as f64 * f.shift[1];
                made.extend(self.copy_selection(move |p| [p[0] + dx, p[1] + dy], false));
            }
        }
        self.finish_copies(made, f.group);
    }

    fn circular_copy(&mut self, f: &XformForm) {
        self.checkpoint();
        let c = [f.dx, f.dy];
        let step = if f.total_angle { f.angle / f.count.max(1) as f64 } else { f.angle };
        let (lo, hi) = self.selected_bbox().unwrap_or((c, c));
        let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let mut made = Vec::new();
        for i in 1..f.count.max(1) {
            // A positive angle goes clockwise.
            let (s, k) = (-step * i as f64).to_radians().sin_cos();
            let rotate = f.rotate_copies;
            made.extend(self.copy_selection(
                move |p| {
                    if rotate {
                        let (x, y) = (p[0] - c[0], p[1] - c[1]);
                        [c[0] + x * k - y * s, c[1] + x * s + y * k]
                    } else {
                        let (x, y) = (middle[0] - c[0], middle[1] - c[1]);
                        [p[0] + (c[0] + x * k - y * s - middle[0]), p[1] + (c[1] + x * s + y * k - middle[1])]
                    }
                },
                false,
            ));
        }
        self.finish_copies(made, f.group);
    }

    /// The originals stay selected with their copies, and go in the group with them.
    fn finish_copies(&mut self, made: Vec<u64>, group: bool) {
        self.selected.extend(made);
        if group && self.selected.len() > 1 {
            let chosen = self.selected.clone();
            self.keep_rules(|a| a.group_now(chosen));
        }
    }

    /// Copies the first selected object, or small circles, along the last selected shape.
    fn copy_along(&mut self, f: &XformForm) {
        let items = self.selection_in_click_order();
        let Some(rail) = items.last().and_then(|v| v.paths.first()).cloned() else { return };
        let others: Vec<Vector> = items[..items.len() - 1].to_vec();
        if !f.copy_circles && others.is_empty() {
            self.status = crate::i18n::tr("Select the object to copy first, then the shape to copy along.").into();
            return;
        }
        let mut route = rail.pts.clone();
        if rail.closed {
            route.push(route[0]);
        }
        if f.reverse {
            route.reverse();
        }
        let total = length(&route);
        let count = if f.by_number { f.count.max(1) } else { ((total / f.distance.max(1e-6)).floor() as usize).max(1) };
        let spacing = if f.by_number || f.even {
            total / if rail.closed { count as f64 } else { (count.max(2) - 1) as f64 }
        } else {
            f.distance.max(1e-6)
        };
        self.checkpoint();
        let layer = if f.new_layer {
            self.doc.layers.push(Layer::named("Copies"));
            self.doc.layers.len() - 1
        } else {
            self.doc.active_layer
        };
        let (blo, bhi) = geom::bbox(others.iter().flat_map(|v| v.paths.iter())).unwrap_or(([0.0, 0.0], [0.0, 0.0]));
        let origin = [(blo[0] + bhi[0]) / 2.0, (blo[1] + bhi[1]) / 2.0];
        let mut made = Vec::new();
        for i in 0..count {
            let (at, dir) = along(&route, spacing * i as f64);
            if f.copy_circles {
                let id = self.doc.new_id();
                let (c, r) = (at, f.size / 2.0);
                self.doc.vectors.push(Vector { id, paths: vec![Path::closed(geom::circle(c, r))], layer, prim: Prim::Circle { c, r }, construction: false, members: Vec::new(), piece: None, face: self.doc.active_face, sheet: self.doc.active_sheet });
                made.push(id);
                continue;
            }
            let (c, s) = if f.align { (dir[0], dir[1]) } else { (1.0, 0.0) };
            let mut now = Vec::new();
            let mut pairs = Vec::new();
            for v in &others {
                let mut copy = v.clone();
                copy.id = self.doc.new_id();
                pairs.push((v.id, copy.id));
                copy.layer = layer;
                copy.prim = Prim::Free;
                for p in copy.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                    let (x, y) = (p[0] - origin[0], p[1] - origin[1]);
                    *p = [at[0] + x * c - y * s, at[1] + x * s + y * c];
                }
                made.push(copy.id);
                now.push(copy.id);
                self.doc.vectors.push(copy);
            }
            self.doc.sort_out_copied_pieces(&now);
            self.doc.join_copies_to_toolpaths(&pairs);
        }
        self.selected = made;
    }

    /// Mirrors about the selection's own centre, the job's centre, or one of its edges.
    pub fn mirror(&mut self, horizontal: bool, edge: Option<Edge>, about_job: bool, copy: bool) {
        let Some((lo, hi)) = self.moving_bbox() else { return };
        self.checkpoint();
        let line = match edge {
            Some(Edge::Left) => lo[0],
            Some(Edge::Right) => hi[0],
            Some(Edge::Top) => hi[1],
            Some(Edge::Bottom) => lo[1],
            None if about_job && horizontal => self.doc.job.width / 2.0,
            None if about_job => self.doc.job.height / 2.0,
            None if horizontal => (lo[0] + hi[0]) / 2.0,
            None => (lo[1] + hi[1]) / 2.0,
        };
        let flip = move |p: P| if horizontal { [2.0 * line - p[0], p[1]] } else { [p[0], 2.0 * line - p[1]] };
        if copy {
            let made = self.copy_selection(flip, false);
            self.selected = made;
        } else {
            self.map_moving(flip);
        }
    }

    /// Turns the selection about the middle of its box. A positive angle goes anticlockwise.
    pub fn turn_selected(&mut self, degrees: f64) {
        let Some((lo, hi)) = self.moving_bbox() else { return };
        self.checkpoint();
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        let (s, k) = degrees.to_radians().sin_cos();
        self.map_moving(|p| {
            let (x, y) = (p[0] - c[0], p[1] - c[1]);
            [c[0] + x * k - y * s, c[1] + x * s + y * k]
        });
    }

    /// Lines up the other selected shapes with the last one selected. `axis` picks which directions.
    pub fn align_to_last_center(&mut self, x: bool, y: bool) {
        self.align_to_last_center_as(x, y, false);
    }

    /// What moves when lining up with the last selected: each shape by its own box, or all of them
    /// by the box around them, so they stay as they are to each other. A shape in the same piece as
    /// the last one is arranged inside that piece, so it moves alone.
    fn blocks_to_align(&self, items: &[Vector], together: bool) -> Vec<Block> {
        let within = items.last().and_then(|v| v.piece);
        let mut blocks = self.blocks(&items[..items.len() - 1], within);
        if together {
            let lo = blocks.iter().fold([f64::MAX; 2], |m, b| [m[0].min(b.lo[0]), m[1].min(b.lo[1])]);
            let hi = blocks.iter().fold([f64::MIN; 2], |m, b| [m[0].max(b.hi[0]), m[1].max(b.hi[1])]);
            for block in &mut blocks {
                (block.lo, block.hi) = (lo, hi);
            }
        }
        blocks
    }

    pub fn align_to_last_center_as(&mut self, x: bool, y: bool, together: bool) {
        let items = self.selection_in_click_order();
        let Some(target) = items.last().and_then(|v| geom::bbox(&v.paths)) else { return };
        if items.len() < 2 {
            self.status = crate::i18n::tr("Select the shapes to move, then the one to align to.").into();
            return;
        }
        self.checkpoint();
        let c = [(target.0[0] + target.1[0]) / 2.0, (target.0[1] + target.1[1]) / 2.0];
        for Block { ids, lo, hi } in self.blocks_to_align(&items, together) {
            let dx = if x { c[0] - (lo[0] + hi[0]) / 2.0 } else { 0.0 };
            let dy = if y { c[1] - (lo[1] + hi[1]) / 2.0 } else { 0.0 };
            self.translate_ids(&ids, dx, dy);
        }
    }

    /// Moves the others to an edge of the last selected shape, inside it or just outside it.
    pub fn align_to_last_edge(&mut self, edge: Edge, inside: bool) {
        self.align_to_last_edge_as(edge, inside, false);
    }

    pub fn align_to_last_edge_as(&mut self, edge: Edge, inside: bool, together: bool) {
        let items = self.selection_in_click_order();
        let Some((tlo, thi)) = items.last().and_then(|v| geom::bbox(&v.paths)) else { return };
        if items.len() < 2 {
            self.status = crate::i18n::tr("Select the shapes to move, then the one to align to.").into();
            return;
        }
        self.checkpoint();
        for Block { ids, lo, hi } in self.blocks_to_align(&items, together) {
            let (dx, dy) = match (edge, inside) {
                (Edge::Left, true) => (tlo[0] - lo[0], 0.0),
                (Edge::Left, false) => (tlo[0] - hi[0], 0.0),
                (Edge::Right, true) => (thi[0] - hi[0], 0.0),
                (Edge::Right, false) => (thi[0] - lo[0], 0.0),
                (Edge::Top, true) => (0.0, thi[1] - hi[1]),
                (Edge::Top, false) => (0.0, thi[1] - lo[1]),
                (Edge::Bottom, true) => (0.0, tlo[1] - lo[1]),
                (Edge::Bottom, false) => (0.0, tlo[1] - hi[1]),
            };
            self.translate_ids(&ids, dx, dy);
        }
    }

    /// Spaces the selected shapes evenly between the outermost two.
    pub fn space_selection(&mut self, horizontal: bool) {
        // Shapes all in one piece are arranged inside it.
        let within = self.doc.shared_piece(&self.selected);
        let items = self.selection_in_click_order();
        let mut blocks = self.blocks(&items, within);
        if blocks.len() < 3 {
            self.status = crate::i18n::tr("Select three or more shapes to space them.").into();
            return;
        }
        let axis = if horizontal { 0 } else { 1 };
        blocks.sort_by(|a, b| (a.lo[axis] + a.hi[axis]).total_cmp(&(b.lo[axis] + b.hi[axis])));
        let first = (blocks[0].lo[axis] + blocks[0].hi[axis]) / 2.0;
        let last = (blocks[blocks.len() - 1].lo[axis] + blocks[blocks.len() - 1].hi[axis]) / 2.0;
        let gap = (last - first) / (blocks.len() - 1) as f64;
        self.checkpoint();
        for (i, block) in blocks.iter().enumerate() {
            let shift = first + gap * i as f64 - (block.lo[axis] + block.hi[axis]) / 2.0;
            if horizontal {
                self.translate_ids(&block.ids, shift, 0.0);
            } else {
                self.translate_ids(&block.ids, 0.0, shift);
            }
        }
    }

    /// How many closed and open lines the selection has, and would have after joining.
    pub fn join_counts(&self, tolerance: f64) -> ([usize; 2], [usize; 2]) {
        let paths = self.selected_paths();
        let before = [paths.iter().filter(|p| p.closed).count(), paths.iter().filter(|p| !p.closed).count()];
        let joined = join_paths(paths, tolerance);
        let after = [joined.iter().filter(|p| p.closed).count(), joined.iter().filter(|p| !p.closed).count()];
        (before, after)
    }

    pub fn join_with(&mut self, tolerance: f64) {
        self.keep_rules(|a| a.join_inner(tolerance));
    }

    fn join_inner(&mut self, tolerance: f64) {
        let paths = self.selected_paths();
        if !paths.iter().any(|p| !p.closed) {
            self.status = crate::i18n::tr("Select open lines to join.").into();
            return;
        }
        self.checkpoint();
        let joined = join_paths(paths, tolerance);
        let sel = std::mem::take(&mut self.selected);
        let piece = self.doc.shared_piece(&sel);
        self.doc.vectors.retain(|v| !sel.contains(&v.id));
        for p in joined {
            let id = self.doc.add_vector(vec![p], Prim::Free);
            self.doc.put_in_piece(&[id], piece);
            self.selected.push(id);
        }
    }

    /// Rounds the sharp corner nearest to `at`, within `reach`.
    pub fn fillet_at(&mut self, at: P, reach: f64, radius: f64, kind: FilletKind) -> bool {
        let done = self.keep_rules(|a| a.fillet_inner(at, reach, radius, kind));
        if let Some((id, start, end)) = self.new_fillet.take() {
            self.hold_fillet(id, start, end);
        }
        done
    }

    fn fillet_inner(&mut self, at: P, reach: f64, radius: f64, kind: FilletKind) -> bool {
        let mut best: Option<(f64, usize, usize, usize)> = None;
        for (vi, v) in self.doc.vectors.iter().enumerate() {
            for (pi, path) in v.paths.iter().enumerate() {
                for (i, q) in path.pts.iter().enumerate() {
                    let d = (q[0] - at[0]).hypot(q[1] - at[1]);
                    if d <= reach && best.is_none_or(|b| d < b.0) {
                        best = Some((d, vi, pi, i));
                    }
                }
            }
        }
        let Some((_, vi, pi, i)) = best else {
            self.status = crate::i18n::tr("Click on a corner of a shape.").into();
            return false;
        };
        let path = self.doc.vectors[vi].paths[pi].clone();
        let n = path.pts.len();
        if !path.closed && (i == 0 || i == n - 1) || n < 3 {
            self.status = crate::i18n::tr("That point is the end of a line, not a corner.").into();
            return false;
        }
        let (a, c, b) = (path.pts[(i + n - 1) % n], path.pts[i], path.pts[(i + 1) % n]);
        let Some(replacement) = corner_points(a, c, b, radius, kind) else {
            self.status = crate::i18n::tr("That corner is too small or too flat for this radius.").into();
            return false;
        };
        self.checkpoint();
        if kind == FilletKind::Normal {
            self.new_fillet = Some((self.doc.vectors[vi].id, replacement[0], *replacement.last().unwrap()));
        }
        let v = &mut self.doc.vectors[vi];
        v.paths[pi].pts.splice(i..=i, replacement);
        v.prim = Prim::Free;
        true
    }

    /// Removes the stretch of line under `at` between its crossings with other lines.
    pub fn trim_at(&mut self, at: P, reach: f64, delete_whole: bool) -> bool {
        let done = self.keep_rules(|a| a.trim_inner(at, reach, delete_whole));
        self.attach_trim_ends();
        done
    }

    fn trim_inner(&mut self, at: P, reach: f64, delete_whole: bool) -> bool {
        let mut best: Option<(f64, usize, usize, usize)> = None;
        for (vi, v) in self.doc.vectors.iter().enumerate() {
            for (pi, path) in v.paths.iter().enumerate() {
                for (si, (a, b)) in path.segments().enumerate() {
                    let d = geom::dist2_seg(at, a, b);
                    if d <= reach * reach && best.is_none_or(|x| d < x.0) {
                        best = Some((d, vi, pi, si));
                    }
                }
            }
        }
        let Some((_, vi, pi, si)) = best else { return false };
        if delete_whole {
            self.checkpoint();
            self.doc.vectors[vi].paths.remove(pi);
            if self.doc.vectors[vi].paths.is_empty() {
                let id = self.doc.vectors.remove(vi).id;
                self.selected.retain(|s| *s != id);
            }
            return true;
        }
        let path = self.doc.vectors[vi].paths[pi].clone();
        // Every other line, with what a rule would call it.
        let mut others: Vec<(P, P, crate::constraints::Elem)> = Vec::new();
        for (ovi, v) in self.doc.vectors.iter().enumerate() {
            let whole = crate::constraints::round_shape(v).is_some();
            for (opi, p) in v.paths.iter().enumerate() {
                if ovi == vi && opi == pi {
                    continue;
                }
                for (osi, (c, d)) in p.segments().enumerate() {
                    let e = if whole { crate::constraints::Elem::Round { v: v.id } } else { crate::constraints::Elem::Seg { v: v.id, path: opi, seg: osi } };
                    others.push((c, d, e));
                }
            }
        }
        let segs: Vec<(P, P)> = path.segments().collect();
        // Position along the line of every crossing, as segment index plus fraction.
        let mut crossings: Vec<(f64, crate::constraints::Elem)> = Vec::new();
        for (i, (a, b)) in segs.iter().enumerate() {
            for (c, d, e) in &others {
                if let Some(t) = segment_cross(*a, *b, *c, *d) {
                    crossings.push((i as f64 + t, *e));
                }
            }
        }
        crossings.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut cuts: Vec<f64> = crossings.iter().map(|c| c.0).collect();
        cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        if cuts.is_empty() {
            self.status = crate::i18n::tr("That line doesn't cross anything. Hold Shift and click to delete it.").into();
            return false;
        }
        let (sa, sb) = segs[si];
        let along_seg = {
            let (dx, dy) = (sb[0] - sa[0], sb[1] - sa[1]);
            ((at[0] - sa[0]) * dx + (at[1] - sa[1]) * dy) / (dx * dx + dy * dy).max(1e-12)
        };
        let click = si as f64 + along_seg.clamp(0.0, 1.0);
        let total = segs.len() as f64;
        let point_at = |pos: f64| {
            let i = (pos.floor() as usize).min(segs.len() - 1);
            lerp(segs[i].0, segs[i].1, pos - i as f64)
        };
        let piece = |from: f64, to: f64| -> Vec<P> {
            let mut pts = vec![point_at(from)];
            let mut k = from.floor() + 1.0;
            while k < to {
                pts.push(point_at(k));
                k += 1.0;
            }
            pts.push(point_at(to));
            pts
        };
        let mut kept: Vec<Vec<P>> = Vec::new();
        let mut ends_made: Vec<f64> = Vec::new();
        if path.closed {
            // Keep everything except the stretch around the click.
            let before = cuts.iter().rev().find(|c| **c <= click).copied().unwrap_or(*cuts.last().unwrap() - total);
            let after = cuts.iter().find(|c| **c > click).copied().unwrap_or(cuts[0] + total);
            let (from, to) = (after, before + total);
            ends_made.push(before.rem_euclid(total));
            ends_made.push(after.rem_euclid(total));
            if to - from > 1e-9 {
                let mut pts = vec![point_at(from.rem_euclid(total))];
                let mut k = from.floor() + 1.0;
                while k < to {
                    pts.push(point_at(k.rem_euclid(total)));
                    k += 1.0;
                }
                pts.push(point_at(to.rem_euclid(total)));
                kept.push(pts);
            }
        } else {
            let before = cuts.iter().rev().find(|c| **c <= click).copied();
            let after = cuts.iter().find(|c| **c > click).copied();
            if let Some(b) = before {
                kept.push(piece(0.0, b));
                ends_made.push(b);
            }
            if let Some(a) = after {
                kept.push(piece(a, total));
                ends_made.push(a);
            }
        }
        let trimmed_id = self.doc.vectors[vi].id;
        self.trim_ends = ends_made
            .iter()
            .filter_map(|pos| {
                let hit = crossings.iter().find(|c| (c.0 - pos).abs() < 1e-9).or_else(|| crossings.iter().find(|c| (c.0.rem_euclid(total) - pos).abs() < 1e-9))?;
                (hit.1.vector_id() != trimmed_id).then(|| (trimmed_id, point_at(*pos), hit.1))
            })
            .collect();
        self.checkpoint();
        let v = &mut self.doc.vectors[vi];
        v.prim = Prim::Free;
        v.paths.remove(pi);
        for pts in kept {
            v.paths.push(Path::open(pts));
        }
        if v.paths.is_empty() {
            let id = self.doc.vectors.remove(vi).id;
            self.selected.retain(|s| *s != id);
        }
        true
    }

    /// Looks for repeated points, duplicated lines and crossings.
    pub fn validate(&mut self) {
        let scope: Vec<&Vector> = if self.selected.is_empty() {
            self.doc.vectors.iter().filter(|v| self.doc.shows(v)).collect()
        } else {
            self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).collect()
        };
        let tol = self.doc.job.width.max(self.doc.job.height) * 1e-5;
        let mut issues = Vec::new();
        let mut segments: Vec<(P, P, usize)> = Vec::new();
        let mut owner = 0;
        for v in &scope {
            for path in &v.paths {
                for (a, b) in path.segments() {
                    if (a[0] - b[0]).hypot(a[1] - b[1]) <= tol {
                        issues.push(Issue { kind: IssueKind::ZeroLength, at: a });
                    } else {
                        segments.push((a, b, owner));
                    }
                }
                owner += 1;
            }
        }
        let same = |p: P, q: P| (p[0] - q[0]).hypot(p[1] - q[1]) <= tol;
        const LIMIT: usize = 200;
        'outer: for i in 0..segments.len() {
            let (a, b, oa) = segments[i];
            for &(c, d, ob) in &segments[i + 1..] {
                if (same(a, c) && same(b, d)) || (same(a, d) && same(b, c)) {
                    issues.push(Issue { kind: IssueKind::Overlap, at: lerp(a, b, 0.5) });
                } else if same(a, c) || same(a, d) || same(b, c) || same(b, d) {
                    continue;
                } else if let Some(t) = segment_cross(a, b, c, d) {
                    let _ = (oa, ob);
                    issues.push(Issue { kind: IssueKind::Intersection, at: lerp(a, b, t) });
                }
                if issues.len() >= LIMIT {
                    break 'outer;
                }
            }
        }
        self.status = tf!("Found {} issue(s).", issues.len());
        self.issues = issues;
    }

    pub fn fix_zero_length(&mut self) {
        self.keep_rules(App::fix_zero_inner);
    }

    fn fix_zero_inner(&mut self) {
        let tol = self.doc.job.width.max(self.doc.job.height) * 1e-5;
        self.checkpoint();
        for v in &mut self.doc.vectors {
            for p in &mut v.paths {
                p.pts.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= tol);
                if p.closed && p.pts.len() > 1 {
                    let (f, l) = (p.pts[0], *p.pts.last().unwrap());
                    if (f[0] - l[0]).hypot(f[1] - l[1]) <= tol {
                        p.pts.pop();
                    }
                }
            }
        }
        self.issues.retain(|i| i.kind != IssueKind::ZeroLength);
    }

    /// Draws a measured dimension with its arrows and text as shapes on a "Dimensions" layer.
    pub fn add_dimension(&mut self, a: P, b: P, kind: DimKind, f: &XformForm) {
        let (p, q, value) = match kind {
            DimKind::Length => (a, b, (b[0] - a[0]).hypot(b[1] - a[1])),
            DimKind::Horizontal => (a, [b[0], a[1]], (b[0] - a[0]).abs()),
            DimKind::Vertical => (a, [a[0], b[1]], (b[1] - a[1]).abs()),
        };
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        if len < 1e-6 {
            return;
        }
        let dir = [(q[0] - p[0]) / len, (q[1] - p[1]) / len];
        let normal = [-dir[1], dir[0]];
        let off = f.distance;
        let (p1, q1) = ([p[0] + normal[0] * off, p[1] + normal[1] * off], [q[0] + normal[0] * off, q[1] + normal[1] * off]);
        let head = f.size * 0.6;
        let arrow = |at: P, d: P| {
            vec![
                [at[0] + d[0] * head + normal[0] * head * 0.35, at[1] + d[1] * head + normal[1] * head * 0.35],
                at,
                [at[0] + d[0] * head - normal[0] * head * 0.35, at[1] + d[1] * head - normal[1] * head * 0.35],
            ]
        };
        let mut paths = vec![
            Path::open(vec![p, p1]),
            Path::open(vec![q, q1]),
            Path::open(vec![p1, q1]),
            Path::open(arrow(p1, dir)),
            Path::open(arrow(q1, [-dir[0], -dir[1]])),
        ];
        let label = if f.text.is_empty() { format!("{value:.*}", f.decimals) } else { f.text.clone() };
        let mid = [(p1[0] + q1[0]) / 2.0 + normal[0] * f.size * 0.4, (p1[1] + q1[1]) / 2.0 + normal[1] * f.size * 0.4];
        let glyphs = geom::text_paths(&label, f.size, [0.0, 0.0]);
        let width = geom::bbox(&glyphs).map_or(0.0, |(_, hi)| hi[0]);
        let (c, s) = if dir[0] < -1e-9 { (-dir[0], -dir[1]) } else { (dir[0], dir[1]) };
        for mut g in glyphs {
            for t in &mut g.pts {
                let (x, y) = (t[0] - width / 2.0, t[1]);
                *t = [mid[0] + x * c - y * s, mid[1] + x * s + y * c];
            }
            paths.push(g);
        }
        self.checkpoint();
        let layer = match self.doc.layers.iter().position(|l| l.name == "Dimensions") {
            Some(i) => i,
            None => {
                self.doc.layers.push(Layer::named(crate::i18n::tr("Dimensions")));
                self.doc.layers.len() - 1
            }
        };
        let id = self.doc.new_id();
        self.doc.vectors.push(Vector { id, paths, layer, prim: Prim::Free, construction: false, members: Vec::new(), piece: None, face: self.doc.active_face, sheet: self.doc.active_sheet });
        self.selected = vec![id];
    }

    /// Packs the selected shapes on the sheet in rows, by their bounding boxes.
    /// Makes the extra copies the chosen parts ask for (their nesting copies), so they're laid
    /// out with the rest, and adds them to the selection. A piece is copied whole, as a new
    /// piece. The counts go back to one once the copies are made, so nesting again doesn't
    /// multiply them.
    fn make_nest_copies(&mut self, boundary: Option<u64>) {
        let items: Vec<Vector> = self.selection_in_click_order().into_iter().filter(|v| Some(v.id) != boundary).collect();
        for b in self.blocks(&items, None) {
            let copies = b.ids.iter().filter_map(|id| self.doc.nest_copies.get(id)).copied().max().unwrap_or(1);
            for id in &b.ids {
                self.doc.nest_copies.remove(id);
            }
            for _ in 1..copies {
                let originals: Vec<Vector> = self.doc.vectors.iter().filter(|v| b.ids.contains(&v.id)).cloned().collect();
                let piece = originals.iter().find_map(|v| v.piece).map(|_| self.doc.new_id());
                for mut v in originals {
                    v.id = self.doc.new_id();
                    if v.piece.is_some() {
                        v.piece = piece;
                    }
                    self.selected.push(v.id);
                    self.doc.vectors.push(v);
                }
            }
        }
    }

    /// Nests by true shape. The sheet (or the chosen outline) and each part are drawn on a grid of
    /// cells, about 200 across the sheet; a part's cells are those its material touches, grown by
    /// half the tool's gap, so two parts never come closer than the gap. Biggest parts first, each
    /// in every allowed turn (and mirrored, if asked) goes to the first free place from the chosen
    /// corner, in rows along the chosen direction; the turn that lands earliest wins. With part in
    /// part, holes stay free for smaller parts; without, a part is filled in. A part with no room
    /// left starts a new sheet (or, in an outline, stays where it was).
    fn nest_by_shape(&mut self, parts: Vec<Vec<u64>>, f: &XformForm, outline: Option<&Path>, border: f64, clear: f64) {
        let (jw, jh) = (self.doc.job.width, self.doc.job.height);
        let (area_lo, area_hi) = match outline.and_then(|o| geom::bbox(std::slice::from_ref(o))) {
            Some(b) => b,
            None => ([0.0, 0.0], [jw, jh]),
        };
        let span = (area_hi[0] - area_lo[0]).max(area_hi[1] - area_lo[1]);
        let cell = (span / NEST_SHAPE_CELLS).max(1e-4);
        let (nx, ny) = (((area_hi[0] - area_lo[0]) / cell).ceil() as usize + 1, ((area_hi[1] - area_lo[1]) / cell).ceil() as usize + 1);
        let words = nx.div_ceil(64);
        let reach = cell * std::f64::consts::FRAC_1_SQRT_2;
        let pad = clear / 2.0;
        let seg_dist = |p: P, a: P, b: P| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = dx * dx + dy * dy;
            let t = if l > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
            (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
        };
        // Cells that can't be used: outside the sheet's border, or outside the outline.
        let blank = |outline: Option<&Path>| -> Vec<Vec<u64>> {
            let mut rows = vec![vec![0u64; words]; ny];
            for (iy, row) in rows.iter_mut().enumerate() {
                for ix in 0..nx {
                    let c = [area_lo[0] + (ix as f64 + 0.5) * cell, area_lo[1] + (iy as f64 + 0.5) * cell];
                    let bad = match outline {
                        Some(o) => !geom::inside_even_odd(std::iter::once(o), c) || o.segments().any(|(a, b)| seg_dist(c, a, b) <= (border - pad).max(0.0) + reach),
                        None => c[0] < border + reach || c[1] < border + reach || c[0] > jw - border - reach || c[1] > jh - border - reach,
                    };
                    if bad {
                        row[ix / 64] |= 1 << (ix % 64);
                    }
                }
            }
            rows
        };
        let free = |rows: &[Vec<u64>], y: usize, a: usize, b: usize| -> bool {
            let mut i = a;
            while i < b {
                let (w, bit) = (i / 64, i % 64);
                let n = (64 - bit).min(b - i);
                let mask = if n == 64 { u64::MAX } else { ((1u64 << n) - 1) << bit };
                if rows[y][w] & mask != 0 {
                    return false;
                }
                i += n;
            }
            true
        };
        let fill = |rows: &mut [Vec<u64>], y: usize, a: usize, b: usize| {
            for i in a..b {
                rows[y][i / 64] |= 1 << (i % 64);
            }
        };
        // Each part: its closed outlines, area for the order.
        let mut items: Vec<(Vec<u64>, Vec<Path>, f64)> = parts
            .into_iter()
            .map(|ids| {
                let paths: Vec<Path> = self.doc.vectors.iter().filter(|v| ids.contains(&v.id)).flat_map(|v| v.paths.iter().filter(|p| p.closed && p.pts.len() > 2).cloned()).collect();
                let area = geom::bbox(&paths).map_or(0.0, |(l, h)| (h[0] - l[0]) * (h[1] - l[1]));
                (ids, paths, area)
            })
            .filter(|(_, paths, _)| !paths.is_empty())
            .collect();
        items.sort_by(|a, b| b.2.total_cmp(&a.2));
        let step = nest_value(f.nest_step);
        let step = if f.nest_turn && step > 0.0 { step } else { NEST_NO_TURN };
        let turns: Vec<f64> = (0..).map(|k| k as f64 * step).take_while(|a| *a < NEST_NO_TURN - 1e-9).collect();
        let flips: &[bool] = if f.nest_mirror { &[false, true] } else { &[false] };
        let (from_right, from_top) = match f.anchor {
            Datum::BottomRight => (true, false),
            Datum::TopLeft => (false, true),
            Datum::TopRight => (true, true),
            _ => (false, false),
        };
        let mut sheets: Vec<Vec<Vec<u64>>> = vec![blank(outline)];
        let mut first_new = None;
        let (mut placed, mut left) = (0, 0);
        for (ids, paths, _) in items {
            let (lo, hi) = geom::bbox(&paths).unwrap();
            let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            // Each way the part may lie: its cells as runs per row, and the transform.
            let mut ways = Vec::new();
            for &turn in &turns {
                for &flip in flips {
                    let (s, k) = turn.to_radians().sin_cos();
                    let map = move |p: P| {
                        let x = if flip { centre[0] - (p[0] - centre[0]) } else { p[0] };
                        let (dx, dy) = (x - centre[0], p[1] - centre[1]);
                        [centre[0] + dx * k - dy * s, centre[1] + dx * s + dy * k]
                    };
                    let shape: Vec<Path> = paths.iter().map(|p| Path { pts: p.pts.iter().map(|q| map(*q)).collect(), ..p.clone() }).collect();
                    let (sl, sh) = geom::bbox(&shape).unwrap();
                    let origin = [sl[0] - pad - cell, sl[1] - pad - cell];
                    let (pw, ph) = (((sh[0] - sl[0] + 2.0 * pad) / cell).ceil() as usize + 3, ((sh[1] - sl[1] + 2.0 * pad) / cell).ceil() as usize + 3);
                    let outer: Vec<&Path> = if f.part_in_part {
                        shape.iter().collect()
                    } else {
                        let size = |p: &Path| geom::bbox(std::slice::from_ref(p)).map_or(0.0, |(l, h)| (h[0] - l[0]) * (h[1] - l[1]));
                        let big = shape.iter().map(size).fold(0.0, f64::max);
                        shape.iter().filter(|p| size(p) >= big).collect()
                    };
                    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
                    for py in 0..ph {
                        let mut start = None;
                        for px in 0..=pw {
                            let on = px < pw && {
                                let c = [origin[0] + (px as f64 + 0.5) * cell, origin[1] + (py as f64 + 0.5) * cell];
                                geom::inside_even_odd(outer.iter().copied(), c) || shape.iter().any(|p| p.segments().any(|(a, b)| seg_dist(c, a, b) <= pad + reach))
                            };
                            match (on, start) {
                                (true, None) => start = Some(px),
                                (false, Some(a)) => {
                                    runs.push((py, a, px));
                                    start = None;
                                }
                                _ => {}
                            }
                        }
                    }
                    ways.push((turn, flip, origin, pw, ph, runs));
                }
            }
            // The first place for this part on a sheet, over every way it may lie.
            let find = |rows: &[Vec<u64>]| -> Option<(usize, usize, usize)> {
                let mut best: Option<(usize, usize, usize, usize)> = None;
                for (w, (_, _, _, pw, ph, runs)) in ways.iter().enumerate() {
                    if *pw > nx || *ph > ny {
                        continue;
                    }
                    let (mx, my) = (nx - pw, ny - ph);
                    let (outer_n, inner_n) = if f.along_x { (my, mx) } else { (mx, my) };
                    'scan: for o in 0..=outer_n {
                        if best.is_some_and(|b| b.0 < o) {
                            break;
                        }
                        for i in 0..=inner_n {
                            let (ox, oy) = if f.along_x { (i, o) } else { (o, i) };
                            let ox = if from_right { mx - ox } else { ox };
                            let oy = if from_top { my - oy } else { oy };
                            if runs.iter().all(|(y, a, b)| free(rows, oy + y, ox + a, ox + b)) {
                                if best.is_none_or(|b| (o, i) < (b.0, b.1)) {
                                    best = Some((o, i, w, ox * 100_000 + oy));
                                }
                                break 'scan;
                            }
                        }
                    }
                }
                best.map(|(_, _, w, packed)| (w, packed / 100_000, packed % 100_000))
            };
            let mut spot = None;
            for (si, rows) in sheets.iter().enumerate() {
                if let Some(found) = find(rows) {
                    spot = Some((si, found));
                    break;
                }
            }
            if spot.is_none() && outline.is_none() {
                let fresh = blank(None);
                if let Some(found) = find(&fresh) {
                    sheets.push(fresh);
                    spot = Some((sheets.len() - 1, found));
                }
            }
            let Some((si, (w, ox, oy))) = spot else {
                left += 1;
                continue;
            };
            let (turn, flip, origin, _, _, runs) = &ways[w];
            for (y, a, b) in runs {
                fill(&mut sheets[si], oy + y, ox + a, ox + b);
            }
            // Turn and mirror the part as chosen, then move it into place.
            let (s, k) = turn.to_radians().sin_cos();
            let flip = *flip;
            let keep = std::mem::replace(&mut self.selected, ids.clone());
            self.map_selected(|p| {
                let x = if flip { centre[0] - (p[0] - centre[0]) } else { p[0] };
                let (dx, dy) = (x - centre[0], p[1] - centre[1]);
                [centre[0] + dx * k - dy * s, centre[1] + dx * s + dy * k]
            });
            self.selected = keep;
            let to = [area_lo[0] + ox as f64 * cell, area_lo[1] + oy as f64 * cell];
            self.translate_ids(&ids, to[0] - origin[0], to[1] - origin[1]);
            if si > 0 {
                let base = *first_new.get_or_insert(self.doc.sheet_count());
                let target = (base + si - 1).min(u8::MAX as usize - 1);
                self.doc.extra_sheets = self.doc.extra_sheets.max(target as u8);
                for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
                    v.sheet = target as u8;
                }
                self.selected.retain(|id| !ids.contains(id));
            }
            placed += 1;
        }
        let used = sheets.len();
        self.status = if left > 0 {
            tf!("Nested {placed} parts by shape; {left} didn't fit.")
        } else if used > 1 {
            tf!("Nested {placed} parts by shape on {used} sheets.")
        } else {
            tf!("Nested {placed} parts by shape.")
        };
    }

    /// Fills a chosen outline instead of the sheet: biggest parts first, each at the first place
    /// from the chosen corner where its box, kept the border gap from the outline and the tool's
    /// gap from the parts already in, fits wholly inside. Places are tried at the nester's
    /// resolution, coarser only for a very big outline. Parts that don't fit stay where they were.
    fn nest_in_outline(&mut self, outline: &Path, mut sized: Vec<Block>, guests: &[(Vec<u64>, usize)], f: &XformForm, border: f64, clear: f64) {
        let Some((ol, oh)) = geom::bbox(std::slice::from_ref(outline)) else { return };
        let resolution = if self.doc.job.units == Units::Mm { NEST_RESOLUTION_MM } else { NEST_RESOLUTION_MM * INCH_PER_MM };
        let cells = ((oh[0] - ol[0]) / resolution) * ((oh[1] - ol[1]) / resolution);
        let step = resolution * (cells / 40_000.0).sqrt().max(1.0);
        let order: Vec<usize> = (0..sized.len()).collect();
        let mut by_area = order.clone();
        by_area.sort_by(|a, b| {
            let area = |x: &Block| (x.hi[0] - x.lo[0]) * (x.hi[1] - x.lo[1]);
            area(&sized[*b]).total_cmp(&area(&sized[*a]))
        });
        let (from_right, from_top) = match f.anchor {
            Datum::BottomRight => (true, false),
            Datum::TopLeft => (false, true),
            Datum::TopRight => (true, true),
            _ => (false, false),
        };
        let mut taken: Vec<(P, P)> = Vec::new();
        let (mut placed, mut left) = (0, 0);
        for i in by_area {
            let Block { ids, lo, hi } = std::mem::replace(&mut sized[i], Block { ids: Vec::new(), lo: [0.0; 2], hi: [0.0; 2] });
            let (bw, bh) = (hi[0] - lo[0], hi[1] - lo[1]);
            let (nx, ny) = (((oh[0] - ol[0] - bw) / step).floor().max(-1.0) as i64, ((oh[1] - ol[1] - bh) / step).floor().max(-1.0) as i64);
            let at = |ix: i64, iy: i64| {
                let x = if from_right { oh[0] - bw - ix as f64 * step } else { ol[0] + ix as f64 * step };
                let y = if from_top { oh[1] - bh - iy as f64 * step } else { ol[1] + iy as f64 * step };
                [x, y]
            };
            let fits = |p: P, taken: &[(P, P)]| {
                let (a, b) = (p, [p[0] + bw, p[1] + bh]);
                taken.iter().all(|(c, d)| b[0] + clear <= c[0] || a[0] >= d[0] + clear || b[1] + clear <= c[1] || a[1] >= d[1] + clear) && box_fits_in(outline, a, b, border)
            };
            let mut spot = None;
            // Rows along the nesting direction, from the chosen corner.
            'search: for outer in 0..=(if f.along_x { ny } else { nx }).max(-1) {
                for inner in 0..=(if f.along_x { nx } else { ny }).max(-1) {
                    let p = if f.along_x { at(inner, outer) } else { at(outer, inner) };
                    if fits(p, &taken) {
                        spot = Some(p);
                        break 'search;
                    }
                }
            }
            match spot {
                Some(p) => {
                    let (dx, dy) = (p[0] - lo[0], p[1] - lo[1]);
                    self.translate_ids(&ids, dx, dy);
                    for (gids, _) in guests.iter().filter(|(_, h)| *h == i) {
                        self.translate_ids(gids, dx, dy);
                        placed += 1;
                    }
                    taken.push((p, [p[0] + bw, p[1] + bh]));
                    placed += 1;
                }
                None => left += 1,
            }
        }
        self.status = if left > 0 {
            tf!("Nested {placed} parts in the outline; {left} didn't fit.")
        } else {
            tf!("Nested {placed} parts in the outline.")
        };
    }

    /// Turns each part, in steps of the form's angle, to the turn that makes its row shallowest
    /// while it still fits across the sheet (the smallest box when no turn fits).
    fn turn_parts_to_nest(&mut self, items: &[Vector], f: &XformForm) {
        let border = nest_value(f.border);
        let (w, h) = (self.doc.job.width - 2.0 * border, self.doc.job.height - 2.0 * border);
        let (primary, secondary) = if f.along_x { (w, h) } else { (h, w) };
        // As the original hands each part its step (`0x1403bf330`): the form's step when turning is
        // on and the step is above 0, otherwise a full turn (360, `0x14210c1d8`), which is no turning.
        let step = nest_value(f.nest_step);
        let step = if step > 0.0 { step } else { NEST_NO_TURN };
        // Turns half a turn apart give the same box, so the first half turn is enough.
        let turns: Vec<f64> = (0..).map(|k| k as f64 * step).take_while(|a| *a < (NEST_NO_TURN / 2.0).max(step) - 1e-9).collect();
        for Block { ids, lo, hi } in self.blocks(items, None) {
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let pts: Vec<P> = self.doc.vectors.iter().filter(|v| ids.contains(&v.id)).flat_map(|v| v.paths.iter().flat_map(|p| p.pts.iter().copied())).collect();
            let size = |a: f64| {
                let (s, k) = a.to_radians().sin_cos();
                let (mut l, mut g) = ([f64::MAX; 2], [f64::MIN; 2]);
                for p in &pts {
                    let (x, y) = (p[0] - c[0], p[1] - c[1]);
                    let q = [x * k - y * s, x * s + y * k];
                    l = [l[0].min(q[0]), l[1].min(q[1])];
                    g = [g[0].max(q[0]), g[1].max(q[1])];
                }
                let (bw, bh) = (g[0] - l[0], g[1] - l[1]);
                if f.along_x { (bw, bh) } else { (bh, bw) }
            };
            let fits = |a: &f64| {
                let (run, depth) = size(*a);
                run <= primary + 1e-9 && depth <= secondary + 1e-9
            };
            let best = turns.iter().copied().filter(fits).min_by(|a, b| size(*a).1.total_cmp(&size(*b).1).then(a.total_cmp(b))).or_else(|| {
                turns.iter().copied().min_by(|a, b| {
                    let (ra, da) = size(*a);
                    let (rb, db) = size(*b);
                    (ra * da).total_cmp(&(rb * db))
                })
            });
            let Some(a) = best.filter(|a| a.abs() > 1e-9) else { continue };
            let keep = std::mem::replace(&mut self.selected, ids.clone());
            let (s, k) = a.to_radians().sin_cos();
            self.map_selected(|p| {
                let (x, y) = (p[0] - c[0], p[1] - c[1]);
                [c[0] + x * k - y * s, c[1] + x * s + y * k]
            });
            self.selected = keep;
        }
    }

    /// Lays the selected parts out in rows from the chosen corner. Parts that don't fit go onto
    /// new sheets after the others, laid out the same way, as the original's nesting does: it
    /// offers its nester a sheet up to 999 times (`0x1403be110`, `0x14090dc10`), then moves each part
    /// to the sheet it landed on (`nestComponentSheet`) and makes the sheets that are needed.
    fn nest_selected(&mut self, f: &XformForm) {
        let items = self.selection_in_click_order();
        if items.is_empty() {
            return;
        }
        // With "Last vector is nest boundary", the last shape chosen is the outline to fill.
        // Only shapes on the first sheet are nested, as in the original ("Invalid Sheet For
        // Nesting", `0x1403be110`, string 710).
        if self.doc.active_sheet != 0 {
            self.status = crate::i18n::tr("Only shapes on the first sheet can be nested.").into();
            return;
        }
        // A boundary with an open line in it is refused (string 1086).
        if f.into_boundary && items.last().is_some_and(|v| v.paths.iter().any(|p| !p.closed)) {
            self.status = crate::i18n::tr("Invalid boundary for nesting: the boundary contains open lines.").into();
            return;
        }
        let boundary = if f.into_boundary {
            match items.last().and_then(|v| v.paths.iter().find(|p| p.closed).cloned().map(|p| (v.id, p))) {
                Some(b) if items.len() > 1 => Some(b),
                _ => {
                    self.status = crate::i18n::tr("Select the parts, then the closed outline to nest them in, last.").into();
                    return;
                }
            }
        } else {
            None
        };
        self.checkpoint();
        self.make_nest_copies(boundary.as_ref().map(|b| b.0));
        let keep_selection = self.selected.clone();
        if let Some((bid, _)) = &boundary {
            self.selected.retain(|id| id != bid);
        }
        let items = self.selection_in_click_order();
        if f.nest_turn && !f.by_shape {
            self.turn_parts_to_nest(&items, f);
        }
        let items = self.selection_in_click_order();
        self.selected = keep_selection;
        let nv = nest_value;
        let (border, clear) = (nv(f.border), nv(f.tool_dia) + nv(f.clearance));
        let (w, h) = (self.doc.job.width - 2.0 * border, self.doc.job.height - 2.0 * border);
        let (primary, secondary) = if f.along_x { (w, h) } else { (h, w) };
        let (mut u, mut v, mut row) = (0.0f64, 0.0f64, 0.0f64);
        let mut sheet = 0usize;
        let mut placed = 0;
        let mut landed: Vec<(Vec<u64>, usize)> = Vec::new();
        let mut sized = self.blocks(&items, None);
        // Only closed outlines make parts, as the original's (`0x14090b960`: a contour that is
        // closed and not empty); open lines stay where they are.
        sized.retain(|b| self.doc.vectors.iter().filter(|v| b.ids.contains(&v.id)).any(|v| v.paths.iter().any(|p| p.closed && p.pts.len() > 2)));
        if sized.is_empty() {
            self.status = crate::i18n::tr("No suitable parts to nest are selected").into();
            return;
        }
        if f.by_shape {
            let parts: Vec<Vec<u64>> = sized.into_iter().map(|b| b.ids).collect();
            self.nest_by_shape(parts, f, boundary.as_ref().map(|b| &b.1), border, clear);
            return;
        }
        sized.sort_by(|a, b| {
            let height = |x: &Block| if f.along_x { x.hi[1] - x.lo[1] } else { x.hi[0] - x.lo[0] };
            height(b).total_cmp(&height(a))
        });
        // Part in part: smaller parts go into the holes of bigger ones first, where they stay as
        // their host moves (the original's `PartInPart`).
        let mut guests: Vec<(Vec<u64>, usize)> = Vec::new();
        if f.part_in_part {
            sized.sort_by(|a, b| {
                let area = |x: &Block| (x.hi[0] - x.lo[0]) * (x.hi[1] - x.lo[1]);
                area(b).total_cmp(&area(a))
            });
            let mut taken: Vec<Vec<(P, P)>> = Vec::new();
            let holes_of = |ids: &[u64]| -> Vec<Path> {
                let paths: Vec<Path> = self.doc.vectors.iter().filter(|v| ids.contains(&v.id)).flat_map(|v| v.paths.iter().filter(|p| p.closed).cloned()).collect();
                let size = |p: &Path| geom::bbox(std::slice::from_ref(p)).map_or(0.0, |(l, h)| (h[0] - l[0]) * (h[1] - l[1]));
                let Some(outer) = paths.iter().map(size).fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s)))) else { return Vec::new() };
                paths.into_iter().filter(|p| size(p) < outer).collect()
            };
            let hole_lists: Vec<Vec<Path>> = sized.iter().map(|b| holes_of(&b.ids)).collect();
            for _ in 0..sized.len() {
                taken.push(Vec::new());
            }
            let mut is_guest = vec![false; sized.len()];
            for g in 1..sized.len() {
                let (glo, ghi) = (sized[g].lo, sized[g].hi);
                let (gw, gh) = (ghi[0] - glo[0], ghi[1] - glo[1]);
                // The original's nester works at a resolution of 0.5 mm (`Nesting/ResolutionMM`,
                // `0x14090b910`); positions in a hole are tried that far apart, coarser only for a
                // very big hole.
                let resolution = if self.doc.job.units == Units::Mm { NEST_RESOLUTION_MM } else { NEST_RESOLUTION_MM * INCH_PER_MM };
                let step_for = |hole: &Path| {
                    let (hl, hh) = geom::bbox(std::slice::from_ref(hole)).unwrap_or(([0.0; 2], [1.0; 2]));
                    let cells = ((hh[0] - hl[0]) / resolution) * ((hh[1] - hl[1]) / resolution);
                    resolution * (cells / 40_000.0).sqrt().max(1.0)
                };
                'hosts: for h in 0..g {
                    if is_guest[h] {
                        continue;
                    }
                    for hole in &hole_lists[h] {
                        let Some((hl, hh)) = geom::bbox(std::slice::from_ref(hole)) else { continue };
                        let step = step_for(hole);
                        let mut y = hl[1] + clear;
                        while y + gh + clear <= hh[1] + 1e-9 {
                            let mut x = hl[0] + clear;
                            while x + gw + clear <= hh[0] + 1e-9 {
                                let (lo, hi) = ([x, y], [x + gw, y + gh]);
                                let free = taken[h].iter().all(|(a, b)| hi[0] + clear <= a[0] || lo[0] >= b[0] + clear || hi[1] + clear <= a[1] || lo[1] >= b[1] + clear);
                                if free && box_fits_in(hole, lo, hi, clear) {
                                    let ids = sized[g].ids.clone();
                                    self.translate_ids(&ids, x - glo[0], y - glo[1]);
                                    taken[h].push((lo, hi));
                                    is_guest[g] = true;
                                    guests.push((ids, h));
                                    break 'hosts;
                                }
                                x += step;
                            }
                            y += step;
                        }
                    }
                }
            }
            // The hosts keep their place in the order; guests leave it.
            let host_ids: Vec<Vec<u64>> = sized.iter().map(|b| b.ids.clone()).collect();
            let mut kept = Vec::new();
            for (i, b) in sized.into_iter().enumerate() {
                if !is_guest[i] {
                    kept.push(b);
                }
            }
            sized = kept;
            sized.sort_by(|a, b| {
                let height = |x: &Block| if f.along_x { x.hi[1] - x.lo[1] } else { x.hi[0] - x.lo[0] };
                height(b).total_cmp(&height(a))
            });
            // Guests named by their host's shapes, to follow it.
            for (_, h) in guests.iter_mut() {
                let host = &host_ids[*h];
                *h = sized.iter().position(|b| b.ids == *host).unwrap_or(usize::MAX);
            }
        }
        if let Some((_, outline)) = &boundary {
            self.nest_in_outline(outline, sized, &guests, f, border, clear);
            return;
        }
        for (bi, Block { ids, lo, hi }) in sized.into_iter().enumerate() {
            let (bw, bh) = (hi[0] - lo[0], hi[1] - lo[1]);
            let (run, depth) = if f.along_x { (bw, bh) } else { (bh, bw) };
            if u > 0.0 && u + run > primary {
                u = 0.0;
                v += row + clear;
                row = 0.0;
            }
            // No room left on this sheet: start the next one.
            if v > 0.0 && v + depth > secondary {
                sheet += 1;
                u = 0.0;
                v = 0.0;
                row = 0.0;
            }
            let (x, y) = if f.along_x { (u, v) } else { (v, u) };
            // Coordinates from the chosen corner of the sheet.
            let (tx, ty) = match f.anchor {
                Datum::BottomRight => (self.doc.job.width - border - x - bw, border + y),
                Datum::TopLeft => (border + x, self.doc.job.height - border - y - bh),
                Datum::TopRight => (self.doc.job.width - border - x - bw, self.doc.job.height - border - y - bh),
                _ => (border + x, border + y),
            };
            self.translate_ids(&ids, tx - lo[0], ty - lo[1]);
            if run <= primary && depth <= secondary {
                placed += 1;
            }
            // The parts in its holes come along, onto its sheet.
            for (gids, _) in guests.iter().filter(|(_, h)| *h == bi) {
                self.translate_ids(gids, tx - lo[0], ty - lo[1]);
                placed += 1;
                landed.push((gids.clone(), sheet));
            }
            landed.push((ids, sheet));
            u += run + clear;
            row = row.max(depth);
        }
        // The first sheet is the one showing; the others are added after the job's last sheet.
        let first_new = self.doc.sheet_count();
        let sheets = sheet + 1;
        if sheets > 1 {
            let wanted = (first_new + sheets - 1).min(u8::MAX as usize);
            self.doc.extra_sheets = (wanted - 1) as u8;
            for (ids, s) in &landed {
                if *s > 0 {
                    let target = (first_new + s - 1).min(u8::MAX as usize - 1) as u8;
                    for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
                        v.sheet = target;
                    }
                }
            }
            let moved: Vec<u64> = landed.iter().filter(|(_, s)| *s > 0).flat_map(|(ids, _)| ids.iter().copied()).collect();
            self.selected.retain(|id| !moved.contains(id));
        }
        let over = v + row > secondary;
        self.status = if sheets > 1 {
            tf!("Nested {placed} parts on {sheets} sheets.")
        } else if over {
            tf!("Nested {placed} parts, but they run past the sheet.")
        } else {
            tf!("Nested {placed} parts.")
        };
    }

    /// Sets a selected text along the last selected shape, letter by letter.
    fn text_on_curve(&mut self, f: &XformForm) {
        let items = self.selection_in_click_order();
        let text = items.iter().find_map(|v| match &v.prim {
            Prim::Text { text, height, style, .. } => Some((v.id, text.clone(), *height, crate::fonts::TextStyle { align: crate::fonts::TextAlign::Left, ..style.clone() })),
            _ => None,
        });
        let curve = items.iter().rev().find(|v| !matches!(v.prim, Prim::Text { .. })).and_then(|v| v.paths.first().cloned());
        let (Some((text_id, text, height, style)), Some(curve)) = (text, curve) else {
            self.status = crate::i18n::tr("Select a line of text and the curve to wrap it onto.").into();
            return;
        };
        let mut route = curve.pts.clone();
        if curve.closed {
            route.push(route[0]);
        }
        let total = length(&route);
        let widths: Vec<f64> = text
            .chars()
            .map(|c| {
                let probe = crate::fonts::text_paths(&format!("{c}|"), height, [0.0, 0.0], &style);
                let bar = crate::fonts::text_paths("|", height, [0.0, 0.0], &style);
                let w = geom::bbox(&probe).map_or(height * 0.3, |(_, hi)| hi[0]) - geom::bbox(&bar).map_or(0.0, |(_, hi)| hi[0]);
                w.max(height * 0.25)
            })
            .collect();
        let used: f64 = widths.iter().sum::<f64>() * f.spacing;
        let mut s = match f.anchor {
            Datum::BottomLeft | Datum::TopLeft => 0.0,
            Datum::BottomRight | Datum::TopRight => (total - used).max(0.0),
            Datum::Centre => ((total - used) / 2.0).max(0.0),
        };
        self.checkpoint();
        let mut paths = Vec::new();
        for (c, w) in text.chars().zip(&widths) {
            let advance = w * f.spacing;
            let (at, dir) = along(&route, s + advance / 2.0);
            let normal = [-dir[1], dir[0]];
            let origin = [at[0] + normal[0] * f.distance, at[1] + normal[1] * f.distance];
            let (k, si) = if f.align { (dir[0], dir[1]) } else { (1.0, 0.0) };
            for mut g in crate::fonts::text_paths(&c.to_string(), height, [-w / 2.0, 0.0], &style) {
                for p in &mut g.pts {
                    let (x, y) = (p[0], p[1]);
                    *p = [origin[0] + x * k - y * si, origin[1] + x * si + y * k];
                }
                paths.push(g);
            }
            s += advance;
        }
        if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == text_id) {
            v.paths = paths;
            v.prim = Prim::Free;
        }
    }
}

/// Whether a box, grown by `clear` all round, lies wholly inside a hole: its corners are in, none
/// of the hole's corners is in it, and no side of the hole crosses it.
fn box_fits_in(hole: &Path, lo: P, hi: P, clear: f64) -> bool {
    // A hair inside, so a box exactly touching the hole's side still counts as in.
    let e = 1e-7;
    let (a, b) = ([lo[0] - clear + e, lo[1] - clear + e], [hi[0] + clear - e, hi[1] + clear - e]);
    let corners = [a, [b[0], a[1]], b, [a[0], b[1]]];
    if !corners.iter().all(|c| geom::inside_even_odd(std::iter::once(hole), *c)) {
        return false;
    }
    if hole.pts.iter().any(|p| p[0] > a[0] && p[0] < b[0] && p[1] > a[1] && p[1] < b[1]) {
        return false;
    }
    let cross = |p: P, q: P, r: P, s: P| {
        let d = |a: P, b: P, c: P| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        let (d1, d2, d3, d4) = (d(r, s, p), d(r, s, q), d(p, q, r), d(p, q, s));
        (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
    };
    let sides = [(corners[0], corners[1]), (corners[1], corners[2]), (corners[2], corners[3]), (corners[3], corners[0])];
    !hole.segments().any(|(p, q)| sides.iter().any(|(r, t)| cross(p, q, *r, *t)))
}

/// The original nester's resolution at first, in millimetres (`ResolutionMM`, `0x14090b910`,
/// default `0x1420e8d28`), and its inches per millimetre (`0x1420e8d08`).
pub const NEST_RESOLUTION_MM: f64 = 0.5;
pub const INCH_PER_MM: f64 = 0.03937007874;

/// A nesting setting as the original's form reads it back (`0x1403bd2c0`): anything at or below 0
/// becomes 0.
pub fn nest_value(v: f64) -> f64 {
    if v > 0.0 { v } else { 0.0 }
}

/// How far nesting turns a part at a time when it may turn: a quarter turn, the step each part
/// the original hands its nester starts with (`0x14090b960`, 90.0 at `+0x30`).
pub const NEST_STEP: f64 = 90.0;
/// How many grid cells nesting by shape lays across the sheet's longer side (Burin's choice, so
/// a nest takes a moment rather than minutes).
pub const NEST_SHAPE_CELLS: f64 = 200.0;

/// The step a part gets when it may not turn: a full turn (`0x14210c1d8`).
pub const NEST_NO_TURN: f64 = 360.0;

/// Connects open lines whose ends touch within `tolerance`.
pub fn join_paths(paths: Vec<Path>, tolerance: f64) -> Vec<Path> {
    let near = |a: P, b: P| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
    let mut keep = Vec::new();
    let mut open: Vec<Vec<P>> = Vec::new();
    for p in paths {
        if p.closed {
            keep.push(p);
        } else if p.pts.len() >= 2 {
            open.push(p.pts);
        }
    }
    while let Some(mut line) = open.pop() {
        loop {
            let end = *line.last().unwrap();
            let start = line[0];
            if let Some(i) = open.iter().position(|o| near(end, o[0]) || near(end, *o.last().unwrap())) {
                let mut o = open.swap_remove(i);
                if !near(end, o[0]) {
                    o.reverse();
                }
                line.extend(o.into_iter().skip(1));
            } else if let Some(i) = open.iter().position(|o| near(start, o[0]) || near(start, *o.last().unwrap())) {
                let mut o = open.swap_remove(i);
                if !near(start, *o.last().unwrap()) {
                    o.reverse();
                }
                o.pop();
                o.extend(line);
                line = o;
            } else {
                break;
            }
        }
        if line.len() > 2 && near(line[0], *line.last().unwrap()) {
            line.pop();
            keep.push(Path::closed(line));
        } else {
            keep.push(Path::open(line));
        }
    }
    keep
}

/// Replaces corner `c` between `a` and `b`, or `None` when the corner can't take this radius.
fn corner_points(a: P, c: P, b: P, r: f64, kind: FilletKind) -> Option<Vec<P>> {
    let unit = |p: P, q: P| {
        let l = (q[0] - p[0]).hypot(q[1] - p[1]);
        (l > 1e-12).then(|| [(q[0] - p[0]) / l, (q[1] - p[1]) / l])
    };
    let (u, v) = (unit(c, a)?, unit(c, b)?);
    let cos = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0);
    let half = cos.acos() / 2.0;
    if half > 1.54 || half < 0.02 || r <= 0.0 {
        return None;
    }
    let (la, lb) = ((a[0] - c[0]).hypot(a[1] - c[1]), (b[0] - c[0]).hypot(b[1] - c[1]));
    let arc = |centre: P, from: P, to: P, sweep_long: bool| -> Vec<P> {
        let a0 = (from[1] - centre[1]).atan2(from[0] - centre[0]);
        let a1 = (to[1] - centre[1]).atan2(to[0] - centre[0]);
        let mut d = (a1 - a0).rem_euclid(std::f64::consts::TAU);
        let short = d <= std::f64::consts::PI;
        if short == sweep_long {
            d -= std::f64::consts::TAU;
        }
        (0..=24).map(|i| {
            let t = a0 + d * i as f64 / 24.0;
            [centre[0] + r * t.cos(), centre[1] + r * t.sin()]
        }).collect()
    };
    match kind {
        FilletKind::Normal => {
            let reach = r / half.tan();
            if reach >= la || reach >= lb {
                return None;
            }
            let bis = unit([0.0, 0.0], [u[0] + v[0], u[1] + v[1]])?;
            let centre = [c[0] + bis[0] * r / half.sin(), c[1] + bis[1] * r / half.sin()];
            let (from, to) = ([c[0] + u[0] * reach, c[1] + u[1] * reach], [c[0] + v[0] * reach, c[1] + v[1] * reach]);
            Some(arc(centre, from, to, false))
        }
        FilletKind::DogBone | FilletKind::TBone => {
            // A round notch past the corner, so a square part seats fully.
            let dir = if kind == FilletKind::TBone {
                [-u[0], -u[1]]
            } else {
                unit([0.0, 0.0], [-(u[0] + v[0]), -(u[1] + v[1])])?
            };
            let centre = [c[0] + dir[0] * r, c[1] + dir[1] * r];
            let chord = |w: P| {
                let t = 2.0 * r * (dir[0] * w[0] + dir[1] * w[1]);
                [c[0] + w[0] * t, c[1] + w[1] * t]
            };
            let reach_on = |w: P| (2.0 * r * (dir[0] * w[0] + dir[1] * w[1])).abs();
            let (from, to) = if kind == FilletKind::TBone { (c, chord(v)) } else { (c, c) };
            if reach_on(v) >= lb {
                return None;
            }
            let mut pts = vec![c];
            let start = if kind == FilletKind::TBone { from } else { c };
            let end = if kind == FilletKind::TBone { to } else { c };
            if kind == FilletKind::DogBone {
                pts.extend((1..24).map(|i| {
                    let t = std::f64::consts::TAU * i as f64 / 24.0;
                    let (s, k) = t.sin_cos();
                    let (x, y) = (c[0] - centre[0], c[1] - centre[1]);
                    [centre[0] + x * k - y * s, centre[1] + x * s + y * k]
                }));
                pts.push(c);
            } else {
                pts = arc(centre, start, end, true);
            }
            Some(pts)
        }
    }
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

    fn add(a: &mut App, paths: Vec<Path>) -> u64 {
        let id = a.doc.add_vector(paths, Prim::Free);
        a.selected.push(id);
        id
    }

    #[test]
    fn move_uses_the_chosen_anchor() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([2.0, 2.0], 2.0, 2.0, 0.0))]);
        let mut f = XformForm::new(Xform::Move, &a.doc.job);
        f.anchor = Datum::BottomLeft;
        f.absolute = true;
        f.dx = 5.0;
        f.dy = 6.0;
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        let (lo, _) = a.selected_bbox().unwrap();
        assert!((lo[0] - 5.0).abs() < 1e-9 && (lo[1] - 6.0).abs() < 1e-9);
    }

    #[test]
    fn rotate_turns_around_the_pivot() {
        let mut a = app();
        add(&mut a, vec![Path::open(vec![[1.0, 0.0], [2.0, 0.0]])]);
        let mut f = XformForm::new(Xform::Rotate, &a.doc.job);
        f.pivot = [0.0, 0.0];
        f.angle = 90.0;
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        let p = a.doc.vectors[0].paths[0].pts[0];
        assert!(p[0].abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn mirror_can_make_a_copy_and_use_an_edge() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([2.0, 2.0], 2.0, 2.0, 0.0))]);
        a.mirror(true, Some(Edge::Right), false, true);
        assert_eq!(a.doc.vectors.len(), 2);
        let (lo, hi) = a.selected_bbox().unwrap();
        assert!((lo[0] - 3.0).abs() < 1e-9 && (hi[0] - 5.0).abs() < 1e-9);
    }

    #[test]
    fn alignment_to_the_last_selected_shape() {
        let mut a = app();
        let small = add(&mut a, vec![Path::closed(rect([1.0, 1.0], 1.0, 1.0, 0.0))]);
        add(&mut a, vec![Path::closed(rect([6.0, 6.0], 4.0, 4.0, 0.0))]);
        a.align_to_last_center(true, true);
        let bb = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == small).unwrap().paths).unwrap();
        assert!(((bb.0[0] + bb.1[0]) / 2.0 - 6.0).abs() < 1e-9);
        a.align_to_last_edge(Edge::Right, false);
        let bb = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == small).unwrap().paths).unwrap();
        assert!((bb.0[0] - 8.0).abs() < 1e-9, "sits just outside the right edge");
    }

    #[test]
    fn spacing_evens_out_the_middle_shape() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([1.0, 5.0], 1.0, 1.0, 0.0))]);
        let middle = add(&mut a, vec![Path::closed(rect([2.0, 5.0], 1.0, 1.0, 0.0))]);
        add(&mut a, vec![Path::closed(rect([9.0, 5.0], 1.0, 1.0, 0.0))]);
        a.space_selection(true);
        let bb = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == middle).unwrap().paths).unwrap();
        assert!(((bb.0[0] + bb.1[0]) / 2.0 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn join_reports_counts_and_respects_tolerance() {
        let mut a = app();
        add(&mut a, vec![Path::open(vec![[0.0, 0.0], [1.0, 0.0]])]);
        add(&mut a, vec![Path::open(vec![[1.05, 0.0], [2.0, 0.0]])]);
        assert_eq!(a.join_counts(0.01), ([0, 2], [0, 2]));
        assert_eq!(a.join_counts(0.1), ([0, 2], [0, 1]));
        a.join_with(0.1);
        assert_eq!(a.doc.vectors.len(), 1);
    }

    #[test]
    fn array_and_circular_copies_with_grouping_and_angles() {
        let mut a = app();
        add(&mut a, vec![Path::closed(circle([1.0, 1.0], 0.5))]);
        let mut f = XformForm::new(Xform::BlockCopy, &a.doc.job);
        (f.rows, f.cols, f.group) = (2, 2, true);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        assert_eq!(a.doc.vectors.len(), 1, "the shape and its copies are in one group");
        assert_eq!(a.doc.vectors[0].members.len(), 4);
        assert_eq!(a.selected, [a.doc.vectors[0].id]);
        let undo = a.undo.len();
        a.ungroup_selected();
        assert_eq!(a.undo.len(), undo + 1);
        assert_eq!(a.doc.vectors.len(), 4);

        let mut b = app();
        add(&mut b, vec![Path::closed(circle([8.0, 5.0], 0.5))]);
        let mut f = XformForm::new(Xform::CircularCopy, &b.doc.job);
        (f.count, f.dx, f.dy, f.total_angle, f.angle) = (3, 5.0, 5.0, false, 90.0);
        b.left = LeftForm::Xform(f);
        b.apply_xform();
        let (lo, _) = geom::bbox(&b.doc.vectors[1].paths).unwrap();
        assert!((lo[1] - 1.5).abs() < 0.01, "the first copy is a quarter turn clockwise, {lo:?}");
    }

    #[test]
    fn a_copied_circle_is_still_a_circle_unless_it_was_changed() {
        let mut a = app();
        let c = [1.0, 1.0];
        let id = a.doc.add_vector(vec![Path::closed(circle(c, 0.5))], Prim::Circle { c, r: 0.5 });
        a.selected = vec![id];
        let mut f = XformForm::new(Xform::BlockCopy, &a.doc.job);
        (f.rows, f.cols, f.use_offset, f.dx, f.dy) = (1, 2, true, 3.0, 0.0);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        assert_eq!(a.doc.vectors.len(), 2);
        let Prim::Circle { c, r } = a.doc.vectors[1].prim else { panic!("{:?}", a.doc.vectors[1].prim) };
        assert!((c[0] - 4.0).abs() < 1e-9 && (c[1] - 1.0).abs() < 1e-9 && (r - 0.5).abs() < 1e-9, "{c:?} {r}");
    }

    #[test]
    fn copy_along_places_circles_and_objects_on_the_curve() {
        let mut a = app();
        add(&mut a, vec![Path::open(vec![[0.0, 0.0], [10.0, 0.0]])]);
        let mut f = XformForm::new(Xform::CopyAlong, &a.doc.job);
        (f.copy_circles, f.by_number, f.count, f.size) = (true, true, 3, 0.2);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        assert_eq!(a.doc.vectors.len(), 4);
        let centres: Vec<f64> = a.doc.vectors[1..].iter().map(|v| {
            let (lo, hi) = geom::bbox(&v.paths).unwrap();
            (lo[0] + hi[0]) / 2.0
        }).collect();
        assert!((centres[0]).abs() < 1e-6 && (centres[1] - 5.0).abs() < 1e-6 && (centres[2] - 10.0).abs() < 1e-6, "{centres:?}");
    }

    #[test]
    fn fillet_rounds_a_rectangle_corner_to_the_right_size() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        assert!(a.fillet_at([3.0, 3.0], 0.1, 0.5, FilletKind::Normal));
        let path = &a.doc.vectors[0].paths[0];
        assert!(path.pts.len() > 10);
        assert!(!path.pts.iter().any(|p| p[0] == 3.0 && p[1] == 3.0), "the sharp corner is gone");
        let mid = path.pts[12];
        let d = (mid[0] - 3.5).hypot(mid[1] - 3.5);
        assert!((d - 0.5).abs() < 1e-6, "on the arc: {d}");
        assert!(geom::signed_area(&path.pts) < 16.0);
        assert!(!a.fillet_at([9.0, 9.0], 0.1, 0.5, FilletKind::Normal), "nothing to round there");
    }

    #[test]
    fn dog_bone_extends_past_the_corner() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        assert!(a.fillet_at([3.0, 3.0], 0.1, 0.25, FilletKind::DogBone));
        let (lo, _) = geom::bbox(&a.doc.vectors[0].paths).unwrap();
        assert!(lo[0] < 3.0 && lo[1] < 3.0, "{lo:?}");
    }

    #[test]
    fn trim_removes_only_the_clicked_stretch() {
        let mut a = app();
        add(&mut a, vec![Path::open(vec![[0.0, 5.0], [10.0, 5.0]])]);
        add(&mut a, vec![Path::open(vec![[3.0, 0.0], [3.0, 10.0]])]);
        add(&mut a, vec![Path::open(vec![[7.0, 0.0], [7.0, 10.0]])]);
        assert!(a.trim_at([5.0, 5.0], 0.1, false));
        let line = &a.doc.vectors[0];
        assert_eq!(line.paths.len(), 2, "split into two pieces");
        assert!((line.paths[0].pts.last().unwrap()[0] - 3.0).abs() < 1e-9);
        assert!((line.paths[1].pts[0][0] - 7.0).abs() < 1e-9);
        assert!(!a.trim_at([0.5, 9.0], 0.1, false), "nothing there");
    }

    #[test]
    fn trim_on_a_closed_shape_keeps_the_rest_open() {
        let mut a = app();
        add(&mut a, vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))]);
        add(&mut a, vec![Path::open(vec![[5.0, 0.0], [5.0, 10.0]])]);
        assert!(a.trim_at([4.0, 3.0], 0.1, false));
        let piece = &a.doc.vectors[0].paths[0];
        assert!(!piece.closed);
        assert!(piece.pts.iter().any(|p| p[0] == 7.0 && p[1] == 7.0), "the far side stays");
        let shift_del = a.trim_at([5.0, 9.5], 0.1, true);
        assert!(shift_del);
        assert_eq!(a.doc.vectors.len(), 1);
    }

    #[test]
    fn validator_finds_and_fixes_problems() {
        let mut a = app();
        add(&mut a, vec![Path::open(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [2.0, 0.0]])]);
        add(&mut a, vec![Path::open(vec![[1.5, -1.0], [1.5, 1.0]])]);
        add(&mut a, vec![Path::open(vec![[5.0, 5.0], [6.0, 5.0]])]);
        add(&mut a, vec![Path::open(vec![[6.0, 5.0], [5.0, 5.0]])]);
        a.selected.clear();
        a.validate();
        let count = |k| a.issues.iter().filter(|i| i.kind == k).count();
        assert_eq!(count(IssueKind::ZeroLength), 1);
        assert_eq!(count(IssueKind::Overlap), 1);
        assert_eq!(count(IssueKind::Intersection), 1);
        a.fix_zero_length();
        assert_eq!(a.doc.vectors[0].paths[0].pts.len(), 3);
    }

    #[test]
    fn dimension_measures_and_goes_on_its_own_layer() {
        let mut a = app();
        let f = XformForm::new(Xform::Dimension, &a.doc.job);
        a.add_dimension([1.0, 1.0], [4.0, 5.0], DimKind::Horizontal, &f);
        assert_eq!(a.doc.layers.last().unwrap().name, "Dimensions");
        let (lo, hi) = geom::bbox(a.doc.vectors[0].paths[..3].iter()).unwrap();
        assert!((hi[0] - lo[0] - 3.0).abs() < 1e-9, "spans the horizontal distance");
    }

    #[test]
    fn nesting_keeps_parts_apart_and_on_the_sheet() {
        let mut a = app();
        for _ in 0..4 {
            add(&mut a, vec![Path::closed(rect([5.0, 5.0], 3.0, 2.0, 0.0))]);
        }
        let mut f = XformForm::new(Xform::Nest, &a.doc.job);
        (f.tool_dia, f.clearance, f.border) = (0.25, 0.0, 0.5);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        let boxes: Vec<(P, P)> = a.doc.vectors.iter().map(|v| geom::bbox(&v.paths).unwrap()).collect();
        for b in &boxes {
            assert!(b.0[0] >= 0.5 - 1e-9 && b.1[0] <= 9.5 + 1e-9 && b.0[1] >= 0.5 - 1e-9, "{b:?}");
        }
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                let (p, q) = (boxes[i], boxes[j]);
                let apart = p.1[0] + 0.24 < q.0[0] || q.1[0] + 0.24 < p.0[0] || p.1[1] + 0.24 < q.0[1] || q.1[1] + 0.24 < p.0[1];
                assert!(apart, "{p:?} {q:?}");
            }
        }
    }

    #[test]
    fn text_follows_the_curve() {
        let mut a = app();
        let prim = Prim::Text { at: [0.0, 0.0], height: 0.5, text: "AB".into(), style: Default::default() };
        let t = a.doc.add_vector(prim.paths(), prim);
        let c = a.doc.add_vector(vec![Path::open(vec![[0.0, 3.0], [10.0, 3.0]])], Prim::Free);
        a.selected = vec![t, c];
        let mut f = XformForm::new(Xform::TextOnCurve, &a.doc.job);
        f.anchor = Datum::Centre;
        f.distance = 0.0;
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        let (lo, hi) = geom::bbox(&a.doc.vectors[0].paths).unwrap();
        assert!(lo[1] > 2.9 && hi[1] < 3.6, "{lo:?} {hi:?}");
        assert!((lo[0] + hi[0]) / 2.0 > 4.0 && (lo[0] + hi[0]) / 2.0 < 6.0, "centred on the curve");
    }

    #[test]
    fn set_size_and_rotate_can_treat_each_shape_on_its_own() {
        let mut a = app();
        let one = add(&mut a, vec![Path::closed(rect([2.0, 2.0], 1.0, 1.0, 0.0))]);
        let two = add(&mut a, vec![Path::closed(rect([8.0, 8.0], 1.0, 1.0, 0.0))]);
        let mut f = XformForm::new(Xform::Scale, &a.doc.job);
        (f.sx, f.each) = (200.0, true);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        for (id, c) in [(one, 2.0), (two, 8.0)] {
            let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == id).unwrap().paths).unwrap();
            assert!((hi[0] - lo[0] - 2.0).abs() < 1e-9, "doubled");
            assert!(((lo[0] + hi[0]) / 2.0 - c).abs() < 1e-9, "stays centred on itself");
        }
        let mut f = XformForm::new(Xform::Rotate, &a.doc.job);
        (f.angle, f.each) = (45.0, true);
        a.left = LeftForm::Xform(f);
        a.apply_xform();
        let (lo, hi) = geom::bbox(&a.doc.vectors.iter().find(|v| v.id == two).unwrap().paths).unwrap();
        assert!(((lo[0] + hi[0]) / 2.0 - 8.0).abs() < 1e-9, "turned about its own centre");
    }

}
