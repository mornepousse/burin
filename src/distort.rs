//! Distort: bending shapes inside an envelope.

use crate::app::*;
use crate::geom::{self, Path, P};

/// How the envelope around the shapes is defined.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvelopeKind {
    BoundingBox,
    OneCurve,
    TwoCurves,
}

/// A distortion in progress: the original shapes and the box they came from.
#[derive(Clone, Debug)]
pub struct Envelope {
    orig: Vec<(u64, Vec<Path>)>,
    lo: P,
    hi: P,
    /// Bottom left, bottom right, top right, top left.
    pub corners: [P; 4],
    curves: Option<(Vec<P>, Vec<P>)>,
}

fn route_of(path: &Path) -> Vec<P> {
    let mut r = path.pts.clone();
    if path.closed {
        r.push(r[0]);
    }
    r
}

/// The point a fraction of the way along a polyline.
fn at_fraction(route: &[P], t: f64) -> P {
    let total: f64 = route.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
    let mut left = t.clamp(0.0, 1.0) * total;
    for w in route.windows(2) {
        let d = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if left <= d && d > 0.0 {
            let k = left / d;
            return [w[0][0] + (w[1][0] - w[0][0]) * k, w[0][1] + (w[1][1] - w[0][1]) * k];
        }
        left -= d;
    }
    *route.last().unwrap_or(&[0.0, 0.0])
}

impl Envelope {
    fn map(&self, p: P) -> P {
        let (w, h) = ((self.hi[0] - self.lo[0]).max(1e-9), (self.hi[1] - self.lo[1]).max(1e-9));
        let (u, v) = ((p[0] - self.lo[0]) / w, (p[1] - self.lo[1]) / h);
        let (bottom, top) = match &self.curves {
            Some((b, t)) => (at_fraction(b, u), at_fraction(t, u)),
            None => {
                let [a, b, c, d] = self.corners;
                ([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u], [d[0] + (c[0] - d[0]) * u, d[1] + (c[1] - d[1]) * u])
            }
        };
        [bottom[0] + (top[0] - bottom[0]) * v, bottom[1] + (top[1] - bottom[1]) * v]
    }

    pub fn has_handles(&self) -> bool {
        self.curves.is_none()
    }
}

/// Adds points along long straight stretches, so they can bend.
fn densify(path: &Path, step: f64) -> Path {
    let mut pts = Vec::with_capacity(path.pts.len());
    for (a, b) in path.segments() {
        pts.push(a);
        let n = ((a[0] - b[0]).hypot(a[1] - b[1]) / step).ceil() as usize;
        for i in 1..n {
            let t = i as f64 / n as f64;
            pts.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    if !path.closed {
        pts.push(*path.pts.last().unwrap());
    }
    Path { pts, closed: path.closed }
}

impl App {
    /// Which shapes are distorted and which are the curves, in the order they were clicked.
    fn envelope_parts(&self, kind: EnvelopeKind) -> Result<(Vec<u64>, Vec<u64>), String> {
        let order: Vec<u64> = self.selected.iter().copied().filter(|id| self.doc.vectors.iter().any(|v| v.id == *id)).collect();
        let curves_wanted = match kind {
            EnvelopeKind::BoundingBox => 0,
            EnvelopeKind::OneCurve => 1,
            EnvelopeKind::TwoCurves => 2,
        };
        if order.len() <= curves_wanted {
            return Err(match curves_wanted {
                0 => crate::i18n::tr("Select the object to distort.").to_string(),
                1 => crate::i18n::tr("Select the object, then one open line under it.").to_string(),
                _ => crate::i18n::tr("Select the object, then two open lines.").to_string(),
            });
        }
        let (objects, curves) = order.split_at(order.len() - curves_wanted);
        for id in curves {
            let v = self.doc.vectors.iter().find(|v| v.id == *id).unwrap();
            if v.paths.len() != 1 || v.paths[0].closed || v.paths[0].pts.len() < 2 {
                return Err(crate::i18n::tr("The last selected shapes must be open lines.").to_string());
            }
        }
        Ok((objects.to_vec(), curves.to_vec()))
    }

    /// Says whether the selection suits the chosen envelope.
    pub fn envelope_check(&self, kind: EnvelopeKind) -> Result<(), String> {
        self.envelope_parts(kind).map(|_| ())
    }

    /// Puts the selection into an envelope.
    pub fn apply_envelope(&mut self, kind: EnvelopeKind) -> bool {
        let (objects, curves) = match self.envelope_parts(kind) {
            Ok(parts) => parts,
            Err(why) => {
                self.status = why;
                return false;
            }
        };
        let Some((lo, hi)) = geom::bbox(self.doc.vectors.iter().filter(|v| objects.contains(&v.id)).flat_map(|v| v.paths.iter())) else { return false };
        let route = |app: &App, id: u64| route_of(&app.doc.vectors.iter().find(|v| v.id == id).unwrap().paths[0]);
        let curve_routes = match curves.as_slice() {
            [c] => {
                let bottom = route(self, *c);
                let up = hi[1] - lo[1];
                let top: Vec<P> = bottom.iter().map(|p| [p[0], p[1] + up]).collect();
                Some((bottom, top))
            }
            [a, b] => {
                let (mut first, mut second) = (route(self, *a), route(self, *b));
                let mean = |r: &[P]| r.iter().map(|p| p[1]).sum::<f64>() / r.len() as f64;
                if mean(&first) > mean(&second) {
                    std::mem::swap(&mut first, &mut second);
                }
                // Both run the same way, left to right, whichever way they were drawn.
                for r in [&mut first, &mut second] {
                    if r[0][0] > r[r.len() - 1][0] {
                        r.reverse();
                    }
                }
                Some((first, second))
            }
            _ => None,
        };
        let step = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6) / 48.0;
        let orig: Vec<(u64, Vec<Path>)> = self.doc.vectors.iter().filter(|v| objects.contains(&v.id)).map(|v| (v.id, v.paths.iter().map(|p| densify(p, step)).collect())).collect();
        let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        let envelope = Envelope { orig, lo, hi, corners, curves: curve_routes };
        self.keep_rules(|a| {
            a.checkpoint();
            a.envelope = Some(envelope);
            a.redraw_envelope();
        });
        self.selected = objects;
        self.status = crate::i18n::tr("Distortion applied.").into();
        true
    }

    /// Reshapes the shapes from their originals and the envelope as it is now.
    fn redraw_envelope(&mut self) {
        let Some(env) = self.envelope.clone() else { return };
        for (id, paths) in &env.orig {
            if let Some(v) = self.doc.vectors.iter_mut().find(|v| v.id == *id) {
                v.paths = paths.iter().map(|p| Path { pts: p.pts.iter().map(|q| env.map(*q)).collect(), closed: p.closed }).collect();
                v.prim = Prim::Free;
            }
        }
    }

    /// Moves one corner of the envelope.
    pub fn move_envelope_corner(&mut self, corner: usize, to: P) {
        if let Some(env) = &mut self.envelope {
            env.corners[corner] = to;
        }
        self.redraw_envelope();
    }

    /// The handles only exist while the Distort form is open.
    pub fn drop_stale_envelope(&mut self) {
        if self.envelope.is_some() && !matches!(&self.left, LeftForm::Xform(f) if f.kind == Xform::Distort) {
            self.envelope = None;
        }
    }

    /// Finishes: the shapes keep their new form and the handles go away.
    pub fn bake_envelope(&mut self) {
        self.envelope = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{bbox, rect};

    fn app() -> App {
        let mut a = App::default();
        a.new_file();
        a.left = LeftForm::None;
        a
    }

    fn square(a: &mut App) -> u64 {
        a.doc.add_vector(vec![Path::closed(rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free)
    }

    #[test]
    fn a_new_envelope_leaves_the_shape_where_it_was() {
        let mut a = app();
        let s = square(&mut a);
        a.selected = vec![s];
        assert!(a.apply_envelope(EnvelopeKind::BoundingBox));
        let (lo, hi) = bbox(&a.doc.vectors[0].paths).unwrap();
        assert!((lo[0] - 3.0).abs() < 1e-9 && (hi[1] - 7.0).abs() < 1e-9, "{lo:?} {hi:?}");
        assert!(a.doc.vectors[0].paths[0].pts.len() > 4, "points were added so sides can bend");
    }

    #[test]
    fn moving_a_corner_makes_a_keystone_and_undo_restores_it() {
        let mut a = app();
        let s = square(&mut a);
        a.selected = vec![s];
        a.apply_envelope(EnvelopeKind::BoundingBox);
        a.move_envelope_corner(2, [9.0, 9.0]);
        let v = &a.doc.vectors[0];
        assert!(v.paths[0].pts.iter().any(|p| (p[0] - 9.0).abs() < 1e-6 && (p[1] - 9.0).abs() < 1e-6), "the corner went there");
        assert!(v.paths[0].pts.iter().any(|p| (p[0] - 3.0).abs() < 1e-6 && (p[1] - 3.0).abs() < 1e-6), "the opposite one stayed");
        let on_top = at_fraction(&[[3.0, 7.0], [9.0, 9.0]], 0.5);
        assert!(v.paths[0].pts.iter().any(|p| (p[0] - on_top[0]).abs() < 0.2 && (p[1] - on_top[1]).abs() < 0.1), "the top side leans");
        a.bake_envelope();
        assert!(a.envelope.is_none());
        a.do_undo();
        assert_eq!(a.doc.vectors[0].paths[0].pts.len(), 4, "one undo brings the original back");
    }

    #[test]
    fn a_shape_above_one_curve_follows_it() {
        let mut a = app();
        let s = a.doc.add_vector(vec![Path::closed(rect([5.0, 6.0], 6.0, 2.0, 0.0))], Prim::Free);
        let curve = a.doc.add_vector(vec![Path::open(vec![[1.0, 1.0], [5.0, 3.0], [9.0, 1.0]])], Prim::Free);
        a.selected = vec![s, curve];
        assert!(a.envelope_check(EnvelopeKind::OneCurve).is_ok());
        assert!(a.apply_envelope(EnvelopeKind::OneCurve));
        let pts = &a.doc.vectors[0].paths[0].pts;
        let low = pts.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        assert!((low - 1.0).abs() < 1e-6, "its bottom sits on the curve: {low}");
        let peak = pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!((peak - 5.0).abs() < 0.1, "the top is the height above the curve's middle: {peak}");
        assert_eq!(a.selected, vec![s]);
        assert!(!a.envelope.as_ref().unwrap().has_handles());
    }

    #[test]
    fn a_shape_between_two_curves_fills_the_space_whichever_way_they_were_drawn() {
        let mut a = app();
        let s = square(&mut a);
        let top = a.doc.add_vector(vec![Path::open(vec![[9.0, 9.0], [1.0, 9.0]])], Prim::Free);
        let bottom = a.doc.add_vector(vec![Path::open(vec![[1.0, 1.0], [9.0, 1.0]])], Prim::Free);
        a.selected = vec![s, top, bottom];
        assert!(a.apply_envelope(EnvelopeKind::TwoCurves));
        let (lo, hi) = bbox(&a.doc.vectors[0].paths).unwrap();
        for (got, want) in [(lo[0], 1.0), (lo[1], 1.0), (hi[0], 9.0), (hi[1], 9.0)] {
            assert!((got - want).abs() < 1e-6, "{lo:?} {hi:?}");
        }
        // Not twisted: the square's first corner is still bottom left.
        assert!((a.doc.vectors[0].paths[0].pts[0][0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn undo_while_distorting_removes_the_handles_too() {
        let mut a = app();
        let s = square(&mut a);
        a.selected = vec![s];
        a.apply_envelope(EnvelopeKind::BoundingBox);
        a.do_undo();
        assert!(a.envelope.is_none(), "dragging a stale corner would redo the distortion");
        assert_eq!(a.doc.vectors[0].paths[0].pts.len(), 4);
    }

    #[test]
    fn leaving_the_form_any_other_way_keeps_the_new_form_and_drops_the_handles() {
        let mut a = app();
        let s = square(&mut a);
        a.selected = vec![s];
        a.start_xform(Xform::Distort);
        a.apply_envelope(EnvelopeKind::BoundingBox);
        a.move_envelope_corner(2, [9.0, 9.0]);
        a.drop_stale_envelope();
        assert!(a.envelope.is_some(), "still in the form");
        a.left = LeftForm::None;
        a.drop_stale_envelope();
        assert!(a.envelope.is_none());
        assert!(bbox(&a.doc.vectors[0].paths).unwrap().1[0] > 8.9);
    }

    #[test]
    fn the_wrong_selection_is_explained() {
        let mut a = app();
        assert!(a.envelope_check(EnvelopeKind::BoundingBox).unwrap_err().contains("Select the object"));
        assert!(!a.apply_envelope(EnvelopeKind::BoundingBox));
        let s = square(&mut a);
        let t = square(&mut a);
        a.selected = vec![s];
        assert!(a.envelope_check(EnvelopeKind::OneCurve).unwrap_err().contains("one open line"));
        a.selected = vec![s, t];
        assert!(a.envelope_check(EnvelopeKind::OneCurve).unwrap_err().contains("open lines"), "a closed shape can't be the curve");
    }
}
