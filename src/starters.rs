//! Models to start from on the start screen: a name plaque, a coaster and a small box, each drawn
//! and with its toolpaths worked out, in millimetres, ready to change. Burin's own.

use crate::app::*;
use crate::geom::{self, Path};
use crate::toolpath::OpKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Starter {
    Plaque,
    Coaster,
    BoxPanels,
}

impl Starter {
    pub const ALL: [Starter; 3] = [Starter::Plaque, Starter::Coaster, Starter::BoxPanels];

    pub fn name(self) -> &'static str {
        match self {
            Starter::Plaque => crate::i18n::tr("Name plaque"),
            Starter::Coaster => crate::i18n::tr("Coaster"),
            Starter::BoxPanels => crate::i18n::tr("Small box"),
        }
    }
}

impl App {
    /// A fresh job in millimetres of this size.
    fn starter_job(&mut self, w: f64, h: f64, thickness: f64) {
        self.doc = Doc::default();
        self.reset_state();
        self.screen = Screen::Work;
        self.doc.set_units(Units::Mm);
        self.doc.job.width = w;
        self.doc.job.height = h;
        self.doc.job.thickness = thickness;
    }

    /// Works out a toolpath of this kind for these shapes, with cut-out tabs when asked.
    fn starter_toolpath(&mut self, shapes: Vec<u64>, kind: OpKind, depth: Option<f64>, tabs: bool) {
        self.selected = shapes;
        self.start_op(kind);
        if let RightMode::Op { op, .. } = &mut self.right {
            if let Some(d) = depth {
                op.cut_depth = d;
            }
            op.tabs = tabs;
        }
        self.calculate_op();
    }

    pub fn load_starter(&mut self, which: Starter) {
        match which {
            Starter::Plaque => {
                self.starter_job(200.0, 80.0, 12.0);
                let outline = self.doc.add_vector(vec![Path::closed(geom::rect([100.0, 40.0], 192.0, 72.0, 10.0))], Prim::Rect { c: [100.0, 40.0], w: 192.0, h: 72.0, corner: 10.0 });
                let border = self.doc.add_vector(vec![Path::closed(geom::rect([100.0, 40.0], 176.0, 56.0, 6.0))], Prim::Rect { c: [100.0, 40.0], w: 176.0, h: 56.0, corner: 6.0 });
                // The word in the middle of the plaque: measured where it would start, then written there.
                let style: crate::fonts::TextStyle = Default::default();
                let trial = Prim::Text { at: [0.0, 0.0], height: 28.0, text: "Burin".into(), style: style.clone() };
                let at = geom::bbox(trial.paths().iter()).map_or([40.0, 26.0], |(lo, hi)| [100.0 - (lo[0] + hi[0]) / 2.0, 40.0 - (lo[1] + hi[1]) / 2.0]);
                let text = Prim::Text { at, height: 28.0, text: "Burin".into(), style };
                let word = self.doc.add_vector(text.paths(), text);
                self.starter_toolpath(vec![border, word], OpKind::VCarve, None, false);
                self.starter_toolpath(vec![outline], OpKind::Profile, Some(12.5), true);
            }
            Starter::Coaster => {
                self.starter_job(100.0, 100.0, 8.0);
                let rim = self.doc.add_vector(vec![Path::closed(geom::circle([50.0, 50.0], 46.0))], Prim::Circle { c: [50.0, 50.0], r: 46.0 });
                let dish = self.doc.add_vector(vec![Path::closed(geom::circle([50.0, 50.0], 38.0))], Prim::Circle { c: [50.0, 50.0], r: 38.0 });
                self.starter_toolpath(vec![dish], OpKind::Pocket, Some(2.0), false);
                self.starter_toolpath(vec![rim], OpKind::Profile, Some(8.5), true);
            }
            Starter::BoxPanels => {
                self.starter_job(300.0, 200.0, 6.0);
                // The bottom, two sides and two ends, laid out with room for the cutter between.
                let panels = [([75.0, 140.0], 130.0, 90.0), ([215.0, 160.0], 130.0, 50.0), ([215.0, 95.0], 130.0, 50.0), ([60.0, 45.0], 90.0, 50.0), ([165.0, 45.0], 90.0, 50.0)];
                let ids: Vec<u64> = panels.iter().map(|(c, w, h)| self.doc.add_vector(vec![Path::closed(geom::rect(*c, *w, *h, 0.0))], Prim::Rect { c: *c, w: *w, h: *h, corner: 0.0 })).collect();
                self.starter_toolpath(ids, OpKind::Profile, Some(6.5), true);
            }
        }
        self.selected.clear();
        self.selected_tp = Some(0);
        // Opened on its drawing, its toolpaths listed.
        self.right = RightMode::List;
        self.view3d = false;
        self.undo.clear();
        self.dirty = false;
        self.auto_fit = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_starter_has_its_shapes_and_toolpaths_ready() {
        for which in Starter::ALL {
            let mut app = App::default();
            app.load_starter(which);
            assert_eq!(app.doc.job.units, Units::Mm, "{which:?}");
            assert!(!app.doc.vectors.is_empty(), "{which:?}");
            assert!(!app.doc.toolpaths.is_empty() && app.doc.toolpaths.iter().all(|t| !t.moves.is_empty()), "{which:?}: {:?}", app.doc.toolpaths.iter().map(|t| (t.op.name.clone(), t.moves.len())).collect::<Vec<_>>());
            assert!(app.follow_tasks().is_empty(), "{which:?}: no toolpath is behind its shapes");
            // Everything on the material.
            let (lo, hi) = geom::bbox(app.doc.vectors.iter().flat_map(|v| v.paths.iter())).unwrap();
            assert!(lo[0] >= 0.0 && lo[1] >= 0.0 && hi[0] <= app.doc.job.width && hi[1] <= app.doc.job.height, "{which:?}: {lo:?} {hi:?}");
        }
    }
}
