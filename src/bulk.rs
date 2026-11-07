//! Finding shapes that are alike, and changing them all at once.

use crate::app::*;
use crate::edit::anchor_point;
use crate::geom::{self, Path, P};
use crate::toolpath;

/// What a shape is, judged from its geometry so imported or scaled shapes count too.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    Circle { d: f64 },
    Rectangle { w: f64, h: f64 },
    Other { w: f64, h: f64 },
}

/// The centre and radius when a closed path is a circle.
pub fn as_circle(path: &Path) -> Option<(P, f64)> {
    if !path.closed || path.pts.len() < 16 {
        return None;
    }
    let n = path.pts.len() as f64;
    let c = [path.pts.iter().map(|p| p[0]).sum::<f64>() / n, path.pts.iter().map(|p| p[1]).sum::<f64>() / n];
    let r = path.pts.iter().map(|p| (p[0] - c[0]).hypot(p[1] - c[1])).sum::<f64>() / n;
    (r > 0.0 && path.pts.iter().all(|p| ((p[0] - c[0]).hypot(p[1] - c[1]) - r).abs() <= r * 0.004)).then_some((c, r))
}

/// The centre and radii when a closed path is an upright ellipse.
pub fn as_ellipse(path: &Path) -> Option<(P, f64, f64)> {
    if !path.closed || path.pts.len() < 16 {
        return None;
    }
    let (lo, hi) = geom::bbox(std::iter::once(path))?;
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (rx, ry) = ((hi[0] - lo[0]) / 2.0, (hi[1] - lo[1]) / 2.0);
    (rx > 0.0
        && ry > 0.0
        && path.pts.iter().all(|p| {
            let (x, y) = ((p[0] - c[0]) / rx, (p[1] - c[1]) / ry);
            (x * x + y * y - 1.0).abs() < 0.01
        }))
    .then_some((c, rx, ry))
}

pub fn is_rectangle(path: &Path) -> bool {
    path.closed
        && path.pts.len() == 4
        && path.segments().all(|(a, b)| {
            let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
            // Loose enough for a rectangle that has been through the rules.
            dx < 1e-4 * (dx + dy) || dy < 1e-4 * (dx + dy)
        })
}

pub fn classify(v: &Vector) -> Option<Class> {
    if let [only] = v.paths.as_slice() {
        if let Some((_, r)) = as_circle(only) {
            return Some(Class::Circle { d: r * 2.0 });
        }
        if is_rectangle(only) {
            let (lo, hi) = geom::bbox(&v.paths)?;
            return Some(Class::Rectangle { w: hi[0] - lo[0], h: hi[1] - lo[1] });
        }
    }
    let (lo, hi) = geom::bbox(&v.paths)?;
    match v.prim {
        Prim::Rect { .. } => Some(Class::Rectangle { w: hi[0] - lo[0], h: hi[1] - lo[1] }),
        _ => Some(Class::Other { w: hi[0] - lo[0], h: hi[1] - lo[1] }),
    }
}

impl Class {
    pub fn same_as(&self, other: &Class, tol: f64) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= tol;
        match (self, other) {
            (Class::Circle { d: a }, Class::Circle { d: b }) => near(*a, *b),
            (Class::Rectangle { w: a, h: b }, Class::Rectangle { w: c, h: d }) | (Class::Other { w: a, h: b }, Class::Other { w: c, h: d }) => near(*a, *c) && near(*b, *d),
            _ => false,
        }
    }

    pub fn describe(&self, unit: &str) -> String {
        let n = |v: f64| toolpath::trim((v * 10_000.0).round() / 10_000.0);
        match self {
            Class::Circle { d } => tf!("Circle, diameter {} {unit}", n(*d)),
            Class::Rectangle { w, h } => tf!("Rectangle {} x {} {unit}", n(*w), n(*h)),
            Class::Other { w, h } => tf!("Shape {} x {} {unit}", n(*w), n(*h)),
        }
    }
}

/// A set of alike shapes on the sheet.
#[derive(Clone, Debug)]
pub struct SimilarGroup {
    pub class: Class,
    pub ids: Vec<u64>,
}

impl App {
    fn editable(&self, v: &Vector) -> bool {
        self.doc.layers.get(v.layer).is_none_or(|l| l.visible && !l.locked)
    }

    /// Groups the visible shapes by kind and size, the biggest groups first.
    pub fn similar_groups(&self, tol: f64, active_layer_only: bool) -> Vec<SimilarGroup> {
        let mut groups: Vec<SimilarGroup> = Vec::new();
        for v in self.doc.vectors.iter().filter(|v| self.editable(v) && (!active_layer_only || v.layer == self.doc.active_layer)) {
            let Some(class) = classify(v) else { continue };
            match groups.iter_mut().find(|g| g.class.same_as(&class, tol)) {
                Some(g) => g.ids.push(v.id),
                None => groups.push(SimilarGroup { class, ids: vec![v.id] }),
            }
        }
        groups.sort_by(|a, b| b.ids.len().cmp(&a.ids.len()));
        groups
    }

    /// Adds to the selection every shape alike one that is already selected.
    pub fn select_similar(&mut self, tol: f64) {
        let seeds: Vec<Class> = self.doc.vectors.iter().filter(|v| self.selected.contains(&v.id)).filter_map(classify).collect();
        if seeds.is_empty() {
            self.status = crate::i18n::tr("Select one shape first, then its look-alikes can be found.").into();
            return;
        }
        let matches: Vec<u64> = self
            .doc
            .vectors
            .iter()
            .filter(|v| self.editable(v) && classify(v).is_some_and(|c| seeds.iter().any(|s| s.same_as(&c, tol))))
            .map(|v| v.id)
            .collect();
        self.status = tf!("{} similar shapes selected.", matches.len());
        self.selected = matches;
    }

    /// The tolerance used when none was typed: a tenth of a percent of the job.
    pub fn default_similar_tolerance(&self) -> f64 {
        self.doc.job.width.max(self.doc.job.height) * 1e-3
    }

    /// Resizes each selected shape on its own: circles to `d`, the others to `w` by `h`.
    /// A value of `None` leaves that size as it is.
    pub fn resize_each(&mut self, d: Option<f64>, w: Option<f64>, h: Option<f64>, anchor: Datum) -> usize {
        if d.is_some_and(|v| v <= 0.0) || w.is_some_and(|v| v <= 0.0) || h.is_some_and(|v| v <= 0.0) {
            self.status = crate::i18n::tr("Sizes must be above zero.").into();
            return 0;
        }
        self.checkpoint();
        let mut changed = 0;
        let ids = self.selected.clone();
        for v in self.doc.vectors.iter_mut().filter(|v| ids.contains(&v.id)) {
            let Some(class) = classify(v) else { continue };
            let Some((lo, hi)) = geom::bbox(&v.paths) else { continue };
            let at = anchor_point(lo, hi, anchor);
            if let Class::Circle { d: old } = class {
                let Some(new) = d else { continue };
                let (c, r) = as_circle(&v.paths[0]).unwrap();
                let c = [at[0] + (c[0] - at[0]) * new / old, at[1] + (c[1] - at[1]) * new / old];
                let _ = r;
                v.paths = vec![Path::closed(geom::circle(c, new / 2.0))];
                v.prim = Prim::Circle { c, r: new / 2.0 };
                changed += 1;
                continue;
            }
            if w.is_none() && h.is_none() {
                continue;
            }
            let (old_w, old_h) = (hi[0] - lo[0], hi[1] - lo[1]);
            let sx = w.map_or(1.0, |n| if old_w > 0.0 { n / old_w } else { 1.0 });
            let sy = h.map_or(1.0, |n| if old_h > 0.0 { n / old_h } else { 1.0 });
            for p in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
                *p = [at[0] + (p[0] - at[0]) * sx, at[1] + (p[1] - at[1]) * sy];
            }
            v.prim = match v.prim {
                Prim::Rect { c, w: pw, h: ph, corner } => Prim::Rect {
                    c: [at[0] + (c[0] - at[0]) * sx, at[1] + (c[1] - at[1]) * sy],
                    w: pw * sx,
                    h: ph * sy,
                    corner,
                },
                _ => Prim::Free,
            };
            changed += 1;
        }
        self.status = tf!("Changed {changed} shapes.");
        changed
    }

    /// The toolpaths whose shapes have changed, each with its settings and its shapes as they are now.
    pub(crate) fn follow_tasks(&mut self) -> Vec<crate::recalc::Task> {
        // Shapes chosen by filter are chosen afresh, in case the drawing has changed.
        let chosen: Vec<(usize, Vec<u64>)> = self.doc.toolpaths.iter().enumerate().filter_map(|(i, t)| t.op.selector.as_ref().map(|f| (i, self.filter_ids(f, t.op.tool.dia)))).collect();
        for (i, ids) in chosen {
            self.doc.toolpaths[i].op.source = ids;
        }
        let mut tasks = Vec::new();
        let shapes = self.doc.current_paths();
        for (i, tp) in self.doc.toolpaths.iter().enumerate() {
            if tp.op.source.is_empty() || tp.op.merged {
                continue;
            }
            let paths: Vec<Path> = tp.op.source.iter().filter_map(|id| shapes.get(id)).flat_map(|lines| lines.iter().cloned()).collect();
            if !paths.is_empty() && paths != tp.op.paths {
                let mut op = tp.op.clone();
                op.paths = paths;
                tasks.push((i, op));
            }
        }
        tasks
    }

    /// Toolpaths stay as they are until Recalculate is pressed: this only notes which are out of date,
    /// once the change is over, and says so.
    pub fn note_changed_shapes(&mut self) {
        self.doc.tidy_pieces();
        let dragging = !matches!(self.drag, crate::canvas::Drag::None);
        if dragging || self.nudging || self.recalc.is_some() {
            return;
        }
        let mut print = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |v: u64| {
            print ^= v;
            print = print.wrapping_mul(0x100_0000_01b3);
        };
        for v in &self.doc.vectors {
            eat(v.id);
            eat(u64::from(v.construction));
            eat(v.layer as u64);
            for q in v.paths.iter().flat_map(|p| p.pts.iter()) {
                eat(q[0].to_bits());
                eat(q[1].to_bits());
            }
        }
        for t in &self.doc.toolpaths {
            eat(std::sync::Arc::as_ptr(&t.moves) as usize as u64);
        }
        eat(u64::from(self.all_behind));
        if self.shapes_print == Some(print) {
            return;
        }
        self.shapes_print = Some(print);
        let before = self.behind.len();
        self.behind = self.follow_tasks().into_iter().map(|(i, _)| i).collect();
        if self.all_behind {
            self.behind = (0..self.doc.toolpaths.len()).collect();
        }
        if self.behind.len() > before {
            self.status = tf!("{} toolpath(s) no longer match their shapes. Use Recalculate when you're ready.", self.behind.len());
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

    fn hole(a: &mut App, c: P, d: f64) -> u64 {
        a.doc.add_vector(vec![Path::closed(circle(c, d / 2.0))], Prim::Free)
    }

    #[test]
    fn imported_circles_and_rectangles_are_recognised() {
        let mut a = app();
        let c = hole(&mut a, [1.0, 1.0], 0.16);
        let r = a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 2.0, 1.0, 0.0))], Prim::Free);
        let s = a.doc.add_vector(vec![Path::closed(geom::star([5.0, 5.0], 2.0, 1.0, 5))], Prim::Free);
        let class = |a: &App, id| classify(a.doc.vectors.iter().find(|v| v.id == id).unwrap()).unwrap();
        assert!(matches!(class(&a, c), Class::Circle { d } if (d - 0.16).abs() < 1e-3));
        assert!(matches!(class(&a, r), Class::Rectangle { w, h } if (w - 2.0).abs() < 1e-9 && (h - 1.0).abs() < 1e-9));
        assert!(matches!(class(&a, s), Class::Other { .. }));
    }

    #[test]
    fn groups_collect_same_size_holes() {
        let mut a = app();
        for i in 0..4 {
            hole(&mut a, [1.0 + i as f64, 1.0], 0.16);
        }
        hole(&mut a, [1.0, 3.0], 0.25);
        let groups = a.similar_groups(0.005, false);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].ids.len(), 4);
        assert!(groups[0].class.describe("inches").contains("0.16"));
    }

    #[test]
    fn select_similar_finds_all_holes_of_the_same_size_only() {
        let mut a = app();
        let first = hole(&mut a, [1.0, 1.0], 0.16);
        hole(&mut a, [2.0, 1.0], 0.16);
        hole(&mut a, [3.0, 1.0], 0.16);
        let other = hole(&mut a, [4.0, 1.0], 0.5);
        a.selected = vec![first];
        a.select_similar(0.005);
        assert_eq!(a.selected.len(), 3);
        assert!(!a.selected.contains(&other));
    }

    #[test]
    fn locked_and_hidden_layers_are_left_out() {
        let mut a = app();
        let first = hole(&mut a, [1.0, 1.0], 0.16);
        a.doc.layers.push(Layer::named("Locked"));
        a.doc.active_layer = 1;
        hole(&mut a, [2.0, 1.0], 0.16);
        a.doc.layers[1].locked = true;
        a.selected = vec![first];
        a.select_similar(0.005);
        assert_eq!(a.selected, vec![first]);
    }

    #[test]
    fn resizing_all_holes_keeps_each_centre() {
        let mut a = app();
        let ids: Vec<u64> = (0..3).map(|i| hole(&mut a, [1.0 + i as f64 * 2.0, 2.0], 0.16)).collect();
        a.selected = ids.clone();
        assert_eq!(a.resize_each(Some(0.25), None, None, Datum::Centre), 3);
        for (i, id) in ids.iter().enumerate() {
            let v = a.doc.vectors.iter().find(|v| v.id == *id).unwrap();
            let (c, r) = as_circle(&v.paths[0]).unwrap();
            assert!((r - 0.125).abs() < 1e-3, "radius {r}");
            assert!((c[0] - (1.0 + i as f64 * 2.0)).abs() < 1e-6 && (c[1] - 2.0).abs() < 1e-6, "{c:?}");
            assert!(matches!(v.prim, Prim::Circle { .. }), "stays an editable circle");
        }
        a.do_undo();
        let (_, r) = as_circle(&a.doc.vectors[0].paths[0]).unwrap();
        assert!((r - 0.08).abs() < 1e-3, "one undo restores them all");
    }

    #[test]
    fn mixed_selection_changes_each_kind_by_its_own_field() {
        let mut a = app();
        let h = hole(&mut a, [1.0, 1.0], 0.16);
        let r = a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 2.0, 1.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 2.0, h: 1.0, corner: 0.0 });
        a.selected = vec![h, r];
        a.resize_each(Some(0.3), Some(4.0), None, Datum::Centre);
        let (lo, hi) = geom::bbox(&a.doc.vectors[1].paths).unwrap();
        assert!((hi[0] - lo[0] - 4.0).abs() < 1e-9 && (hi[1] - lo[1] - 1.0).abs() < 1e-9);
        assert!(matches!(a.doc.vectors[1].prim, Prim::Rect { w, .. } if (w - 4.0).abs() < 1e-9));
        let (_, radius) = as_circle(&a.doc.vectors[0].paths[0]).unwrap();
        assert!((radius - 0.15).abs() < 1e-3);
    }

    #[test]
    fn zero_or_negative_sizes_are_refused() {
        let mut a = app();
        let h = hole(&mut a, [1.0, 1.0], 0.16);
        a.selected = vec![h];
        assert_eq!(a.resize_each(Some(0.0), None, None, Datum::Centre), 0);
        assert!(a.undo.is_empty());
    }

    #[test]
    fn toolpaths_wait_for_recalculate_after_resizing_holes() {
        let mut a = app();
        let ids: Vec<u64> = (0..2).map(|i| hole(&mut a, [2.0 + i as f64 * 3.0, 3.0], 1.0)).collect();
        a.selected = ids;
        a.start_op(toolpath::OpKind::Profile);
        a.calculate_op();
        assert_eq!(a.doc.toolpaths[0].op.source.len(), 2);
        let widest = |a: &App| a.doc.toolpaths[0].moves.iter().filter(|m| !m.rapid && m.p[2] < 0.0).map(|m| m.p[0]).fold(0.0, f64::max);
        let before = widest(&a);
        a.resize_each(Some(2.0), None, None, Datum::Centre);
        assert_eq!(widest(&a), before, "not on its own");
        a.recalculate_all();
        assert!((widest(&a) - before - 0.5).abs() < 0.02, "{} -> {}", before, widest(&a));
    }
}
