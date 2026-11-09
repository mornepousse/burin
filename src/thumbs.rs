//! Small drawings of recent projects for the start screen: each file's outlines, read once and
//! kept, scaled into a card. Burin's own.

use crate::geom::P;
use std::path::Path as FilePath;

/// A project's outlines in a 0..1 square, with the sheet's width over its height.
#[derive(Clone, Debug, Default)]
pub struct Thumb {
    pub lines: Vec<Vec<[f32; 2]>>,
    pub aspect: f32,
}

/// The most points kept for one drawing: enough to recognise it, little to draw.
const MOST_POINTS: usize = 4000;

/// The outlines of a saved project, or None when the file can't be read.
pub fn read(path: &FilePath) -> Option<Thumb> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let (lo, hi, paths): (P, P, Vec<Vec<P>>) = match ext.as_str() {
        "burin" => {
            let text = crate::platform::read_to_string(path).ok()?;
            let doc: crate::app::Doc = serde_json::from_str(&text).ok()?;
            let paths = doc.vectors.iter().flat_map(|v| v.paths.iter().map(|p| p.pts.clone())).collect();
            ([0.0, 0.0], [doc.job.width, doc.job.height], paths)
        }
        "crv3d" | "crv" => {
            let bytes = crate::platform::read(path).ok()?;
            let p = crate::crv3d::read(&bytes).ok()?;
            ([p.min[0], p.min[1]], [p.max[0], p.max[1]], p.shapes.into_iter().map(|(_, s)| s.pts).collect())
        }
        _ => return None,
    };
    let (w, h) = (hi[0] - lo[0], hi[1] - lo[1]);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let total: usize = paths.iter().map(|p| p.len()).sum();
    let every = (total / MOST_POINTS).max(1);
    let side = w.max(h);
    let lines = paths
        .iter()
        .filter(|p| p.len() > 1)
        .map(|p| {
            let mut kept: Vec<[f32; 2]> = p.iter().step_by(every).map(|q| [((q[0] - lo[0]) / side) as f32, ((q[1] - lo[1]) / side) as f32]).collect();
            if let (Some(last), Some(end)) = (p.last(), kept.last()) {
                let end_now = [((last[0] - lo[0]) / side) as f32, ((last[1] - lo[1]) / side) as f32];
                if *end != end_now {
                    kept.push(end_now);
                }
            }
            kept
        })
        .collect();
    Some(Thumb { lines, aspect: (w / h) as f32 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_project_gives_its_outlines_in_a_unit_square() {
        let dir = std::env::temp_dir().join(format!("burin-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = crate::app::App::default();
        app.new_file();
        app.doc.job.width = 20.0;
        app.doc.job.height = 10.0;
        app.doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([10.0, 5.0], 4.0, 2.0, 0.0))], crate::app::Prim::Free);
        let file = dir.join("t.burin");
        std::fs::write(&file, serde_json::to_string(&app.doc).unwrap()).unwrap();
        let t = read(&file).unwrap();
        assert_eq!(t.lines.len(), 1);
        assert!((t.aspect - 2.0).abs() < 1e-6);
        assert!(t.lines[0].iter().all(|q| (0.39..=0.61).contains(&q[0]) && (0.19..=0.31).contains(&q[1])), "{:?}", t.lines[0]);
        assert!(read(&dir.join("missing.burin")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
