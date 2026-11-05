//! Clicks and drags through the real interface.

use crate::app::{App, Datum, Job, LeftForm, Prim, RightMode, Screen, Units, Xform};
use egui::{vec2, Pos2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

fn harness() -> Harness<'static, App> {
    let h = Harness::builder().with_size(vec2(1910.0, 1040.0)).with_accessibility_check(false).build_ui_state(|ui, app: &mut App| app.frame(ui), App::default());
    crate::ui::setup_style(&h.ctx);
    h
}

fn click(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    let mid = from + (to - from) * 0.5;
    h.hover_at(mid);
    h.run();
    h.hover_at(to);
    h.run();
    h.drop_at(to);
    h.run();
}

/// Presses and releases at a point.
fn click_at(h: &mut Harness<'static, App>, at: Pos2) {
    h.hover_at(at);
    h.run();
    h.drag_at(at);
    h.drop_at(at);
    h.run();
}

#[test]
fn from_start_screen_to_a_carved_ring() {
    let mut h = harness();
    assert_eq!(h.state().screen, Screen::Start);
    click(&mut h, "Create a new file");
    assert!(matches!(h.state().left, LeftForm::Job { .. }));
    click(&mut h, "OK");
    assert!(matches!(h.state().left, LeftForm::None));

    // Draw a circle by dragging on the sheet.
    click(&mut h, "Draw circle");
    assert!(matches!(h.state().left, LeftForm::Draw(_)));
    let (from, to) = {
        let a = h.state();
        (a.to_screen([5.0, 5.0]), a.to_screen([6.0, 5.0]))
    };
    drag(&mut h, from, to);
    {
        let a = h.state();
        assert_eq!(a.doc.vectors.len(), 1);
        let Prim::Circle { c, r } = a.doc.vectors[0].prim else { panic!("a circle was drawn") };
        assert!((c[0] - 5.0).abs() < 0.03 && (c[1] - 5.0).abs() < 0.03, "centre {c:?}");
        assert!((r - 1.0).abs() < 0.03, "radius {r}");
        assert_eq!(a.selected.len(), 1);
    }
    click(&mut h, "Close");
    assert!(matches!(h.state().left, LeftForm::None));

    // Open the toolpaths panel and make a profile.
    h.get_all_by_label("Toolpaths").last().expect("the side tab").click();
    h.run();
    assert!(h.state().right_open);
    click(&mut h, "Profile toolpath");
    assert!(matches!(h.state().right, RightMode::Op { .. }));
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    assert!(h.state().view3d, "calculating goes to the 3D view");
    assert!(matches!(h.state().right, RightMode::Preview));

    // Preview it: the disc inside the ring stays, the ring goes through.
    h.get_by_label("Preview Selected Toolpath").click();
    h.run_steps(3);
    assert!(h.state().anim.playing, "the preview animates");
    click(&mut h, "Go to end");
    {
        let sim = h.state().sim.as_ref().expect("preview ran");
        assert!(sim.grid.sample(6.125, 5.0) <= -0.49, "cut through");
        assert_eq!(sim.grid.sample(5.0, 5.0), 0.0);
    }
    click(&mut h, "Reset Preview");
    assert!(h.state().sim.is_none());
}

#[test]
fn select_move_and_undo_with_the_mouse() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    click(&mut h, "Draw rectangle");
    let (a, b) = {
        let s = h.state();
        (s.to_screen([2.0, 2.0]), s.to_screen([4.0, 3.0]))
    };
    drag(&mut h, a, b);
    assert_eq!(h.state().doc.vectors.len(), 1);
    click(&mut h, "Close");

    // Click on empty sheet clears, click on the edge selects.
    let empty = h.state().to_screen([8.0, 8.0]);
    click_at(&mut h, empty);
    assert!(h.state().selected.is_empty());
    let edge = h.state().to_screen([3.0, 2.0]);
    click_at(&mut h, edge);
    assert_eq!(h.state().selected.len(), 1);

    // Drag it by one unit to the right.
    let (from, to) = {
        let s = h.state();
        (s.to_screen([3.0, 2.0]), s.to_screen([4.0, 2.0]))
    };
    drag(&mut h, from, to);
    let (lo, _) = h.state().selected_bbox().unwrap();
    assert!((lo[0] - 3.0).abs() < 0.03, "moved to {lo:?}");

    click(&mut h, "Undo");
    let (lo, _) = h.state().selected_bbox().unwrap();
    assert!((lo[0] - 2.0).abs() < 0.03, "undo put it back, {lo:?}");
}

#[test]
fn lines_tool_builds_a_closed_shape() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    click(&mut h, "Draw lines");
    for p in [[2.0, 2.0], [5.0, 2.0], [5.0, 5.0]] {
        let at = h.state().to_screen(p);
        click_at(&mut h, at);
    }
    let start = h.state().to_screen([2.0, 2.0]);
    click_at(&mut h, start);
    let a = h.state();
    assert_eq!(a.doc.vectors.len(), 1, "clicking the first point closes the shape");
    assert!(a.doc.vectors[0].paths[0].closed);
    assert_eq!(a.doc.vectors[0].paths[0].pts.len(), 3);
}

#[test]
fn modeling_tab_makes_a_shape_visible_in_3d() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([5.0, 5.0], 2.0))], Prim::Free);
    h.state_mut().select_all();
    h.run();
    click(&mut h, "Modeling");
    click(&mut h, "Create shape from selection");
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.components.len(), 1);
    assert!(h.state().view3d);
    h.run();
    assert!(h.state().model.is_some());
}

#[test]
fn tabs_clipart_and_layers_work() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    click(&mut h, "Clipart");
    click(&mut h, "Heart");
    assert_eq!(h.state().doc.vectors.len(), 1);
    click(&mut h, "Layers");
    click(&mut h, "Add New Layer ...");
    assert_eq!(h.state().doc.layers.len(), 2);
    assert_eq!(h.state().doc.active_layer, 1);
}

#[test]
fn cancel_on_a_new_file_returns_to_the_start_screen() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "Cancel");
    assert_eq!(h.state().screen, Screen::Start);
}

#[test]
fn view_tools_stay_clear_of_the_right_panel_in_a_narrow_window() {
    let mut h = Harness::builder().with_size(vec2(943.0, 700.0)).with_accessibility_check(false).build_ui_state(|ui, app: &mut App| app.frame(ui), App::default());
    crate::ui::setup_style(&h.ctx);
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    let last = h.get_by_label("Show or hide the right panel").rect();
    assert!(last.right() <= 943.0 - 282.0, "the last tool ends at {}", last.right());
    click(&mut h, "Show grid");
    assert!(h.state().grid);
}

#[test]
fn every_toolpath_form_opens_and_explains_what_it_needs() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    for label in [
        "Profile toolpath",
        "Pocket toolpath",
        "Drill toolpath",
        "Quick engrave toolpath",
        "V-Carve toolpath",
        "Prism carve toolpath",
        "Moulding toolpath",
        "3D roughing toolpath",
        "3D finishing toolpath",
    ] {
        click(&mut h, label);
        assert!(matches!(h.state().right, RightMode::Op { .. }), "{label}");
        click(&mut h, "Calculate");
        assert!(matches!(&h.state().right, RightMode::Op { error: Some(_), .. }), "{label} needs something first");
        // Close appears once the left form is shut.
        h.get_all_by_label("Close").last().unwrap().click();
        h.run();
        assert!(matches!(h.state().right, RightMode::List), "{label}");
    }
}

#[test]
fn texture_needs_no_selection_and_save_form_lists_it() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    click(&mut h, "Texture toolpath");
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    assert_eq!(h.state().doc.toolpaths[0].op.name, "Texture 1");
    h.state_mut().right = RightMode::Save;
    h.run();
    h.get_by_label("Save Toolpath(s) ...");
    assert!(h.state().gcode_text(None).unwrap().contains("Toolpath: Texture 1"));
}

#[test]
fn transform_forms_open_from_the_panel_and_apply() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([3.0, 3.0], 2.0, 2.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.run();
    for label in ["Move", "Set size", "Rotate", "Mirror", "Alignment tools", "Offset", "Array copy", "Circular copy", "Copy along vectors", "Nest parts", "Join vectors", "Create fillets", "Trim lines", "Vector validator", "Dimensions", "Text on a curve"] {
        click(&mut h, label);
        assert!(matches!(h.state().left, LeftForm::Xform(_)), "{label}");
        h.get_all_by_label("Close").next().unwrap().click();
        h.run();
        assert!(matches!(h.state().left, LeftForm::None), "{label} closes");
    }
    click(&mut h, "Mirror");
    click(&mut h, "Right");
    let (lo, _) = h.state().selected_bbox().unwrap();
    assert!((lo[0] - 4.0).abs() < 1e-9, "mirrored across the right edge: {lo:?}");
}

#[test]
fn fillet_form_rounds_the_clicked_corner() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
    h.run();
    click(&mut h, "Create fillets");
    let corner = h.state().to_screen([3.0, 3.0]);
    click_at(&mut h, corner);
    assert!(h.state().doc.vectors[0].paths[0].pts.len() > 10);
}

#[test]
fn the_tool_list_chooses_a_tool_for_the_form() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    click(&mut h, "Profile toolpath");
    click(&mut h, "Select ...");
    assert!(h.state().tools_open);
    click(&mut h, "End Mill (0.5 inch)");
    click(&mut h, "Use This Tool");
    assert!(!h.state().tools_open);
    match &h.state().right {
        RightMode::Op { op, .. } => assert_eq!(op.tool.label(), "End Mill (0.5 inch)"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn material_setup_cancel_restores_values() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    click(&mut h, "Set ...");
    assert!(matches!(h.state().right, RightMode::Material(_)));
    h.state_mut().doc.job.thickness = 2.0;
    h.run();
    click(&mut h, "Cancel");
    assert_eq!(h.state().doc.job.thickness, 0.5);
}

#[test]
fn all_holes_of_one_size_can_be_resized_from_the_form() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    for x in [2.0, 4.0, 6.0] {
        let (c, r) = ([x, 3.0], 0.08);
        h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, r))], Prim::Circle { c, r });
    }
    let (c, r) = ([5.0, 7.0], 1.0);
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, r))], Prim::Circle { c, r });
    h.run();
    click(&mut h, "Modify several shapes");
    click(&mut h, "3 x  Circle, diameter 0.16 inches");
    assert_eq!(h.state().selected.len(), 3);
    match &mut h.state_mut().left {
        LeftForm::Xform(f) => {
            assert!((f.bulk_d - 0.16).abs() < 1e-3, "starts from their size");
            f.bulk_d = 0.25;
        }
        other => panic!("{other:?}"),
    }
    h.run();
    click(&mut h, "Apply to All Selected");
    let radii: Vec<f64> = h.state().doc.vectors.iter().map(|v| crate::bulk::as_circle(&v.paths[0]).unwrap().1).collect();
    assert!((radii[0] - 0.125).abs() < 1e-3 && (radii[2] - 0.125).abs() < 1e-3, "{radii:?}");
    assert!((radii[3] - 1.0).abs() < 1e-3, "the big circle is left alone");
}

#[test]
fn a_rule_is_added_by_clicking_on_the_sheet() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    for (x, r) in [(3.0, 0.5), (7.0, 1.0)] {
        let c = [x, 5.0];
        h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, r))], Prim::Circle { c, r });
    }
    h.run();
    click(&mut h, "Drawing constraints");
    click(&mut h, "Concentric");
    let (a, b) = (h.state().to_screen([3.0, 5.0]), h.state().to_screen([7.0, 5.0]));
    click_at(&mut h, a);
    assert_eq!(h.state().picks.len(), 1);
    click_at(&mut h, b);
    assert_eq!(h.state().doc.constraints.len(), 1);
    let centres: Vec<_> = h.state().doc.vectors.iter().map(|v| crate::bulk::as_circle(&v.paths[0]).unwrap().0).collect();
    assert!((centres[0][0] - centres[1][0]).hypot(centres[0][1] - centres[1][1]) < 1e-3, "{centres:?}");
    click(&mut h, "Remove rule 1");
    assert!(h.state().doc.constraints.is_empty());
}

#[test]
fn dragging_a_shape_keeps_its_rule() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let mut ids = Vec::new();
    for (x, r) in [(3.0, 0.5), (7.0, 1.0)] {
        let c = [x, 5.0];
        ids.push(h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, r))], Prim::Circle { c, r }));
    }
    use crate::constraints::{Elem, Kind};
    assert!(h.state_mut().add_constraint(Kind::Concentric, vec![Elem::Round { v: ids[0] }, Elem::Round { v: ids[1] }]));
    h.run();
    let centre = crate::bulk::as_circle(&h.state().doc.vectors[1].paths[0]).unwrap().0;
    let (r, _) = (crate::bulk::as_circle(&h.state().doc.vectors[1].paths[0]).unwrap().1, 0);
    let (from, to) = (h.state().to_screen([centre[0] + r, centre[1]]), h.state().to_screen([centre[0] + r + 2.0, centre[1] - 1.0]));
    click_at(&mut h, from);
    h.state_mut().selected = vec![ids[1]];
    drag(&mut h, from, to);
    let centres: Vec<_> = h.state().doc.vectors.iter().map(|v| crate::bulk::as_circle(&v.paths[0]).unwrap().0).collect();
    assert!((centres[1][0] - centre[0] - 2.0).abs() < 0.1, "the dragged circle went where it was dragged: {centres:?}");
    assert!((centres[0][0] - centres[1][0]).hypot(centres[0][1] - centres[1][1]) < 1e-3, "and the other followed: {centres:?}");
}

#[test]
fn the_grid_button_turns_on_a_grid_with_real_contrast() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    assert!(!h.state().grid);
    click(&mut h, "Show grid");
    assert!(h.state().grid);
    let (fine, bold) = h.state().grid_steps();
    assert!(fine > 0.0 && (bold / fine - 5.0).abs() < 1e-9);
    assert!(fine * h.state().scale as f64 >= 16.0, "fine lines are far enough apart to see");
    let gap = |a: egui::Color32, b: egui::Color32| (a.r() as i32 - b.r() as i32).abs();
    use crate::theme::*;
    assert!(gap(GRID_FINE, SHEET) >= 15, "fine lines stand out from the sheet");
    assert!(gap(GRID_BOLD, SHEET) >= 40, "bold lines stand out more");
    assert!(gap(GRID_BOLD_DARK, WORKSPACE) >= 20, "and around the sheet too");
    click(&mut h, "Show grid");
    assert!(!h.state().grid);
}

#[test]
fn smart_dimension_with_clicks_then_type_a_value() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[2.0, 5.0], [6.0, 5.0]])], Prim::Free);
    h.run();
    click(&mut h, "Smart dimension");
    let (on, off) = (h.state().to_screen([4.0, 5.0]), h.state().to_screen([4.0, 8.0]));
    click_at(&mut h, on);
    assert_eq!(h.state().picks.len(), 1);
    click_at(&mut h, off);
    assert_eq!(h.state().doc.constraints.len(), 1);
    assert!(h.state().dim_edit.is_some());
    h.state_mut().dim_text = "7".into();
    h.run();
    click(&mut h, "Set Value");
    assert!(h.state().dim_edit.is_none());
    h.run();
    let v = h.state().doc.vectors.iter().find(|v| v.id == id).unwrap();
    let (a, b) = (v.paths[0].pts[0], v.paths[0].pts[1]);
    assert!(((b[0] - a[0]).hypot(b[1] - a[1]) - 7.0).abs() < 1e-3);
}

#[test]
fn double_clicking_a_dimension_reopens_its_value_and_dragging_moves_its_label() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[2.0, 5.0], [6.0, 5.0]])], Prim::Free);
    use crate::constraints::{DimMode, Elem};
    assert!(h.state_mut().make_dimension(&[Elem::Seg { v: id, path: 0, seg: 0 }], DimMode::Smart));
    h.state_mut().dim_edit = None;
    h.run();
    let label = h.state().dim_parts(&h.state().doc.constraints[0]).unwrap().label;
    h.hover_at(label);
    h.run();
    drag(&mut h, label, label + vec2(0.0, -40.0));
    let moved = h.state().dim_parts(&h.state().doc.constraints[0]).unwrap().label;
    assert!((moved.y - (label.y - 40.0)).abs() < 2.0, "{label:?} -> {moved:?}");
    h.state_mut().open_dimension(0);
    assert_eq!(h.state().dim_edit, Some(0));
    assert_eq!(h.state().dim_text, "4.0");
}

#[test]
fn pick_things_first_then_only_fitting_relations_apply() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let a = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 2.0], [5.0, 2.0]])], Prim::Free);
    let b = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 6.0], [5.0, 8.0]])], Prim::Free);
    h.run();
    click(&mut h, "Drawing constraints");
    let (p, q) = (h.state().to_screen([3.0, 2.0]), h.state().to_screen([3.0, 7.0]));
    click_at(&mut h, p);
    click_at(&mut h, q);
    assert_eq!(h.state().picks.len(), 2);
    click(&mut h, "Parallel");
    assert_eq!(h.state().doc.constraints.len(), 1);
    assert!(h.state().picks.is_empty());
    let dir = |id| {
        let s = h.state();
        let v = s.doc.vectors.iter().find(|v| v.id == id).unwrap();
        let (a, b) = (v.paths[0].pts[0], v.paths[0].pts[1]);
        (b[1] - a[1]).atan2(b[0] - a[0])
    };
    assert!((dir(a) - dir(b)).abs() < 1e-3);
}

#[test]
fn normal_and_construction_can_be_switched_from_the_panel() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let (c, r) = ([5.0, 5.0], 1.0);
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, r))], Prim::Circle { c, r });
    h.state_mut().selected = vec![id];
    h.run();
    click(&mut h, "Normal / construction");
    assert!(h.state().doc.vectors[0].construction);
    click(&mut h, "Normal / construction");
    assert!(!h.state().doc.vectors[0].construction);
}

#[test]
fn a_loaded_post_processor_shows_in_the_save_form_with_its_limits() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let def = crate::postp::PostDef::parse(
        "POST_NAME = \"Shop Router\"\nFILE_EXTENSION = \"tap\"\nSUBSTITUTE = \"x\"\nbegin RAPID_MOVE\n\"G0\"\nbegin FEED_MOVE\n\"G1\"\n",
        &|_| None,
    )
    .unwrap();
    h.state_mut().loaded_posts.push(def);
    h.state_mut().post_index = crate::toolpath::POSTS.len();
    h.state_mut().right_open = true;
    h.state_mut().right = RightMode::Save;
    h.run();
    h.get_by_label("Load .pp Files ...");
    h.get_by_label("Load a Folder ...");
    h.get_by_label("Writes .tap files in the job's unit (inches).");
    h.get_by_label("Arcs are written as short straight lines.");
    h.get_by_label("SUBSTITUTE is not applied.");
    click(&mut h, "Remove This One");
    assert!(h.state().loaded_posts.is_empty());
    assert_eq!(h.state().post_index, 0);
}

#[test]
fn trimming_a_ruled_shape_with_the_mouse_leaves_it_in_place() {
    use crate::constraints::{DimMode, Elem, Kind};
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let plate = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 6.0, 4.0, 0.0))], Prim::Rect { c: [5.0, 5.0], w: 6.0, h: 4.0, corner: 0.0 });
    h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[5.0, 1.0], [5.0, 9.0]])], Prim::Free);
    let s = h.state_mut();
    assert!(s.add_constraint(Kind::Fixed, vec![Elem::Node { v: plate, path: 0, node: 0 }]));
    assert!(s.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 0 }], DimMode::Smart));
    assert!(s.make_dimension(&[Elem::Seg { v: plate, path: 0, seg: 1 }], DimMode::Smart));
    s.dim_edit = None;
    h.run();
    click(&mut h, "Trim lines");
    // Click the top side, right of the crossing line, away from any dimension label.
    let at = h.state().to_screen([7.0, 7.0]);
    click_at(&mut h, at);
    h.get_all_by_label("Close").next().unwrap().click();
    h.run();
    h.run();
    let a = h.state();
    assert!(!a.constraint_conflict, "{}", a.status);
    let v = a.doc.vectors.iter().find(|v| v.id == plate).unwrap();
    let pts: Vec<[f64; 2]> = v.paths.iter().flat_map(|p| p.pts.iter().copied()).collect();
    assert!(pts.iter().any(|p| p[0] == 2.0 && p[1] == 7.0), "the far corner stays: {pts:?}");
    assert!(!pts.iter().any(|p| p[0] == 8.0 && p[1] == 7.0), "the trimmed corner is gone: {pts:?}");
    for p in &pts {
        let on_x = (p[0] - 2.0).abs() < 1e-3 || (p[0] - 8.0).abs() < 1e-3;
        let on_y = (p[1] - 3.0).abs() < 1e-3 || (p[1] - 7.0).abs() < 1e-3;
        assert!(on_x || on_y, "a point left the outline: {p:?}");
    }
}

#[test]
fn the_value_box_explains_a_refused_value_and_stays_open() {
    use crate::constraints::{DimMode, Elem, Kind};
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let l = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[2.0, 5.0], [6.0, 5.0]])], Prim::Free);
    let s = h.state_mut();
    assert!(s.add_constraint(Kind::Fixed, vec![Elem::Node { v: l, path: 0, node: 0 }]));
    assert!(s.make_dimension(&[Elem::Seg { v: l, path: 0, seg: 0 }], DimMode::Smart));
    assert!(s.add_constraint(Kind::Fixed, vec![Elem::Node { v: l, path: 0, node: 1 }]));
    s.dim_text = "9".into();
    h.run();
    click(&mut h, "Set Value");
    assert!(h.state().dim_edit.is_some(), "the box stays open");
    assert!(h.state().dim_error.as_deref().unwrap_or("").contains("can't be 9"), "{:?}", h.state().dim_error);
    click(&mut h, "Keep As Is");
    assert!(h.state().dim_edit.is_none());
    assert!(h.state().dim_error.is_none());
    assert!(!h.state().constraint_conflict);
}

/// Lines that cross one tall line, ready to be trimmed from the Trim form.
fn trim_setup() -> Harness<'static, App> {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let line = |p: [f64; 2], q: [f64; 2]| crate::geom::Path::open(vec![p, q]);
    for y in [3.0, 5.0, 7.0] {
        h.state_mut().doc.add_vector(vec![line([1.0, y], [9.0, y])], Prim::Free);
    }
    h.state_mut().doc.add_vector(vec![line([5.0, 1.0], [5.0, 9.0])], Prim::Free);
    h.run();
    click(&mut h, "Trim lines");
    h
}

fn right_ends(h: &Harness<'static, App>) -> Vec<f64> {
    h.state().doc.vectors[..3].iter().map(|v| v.paths.iter().flat_map(|p| p.pts.iter().map(|q| q[0])).fold(0.0, f64::max)).collect()
}

#[test]
fn holding_the_button_down_on_a_line_trims_it_straight_away() {
    let mut h = trim_setup();
    let at = h.state().to_screen([7.0, 5.0]);
    h.hover_at(at);
    h.run();
    h.drag_at(at);
    h.step();
    h.step();
    assert_eq!(right_ends(&h)[1], 5.0, "trimmed while the button is still down");
    // Staying down a long time changes nothing more, and letting go adds nothing.
    h.run_steps(12);
    h.drop_at(at);
    h.run();
    assert_eq!(right_ends(&h), vec![9.0, 5.0, 9.0]);
    assert_eq!(h.state().undo.len(), 1);
}

#[test]
fn sweeping_with_the_button_down_trims_every_line_crossed_and_undoes_as_one() {
    let mut h = trim_setup();
    let (from, to) = (h.state().to_screen([7.0, 2.0]), h.state().to_screen([7.0, 8.0]));
    let before = h.state().undo.len();
    drag(&mut h, from, to);
    assert_eq!(right_ends(&h), vec![5.0, 5.0, 5.0], "all three were swept");
    assert_eq!(h.state().undo.len(), before + 1, "one sweep is one undo");
    h.state_mut().do_undo();
    h.run();
    assert_eq!(right_ends(&h), vec![9.0, 9.0, 9.0]);
}

#[test]
fn a_quick_click_still_trims_exactly_once() {
    let mut h = trim_setup();
    let at = h.state().to_screen([7.0, 5.0]);
    click_at(&mut h, at);
    assert_eq!(right_ends(&h), vec![9.0, 5.0, 9.0]);
    assert_eq!(h.state().undo.len(), 1);
    // The piece that is left isn't touched by a click on empty sheet.
    let empty = h.state().to_screen([7.0, 6.0]);
    click_at(&mut h, empty);
    assert_eq!(right_ends(&h), vec![9.0, 5.0, 9.0]);
}

#[test]
fn a_sweep_that_starts_on_a_dimension_label_still_trims() {
    use crate::constraints::{DimMode, Elem};
    let mut h = trim_setup();
    let id = h.state().doc.vectors[0].id;
    assert!(h.state_mut().make_dimension(&[Elem::Seg { v: id, path: 0, seg: 0 }], DimMode::Smart));
    h.state_mut().dim_edit = None;
    h.run();
    let label = h.state().dim_parts(&h.state().doc.constraints[0]).unwrap().label;
    let to = h.state().to_screen([5.0 + 3.0, 5.0 - 3.0]);
    let place_before = h.state().doc.constraints[0].place;
    drag(&mut h, label, to);
    assert_eq!(h.state().doc.constraints[0].place, place_before, "the label didn't move");
    assert_eq!(right_ends(&h)[0], 5.0, "and the line it crossed was trimmed");
}

#[test]
fn a_long_press_counts_as_a_click_for_the_other_sheet_tools() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
    h.run();
    click(&mut h, "Create fillets");
    let corner = h.state().to_screen([3.0, 3.0]);
    h.hover_at(corner);
    h.run();
    h.drag_at(corner);
    // Let far more than the usual click time go by while the button is down.
    std::thread::sleep(std::time::Duration::from_millis(5));
    h.run_steps(30);
    h.drop_at(corner);
    h.run();
    assert!(h.state().doc.vectors[0].paths[0].pts.len() > 10, "the fillet happened");
}

#[test]
fn the_trace_form_walks_from_nothing_to_vectors() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    click(&mut h, "Trace bitmap");
    assert!(matches!(h.state().left, LeftForm::Trace(_)));
    h.get_by_label("Import a picture to fit vectors to.");
    let (w, size) = (60usize, 60usize);
    let mut rgba = vec![255u8; w * size * 4];
    for y in 15..45 {
        for x in 15..45 {
            let i = (y * w + x) * 4;
            rgba[i..i + 3].copy_from_slice(&[0, 0, 0]);
        }
    }
    h.state_mut().place_bitmap("square".into(), "square".into(), crate::bitmap::Image { w, h: size, rgba: std::sync::Arc::new(rgba) });
    h.run();
    click(&mut h, "Preview");
    assert_eq!(h.state().trace_preview.len(), 1);
    assert!(h.state().doc.vectors.is_empty(), "a preview draws nothing yet");
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.vectors.len(), 1);
    assert!(h.state().doc.vectors[0].paths[0].closed);
    h.get_all_by_label("Close").next().unwrap().click();
    h.run();
    assert!(matches!(h.state().left, LeftForm::None));
    assert!(h.state().trace_preview.is_empty());
}

#[test]
fn dragging_a_distort_corner_reshapes_the_object() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.run();
    click(&mut h, "Distort object");
    h.get_by_label("Selected vectors are valid.");
    click(&mut h, "Apply");
    assert!(h.state().envelope.is_some());
    let (from, to) = (h.state().to_screen([7.0, 7.0]), h.state().to_screen([9.0, 8.0]));
    drag(&mut h, from, to);
    let (_, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - 9.0).abs() < 0.05 && (hi[1] - 8.0).abs() < 0.05, "{hi:?}");
    click(&mut h, "Bake Distortion");
    assert!(h.state().envelope.is_none());
    let (_, still) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((still[0] - 9.0).abs() < 0.05, "the new form stays");
}

/// A started job with one square, nothing else open.
fn job_with_square(at: [f64; 2]) -> Harness<'static, App> {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect(at, 2.0, 2.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.run();
    h
}

#[test]
fn the_job_can_be_changed_from_the_drawing_tab_and_undone() {
    let mut h = job_with_square([5.0, 5.0]);
    let steps = h.state().undo.len();
    click(&mut h, "Job Setup...");
    assert!(matches!(h.state().left, LeftForm::Job { first: false, .. }));
    h.state_mut().doc.job.width = 20.0;
    h.run();
    click(&mut h, "Datum Centre");
    assert_eq!(h.state().doc.job.datum, Datum::Centre);
    click(&mut h, "OK");
    assert!(matches!(h.state().left, LeftForm::None));
    assert_eq!(h.state().undo.len(), steps + 1, "one step of undo");
    assert_eq!(h.state().doc.job.origin(), [10.0, 5.0]);
    h.state_mut().do_undo();
    assert_eq!(h.state().doc.job.width, 10.0);
    assert_eq!(h.state().doc.job.datum, Datum::BottomLeft);
    h.state_mut().do_redo();
    assert_eq!(h.state().doc.job.width, 20.0);
}

#[test]
fn the_job_icon_opens_the_same_form_and_cancel_goes_all_the_way_back() {
    let mut h = job_with_square([5.0, 5.0]);
    let steps = h.state().undo.len();
    click(&mut h, "Job Setup...");
    h.state_mut().doc.job.width = 30.0;
    // Asking again while it is open must not forget where it started.
    h.state_mut().open_job_setup();
    h.run();
    click(&mut h, "Cancel");
    assert_eq!(h.state().doc.job.width, 10.0);
    assert_eq!(h.state().undo.len(), steps, "nothing changed, nothing to undo");
    click(&mut h, "Job Setup...");
    click(&mut h, "OK");
    assert_eq!(h.state().undo.len(), steps, "OK with no change adds no undo step");
}

#[test]
fn job_setup_opens_even_if_the_left_panel_was_hidden() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().left_open = false;
    h.run();
    h.state_mut().open_job_setup();
    h.run();
    assert!(h.state().left_open);
    h.get_by_label("OK");
}

#[test]
fn shrinking_the_job_says_when_shapes_end_up_outside() {
    let mut h = job_with_square([8.0, 8.0]);
    h.state_mut().open_job_setup();
    h.state_mut().doc.job.width = 4.0;
    h.run();
    click(&mut h, "OK");
    assert!(h.state().status.contains("outside the job"), "{}", h.state().status);
}

#[test]
fn positions_are_shown_and_typed_from_the_chosen_origin() {
    let mut h = job_with_square([8.0, 8.0]);
    h.state_mut().doc.job.datum = Datum::Centre;
    h.run();
    click(&mut h, "Move");
    // The square is at 8,8 on the sheet: 3,3 from the middle of a 10 wide job.
    assert!(h.get_all_by_value("3.0").count() >= 2, "both fields show 3");
    let LeftForm::Xform(f) = &h.state().left else { panic!("the move form is open") };
    assert_eq!((f.dx, f.dy), (8.0, 8.0), "it is still kept as a place on the sheet");
    h.get_all_by_value("3.0").next().unwrap().focus();
    h.run();
    h.get_all_by_value("3.0").next().unwrap().type_text("1");
    h.run();
    let LeftForm::Xform(f) = &h.state().left else { panic!("the move form is open") };
    // "3.0" became "3.01" from the origin, which is 8.01 on the sheet.
    assert!((f.dx - 8.01).abs() < 1e-9, "typed value counts from the origin: {}", f.dx);
    assert_eq!(f.dy, 8.0);
}

#[test]
fn an_svg_file_comes_in_selected_and_at_its_real_size() {
    let dir = std::env::temp_dir().join(format!("burin-svg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plate.svg");
    std::fs::write(&file, r#"<svg width="2in" height="2in" viewBox="0 0 2 2"><rect x="0.5" y="0.5" width="1" height="1"/><text>hi</text></svg>"#).unwrap();
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let steps = h.state().undo.len();
    h.state_mut().import_drawing(&file).unwrap();
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 1);
    assert_eq!(h.state().selected.len(), 1);
    assert_eq!(h.state().undo.len(), steps + 1);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - lo[0] - 1.0).abs() < 1e-6 && (lo[1] - 0.5).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(h.state().status.contains("1 objects were left out"), "{}", h.state().status);
    let bad = dir.join("empty.svg");
    std::fs::write(&bad, r#"<svg width="2in" height="2in" viewBox="0 0 2 2"/>"#).unwrap();
    assert!(h.state_mut().import_drawing(&bad).unwrap_err().contains("no lines"));
    assert!(h.state_mut().import_drawing(&dir.join("gone.svg")).unwrap_err().contains("Could not read"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_relief_form_goes_from_picture_to_3d_shape() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let img = crate::bitmap::Image { w: 4, h: 4, rgba: std::sync::Arc::new(vec![200; 64]) };
    h.state_mut().open_relief_picture("wood.png".into(), &img);
    h.run();
    h.get_by_label("Dark is high (lithophane)");
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.components.len(), 1);
    assert!(h.state().view3d);
    assert!(matches!(h.state().left, LeftForm::None));
}

#[test]
fn closing_the_relief_form_adds_nothing() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let img = crate::bitmap::Image { w: 4, h: 4, rgba: std::sync::Arc::new(vec![200; 64]) };
    h.state_mut().open_relief_picture("wood.png".into(), &img);
    h.run();
    click(&mut h, "Close");
    assert!(h.state().doc.components.is_empty());
    assert!(matches!(h.state().left, LeftForm::None));
}

#[test]
fn a_flute_and_a_chamfer_calculate_from_the_toolpath_panel() {
    use crate::toolpath::OpKind;
    for kind in [OpKind::Flute, OpKind::Chamfer] {
        let mut h = harness();
        click(&mut h, "Create a new file");
        click(&mut h, "OK");
        let line = crate::geom::Path::open(vec![[2.0, 2.0], [8.0, 8.0]]);
        let id = h.state_mut().doc.add_vector(vec![line], Prim::Free);
        h.state_mut().selected = vec![id];
        h.state_mut().start_op(kind);
        h.run();
        click(&mut h, "Calculate");
        assert_eq!(h.state().doc.toolpaths.len(), 1, "{kind:?}");
        assert!(!h.state().doc.toolpaths[0].moves.is_empty());
    }
}

#[test]
fn the_new_toolpath_icons_open_their_forms() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    click(&mut h, "Fluting toolpath");
    assert!(matches!(&h.state().right, RightMode::Op { op, .. } if op.kind == crate::toolpath::OpKind::Flute));
    h.state_mut().right = RightMode::List;
    h.run();
    click(&mut h, "Chamfer toolpath");
    assert!(matches!(&h.state().right, RightMode::Op { op, .. } if op.kind == crate::toolpath::OpKind::Chamfer));
}

#[test]
fn a_line_drawn_from_a_corner_starts_exactly_on_it() {
    let mut h = job_with_square([3.3, 3.3]);
    h.state_mut().snap = false;
    h.state_mut().selected.clear();
    h.run();
    click(&mut h, "Draw circle");
    // Press a few pixels off the square's corner.
    let at = h.state().to_screen([2.3, 2.3]) + vec2(3.0, -3.0);
    let to = at + vec2(120.0, -40.0);
    drag(&mut h, at, to);
    let LeftForm::Draw(f) = &h.state().left else { panic!("the form stays open") };
    assert_eq!(h.state().doc.vectors.len(), 2);
    let _ = f;
    let v = h.state().doc.vectors.last().unwrap();
    // A circle is drawn from its centre, which landed on the corner.
    let (c, _) = crate::bulk::as_circle(&v.paths[0]).unwrap();
    assert!((c[0] - 2.3).abs() < 1e-3 && (c[1] - 2.3).abs() < 1e-3, "{c:?}");
}

#[test]
fn a_shape_dragged_by_its_corner_lands_on_another_shapes_corner() {
    let mut h = job_with_square([3.3, 3.3]);
    h.state_mut().snap = false;
    let other = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([7.7, 7.7], 1.0, 1.0, 0.0))], Prim::Free);
    let _ = other;
    h.run();
    let from = h.state().to_screen([4.3, 4.3]) + vec2(-2.0, 2.0);
    let to = h.state().to_screen([7.2, 7.2]) + vec2(3.0, -3.0);
    drag(&mut h, from, to);
    let (_, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - 7.2).abs() < 1e-3 && (hi[1] - 7.2).abs() < 1e-3, "{hi:?}");
}

#[test]
fn guides_come_out_of_the_rulers_and_go_back_in() {
    let mut h = job_with_square([3.3, 3.3]);
    h.state_mut().snap = false;
    h.state_mut().selected.clear();
    h.run();
    let steps = h.state().undo.len();
    let at = h.state().to_screen([7.0, 7.0]);
    let ruler = egui::pos2(at.x, h.state().view_rect.top() - 10.0);
    drag(&mut h, ruler, at);
    assert_eq!(h.state().doc.guides.len(), 1);
    let g = h.state().doc.guides[0];
    assert!(!g.vertical, "pulled down from the top ruler it runs across");
    assert!((g.at - 7.0).abs() < 0.03, "{g:?}");
    assert_eq!(h.state().undo.len(), steps + 1);
    // Picked up again, moved.
    let on = h.state().to_screen([8.5, g.at]);
    let to = h.state().to_screen([8.5, 8.0]);
    drag(&mut h, on, to);
    assert_eq!(h.state().doc.guides.len(), 1);
    assert!((h.state().doc.guides[0].at - 8.0).abs() < 0.03);
    // Dropped back on the ruler, gone; undo brings it back.
    let on = h.state().to_screen([8.5, 8.0]);
    let off = egui::pos2(on.x, h.state().view_rect.top() - 10.0);
    drag(&mut h, on, off);
    assert!(h.state().doc.guides.is_empty());
    h.state_mut().do_undo();
    assert_eq!(h.state().doc.guides.len(), 1);
}

#[test]
fn the_side_ruler_gives_an_upright_guide_and_the_toolbar_switch_works() {
    let mut h = job_with_square([3.3, 3.3]);
    h.state_mut().snap = false;
    h.run();
    let at = h.state().to_screen([7.0, 6.0]);
    let ruler = egui::pos2(h.state().view_rect.left() - 10.0, at.y);
    drag(&mut h, ruler, at);
    assert!(h.state().doc.guides[0].vertical);
    assert!(h.state().snap_shapes);
    click(&mut h, "Snap to shapes");
    assert!(!h.state().snap_shapes);
}

#[test]
fn the_save_form_offers_arcs_and_the_file_uses_them() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([5.0, 5.0], 2.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::Save;
    h.state_mut().right_open = true;
    h.run();
    assert!(!h.state().output_arcs, "off until asked for");
    let plain = h.state().gcode_text(None).unwrap();
    assert!(!plain.contains("G3") && !plain.contains("G2 "));
    click(&mut h, "Write curves as arcs (G2 / G3)");
    assert!(h.state().output_arcs);
    let text = h.state().gcode_text(None).unwrap();
    assert!(text.lines().any(|l| l.starts_with("G2 ") || l.starts_with("G3 ")), "{text}");
    assert!(text.len() < plain.len() / 2, "{} vs {}", text.len(), plain.len());
}

/// A 2 by 2 square at the middle of the job, and a long line above it.
fn box_setup() -> (Harness<'static, App>, u64, u64) {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().snap = false;
    h.state_mut().selected.clear();
    let square = h.state().doc.vectors[0].id;
    let line = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 8.0], [9.0, 8.0]])], Prim::Free);
    h.run();
    (h, square, line)
}

fn box_drag(h: &mut Harness<'static, App>, from: [f64; 2], to: [f64; 2]) {
    let (a, b) = (h.state().to_screen(from), h.state().to_screen(to));
    drag(h, a, b);
}

#[test]
fn left_to_right_takes_only_what_is_wholly_inside() {
    let (mut h, square, _) = box_setup();
    box_drag(&mut h, [3.5, 3.5], [5.0, 7.0]);
    assert!(h.state().selected.is_empty(), "half of the square is outside the box");
    box_drag(&mut h, [3.5, 3.5], [6.5, 7.0]);
    assert_eq!(h.state().selected, vec![square]);
}

#[test]
fn right_to_left_also_takes_what_it_touches() {
    let (mut h, square, _) = box_setup();
    box_drag(&mut h, [5.0, 7.0], [3.5, 3.5]);
    assert_eq!(h.state().selected, vec![square], "the box only reaches part of the square");
}

#[test]
fn only_left_and_right_decide_not_up_and_down() {
    let (mut h, square, _) = box_setup();
    box_drag(&mut h, [5.0, 3.5], [3.5, 7.0]);
    assert_eq!(h.state().selected, vec![square], "right to left, going up");
    box_drag(&mut h, [3.5, 7.0], [5.0, 3.5]);
    assert!(h.state().selected.is_empty(), "left to right, going down");
}

#[test]
fn a_line_passing_through_the_box_counts_as_touching_with_its_ends_outside() {
    let (mut h, _, line) = box_setup();
    box_drag(&mut h, [6.0, 8.5], [5.0, 7.5]);
    assert_eq!(h.state().selected, vec![line]);
    box_drag(&mut h, [5.0, 8.5], [6.0, 7.5]);
    assert!(h.state().selected.is_empty(), "left to right, the line isn't inside");
}

#[test]
fn a_box_from_inside_a_shape_selects_what_is_within_and_leaves_the_outer_shape_alone() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().snap = false;
    h.state_mut().selected.clear();
    let panel = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 8.0, 8.0, 0.0))], Prim::Free);
    let hole = h.state().doc.vectors[0].id;
    h.run();
    let before = crate::geom::bbox(&h.state().doc.vectors[1].paths).unwrap();
    // Starting inside the panel, in both directions, with its outline out of reach.
    box_drag(&mut h, [7.0, 7.0], [3.5, 3.5]);
    assert_eq!(h.state().selected, vec![hole]);
    assert_eq!(crate::geom::bbox(&h.state().doc.vectors[1].paths).unwrap(), before, "the panel didn't move");
    assert!(!h.state().selected.contains(&panel));
    h.state_mut().selected.clear();
    h.run();
    box_drag(&mut h, [3.5, 3.5], [7.0, 7.0]);
    assert_eq!(h.state().selected, vec![hole]);
    // A click inside it still selects it, and once selected it can be dragged from inside.
    h.state_mut().selected = vec![panel];
    h.run();
    box_drag(&mut h, [2.0, 2.0], [2.5, 2.0]);
    let after = crate::geom::bbox(&h.state().doc.vectors[1].paths).unwrap();
    assert!((after.0[0] - before.0[0] - 0.5).abs() < 0.05, "{after:?}");
}

#[test]
fn locked_and_hidden_layers_stay_out_and_shift_adds_without_repeating() {
    let (mut h, square, line) = box_setup();
    box_drag(&mut h, [3.5, 3.5], [6.5, 6.5]);
    assert_eq!(h.state().selected, vec![square]);
    h.state_mut().doc.layers[0].locked = true;
    h.state_mut().selected.clear();
    h.run();
    box_drag(&mut h, [0.5, 0.5], [9.5, 9.5]);
    assert!(h.state().selected.is_empty(), "locked");
    h.state_mut().doc.layers[0].locked = false;
    h.state_mut().doc.layers[0].visible = false;
    h.run();
    box_drag(&mut h, [9.5, 9.5], [0.5, 0.5]);
    assert!(h.state().selected.is_empty(), "hidden");
    h.state_mut().doc.layers[0].visible = true;
    h.state_mut().selected = vec![square];
    h.run();
    let (a, b) = (h.state().to_screen([3.5, 3.5]), h.state().to_screen([6.5, 6.5]));
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    h.run();
    drag(&mut h, a, b);
    assert_eq!(h.state().selected, vec![square], "already selected: not listed twice");
    // Shift keeps what was selected while adding more.
    box_drag(&mut h, [0.5, 7.5], [9.5, 8.5]);
    assert_eq!(h.state().selected, vec![square, line]);
}

/// A started job with one shape selected.
fn job_with_shape(prim: Prim) -> (Harness<'static, App>, u64) {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    h.state_mut().selected = vec![id];
    h.run();
    (h, id)
}

#[test]
fn clicking_a_tool_with_its_kind_of_shape_selected_edits_it() {
    let text = Prim::Text { at: [2.0, 2.0], height: 0.8, text: "Ab".into() };
    let cases = [
        ("Draw circle", Prim::Circle { c: [3.0, 4.0], r: 1.5 }, "Edit Circle"),
        ("Draw ellipse", Prim::Ellipse { c: [3.0, 4.0], rx: 2.0, ry: 1.0 }, "Edit Ellipse"),
        ("Draw rectangle", Prim::Rect { c: [3.0, 4.0], w: 3.0, h: 2.0, corner: 0.25 }, "Edit Rectangle"),
        ("Draw polygon", Prim::Polygon { c: [3.0, 4.0], r: 1.5, sides: 5 }, "Edit Polygon"),
        ("Draw star", Prim::Star { c: [3.0, 4.0], outer: 1.5, inner: 0.5, points: 7 }, "Edit Star"),
        ("Draw arc", Prim::Arc { c: [3.0, 4.0], r: 1.5, start: 10.0, sweep: 90.0 }, "Edit Arc"),
        ("Draw spiral", Prim::Spiral { c: [3.0, 4.0], r: 1.5, turns: 2.0 }, "Edit Spiral"),
        ("Draw text", text, "Edit Text"),
    ];
    for (tool, prim, _title) in cases {
        let (mut h, id) = job_with_shape(prim.clone());
        let steps = h.state().undo.len();
        click(&mut h, tool);
        {
            let a = h.state();
            let LeftForm::Draw(f) = &a.left else { panic!("{tool}: the form is open") };
            assert_eq!(f.editing, Some(id), "{tool}");
            assert_eq!(f.prim().as_ref().map(|p| format!("{p:?}")), Some(format!("{prim:?}")), "{tool}: the form shows the shape");
            assert_eq!(a.selected, vec![id], "{tool}: it stays selected");
        }
        // A change goes to the same shape, in one step of undo.
        if let LeftForm::Draw(f) = &mut h.state_mut().left {
            f.c = [6.0, 6.0];
        }
        h.run();
        click(&mut h, "Apply");
        let a = h.state();
        assert_eq!(a.doc.vectors.len(), 1, "{tool}: no second shape");
        assert_eq!(a.doc.vectors[0].id, id);
        assert_eq!(a.undo.len(), steps + 1, "{tool}");
        let moved = a.doc.vectors[0].prim.paths();
        assert_eq!(a.doc.vectors[0].paths, moved, "{tool}");
        assert_ne!(format!("{:?}", a.doc.vectors[0].prim), format!("{prim:?}"), "{tool}: it changed");
        h.state_mut().do_undo();
        assert_eq!(format!("{:?}", h.state().doc.vectors[0].prim), format!("{prim:?}"), "{tool}");
    }
}

#[test]
fn the_form_says_when_it_is_editing() {
    let (mut h, _) = job_with_shape(Prim::Circle { c: [3.0, 4.0], r: 1.5 });
    click(&mut h, "Draw circle");
    h.get_by_label("Apply changes this shape.");
    let (mut h, _) = job_with_shape(Prim::Circle { c: [3.0, 4.0], r: 1.5 });
    h.state_mut().selected.clear();
    h.run();
    click(&mut h, "Draw circle");
    h.get_by_label("hold shift while selecting");
}

#[test]
fn another_kind_or_several_shapes_still_make_a_new_one() {
    let (mut h, _) = job_with_shape(Prim::Circle { c: [3.0, 4.0], r: 1.5 });
    click(&mut h, "Draw rectangle");
    let LeftForm::Draw(f) = &h.state().left else { panic!("the form is open") };
    assert_eq!(f.editing, None);
    assert!(h.state().selected.is_empty());
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.vectors.len(), 2);

    let (mut h, a) = job_with_shape(Prim::Circle { c: [3.0, 4.0], r: 1.5 });
    let circle = Prim::Circle { c: [7.0, 7.0], r: 1.0 };
    let b = h.state_mut().doc.add_vector(circle.paths(), circle);
    h.state_mut().selected = vec![a, b];
    h.run();
    click(&mut h, "Draw circle");
    let LeftForm::Draw(f) = &h.state().left else { panic!("the form is open") };
    assert_eq!(f.editing, None, "which one would it be?");
}

#[test]
fn shapes_that_forgot_what_they_were_are_recognised_by_their_form() {
    use crate::geom::{circle, ellipse, rect, Path};
    for (tool, paths) in [
        ("Draw circle", Path::closed(circle([3.0, 4.0], 1.5))),
        ("Draw ellipse", Path::closed(ellipse([3.0, 4.0], 2.0, 1.0))),
        ("Draw rectangle", Path::closed(rect([3.0, 4.0], 3.0, 2.0, 0.0))),
    ] {
        let mut h = harness();
        click(&mut h, "Create a new file");
        click(&mut h, "OK");
        let id = h.state_mut().doc.add_vector(vec![paths], Prim::Free);
        h.state_mut().selected = vec![id];
        h.run();
        click(&mut h, tool);
        let LeftForm::Draw(f) = &h.state().left else { panic!("{tool}: the form is open") };
        assert_eq!(f.editing, Some(id), "{tool}");
        assert!((f.c[0] - 3.0).abs() < 1e-6 && (f.c[1] - 4.0).abs() < 1e-6, "{tool}: {:?}", f.c);
        match tool {
            "Draw circle" => assert!((f.r - 1.5).abs() < 1e-6),
            "Draw ellipse" => assert!((f.rx - 2.0).abs() < 1e-6 && (f.ry - 1.0).abs() < 1e-6),
            _ => assert!((f.w - 3.0).abs() < 1e-6 && (f.h - 2.0).abs() < 1e-6),
        }
    }
}

#[test]
fn a_shape_moved_keeps_its_place_in_the_form_and_a_reshaped_one_is_not_trusted() {
    let (mut h, _) = job_with_shape(Prim::Circle { c: [3.0, 4.0], r: 1.5 });
    h.state_mut().translate_selected(1.0, 1.0);
    h.run();
    click(&mut h, "Draw circle");
    let LeftForm::Draw(f) = &h.state().left else { panic!("the form is open") };
    assert!((f.c[0] - 4.0).abs() < 1e-6 && (f.c[1] - 5.0).abs() < 1e-6, "{:?}", f.c);

    // A star dragged out of shape can't be rebuilt from its old numbers.
    let (mut h, id) = job_with_shape(Prim::Star { c: [3.0, 4.0], outer: 1.5, inner: 0.5, points: 7 });
    h.state_mut().doc.vectors[0].paths[0].pts[0][0] += 0.7;
    h.run();
    click(&mut h, "Draw star");
    let LeftForm::Draw(f) = &h.state().left else { panic!("the form is open") };
    assert_eq!(f.editing, None);
    let _ = id;
}

#[test]
fn toolpaths_follow_a_shape_edited_this_way() {
    let (mut h, id) = job_with_shape(Prim::Circle { c: [5.0, 5.0], r: 1.0 });
    h.state_mut().start_op(crate::toolpath::OpKind::Drill);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.state_mut().selected = vec![id];
    h.run();
    click(&mut h, "Draw circle");
    if let LeftForm::Draw(f) = &mut h.state_mut().left {
        f.c = [7.0, 7.0];
    }
    h.run();
    click(&mut h, "Apply");
    let a = h.state();
    assert!(a.doc.toolpaths[0].moves.iter().any(|m| !m.rapid && (m.p[0] - 7.0).abs() < 1e-6 && (m.p[1] - 7.0).abs() < 1e-6), "the drill moved with the circle");
    assert!(a.status.contains("recalculated 1"), "{}", a.status);
}

#[test]
fn a_rule_on_the_shape_is_explained_when_it_wins() {
    use crate::constraints::{DimMode, Elem};
    let (mut h, id) = job_with_shape(Prim::Circle { c: [5.0, 5.0], r: 1.0 });
    assert!(h.state_mut().make_dimension(&[Elem::Round { v: id }], DimMode::Smart), "the size is fixed by a dimension");
    h.run();
    click(&mut h, "Draw circle");
    if let LeftForm::Draw(f) = &mut h.state_mut().left {
        f.r = 2.0;
    }
    h.run();
    click(&mut h, "Apply");
    let a = h.state();
    let (_, r) = crate::bulk::as_circle(&a.doc.vectors[0].paths[0]).unwrap();
    assert!((r - 1.0).abs() < 1e-3, "the rule holds: {r}");
    assert!(a.status.contains("A rule on this shape"), "{}", a.status);
    let LeftForm::Draw(f) = &a.left else { panic!("the form is open") };
    assert!((f.r - 1.0).abs() < 1e-3, "the form shows what is really there: {}", f.r);
}

/// A job with a 4 by 2 rectangle, nothing selected.
fn sketch_rect() -> (Harness<'static, App>, u64) {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().snap = false;
    h.state_mut().snap_shapes = false;
    let prim = Prim::Rect { c: [5.0, 5.0], w: 4.0, h: 2.0, corner: 0.0 };
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    h.run();
    (h, id)
}

fn is_square(a: &App, id: u64) -> bool {
    let v = a.doc.vectors.iter().find(|v| v.id == id).unwrap();
    crate::bulk::is_rectangle(&v.paths[0])
}

#[test]
fn a_rule_already_there_or_already_true_is_refused_with_the_reason() {
    use crate::constraints::{Elem, Kind};
    let (mut h, id) = sketch_rect();
    let seg = |seg| Elem::Seg { v: id, path: 0, seg };
    // Asking for what squaring up the rectangle includes simply succeeds, once.
    assert!(h.state_mut().add_constraint(Kind::Horizontal, vec![seg(0)]));
    let count = h.state().doc.constraints.len();
    let steps = h.state().undo.len();
    assert!(!h.state_mut().add_constraint(Kind::Horizontal, vec![seg(0)]));
    assert!(h.state().sketch_note.as_deref().unwrap().contains("already there"));
    // Opposite sides of a rectangle are parallel already.
    assert!(!h.state_mut().add_constraint(Kind::Parallel, vec![seg(0), seg(2)]));
    assert!(h.state().sketch_note.as_deref().unwrap().contains("already make this true"), "{:?}", h.state().sketch_note);
    assert_eq!(h.state().doc.constraints.len(), count);
    assert_eq!(h.state().undo.len(), steps, "a refusal leaves nothing to undo");
    // And its sides meet at right angles without being told.
    assert!(!h.state_mut().add_constraint(Kind::Perpendicular, vec![seg(0), seg(1)]));
}

#[test]
fn a_second_dimension_that_follows_from_the_first_is_refused() {
    use crate::constraints::{DimMode, Elem};
    let (mut h, id) = sketch_rect();
    let seg = |seg| Elem::Seg { v: id, path: 0, seg };
    assert!(h.state_mut().make_dimension(&[seg(0)], DimMode::Smart));
    h.state_mut().dim_edit = None;
    assert!(!h.state_mut().make_dimension(&[seg(2)], DimMode::Smart), "the opposite side can't differ");
    // A different direction is real information.
    assert!(h.state_mut().make_dimension(&[seg(1)], DimMode::Smart));
}

#[test]
fn a_rule_that_would_crush_a_line_is_refused() {
    use crate::constraints::{Elem, Kind};
    let (mut h, _) = sketch_rect();
    let line = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 8.0], [5.0, 8.5]])], Prim::Free);
    let seg = Elem::Seg { v: line, path: 0, seg: 0 };
    assert!(h.state_mut().add_constraint(Kind::Horizontal, vec![seg]));
    assert!(!h.state_mut().add_constraint(Kind::Vertical, vec![seg]), "level and upright at once leaves no line");
    assert!(h.state().sketch_note.as_deref().unwrap().contains("shrink"), "{:?}", h.state().sketch_note);
    let pts = &h.state().doc.vectors.iter().find(|v| v.id == line).unwrap().paths[0].pts;
    assert!((pts[0][0] - pts[1][0]).abs() > 3.0, "the line is untouched: {pts:?}");
}

#[test]
fn a_dimension_keeps_a_rectangle_square_even_when_it_forgot_it_was_one() {
    use crate::constraints::{DimMode, Elem};
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 2.0, 0.0))], Prim::Free);
    assert!(h.state_mut().make_dimension(&[Elem::Seg { v: id, path: 0, seg: 0 }], DimMode::Smart));
    h.state_mut().dim_edit = None;
    let last = h.state().doc.constraints.len() - 1;
    assert!(h.state_mut().set_dimension(last, 6.0));
    h.run();
    assert!(is_square(h.state(), id));
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - lo[0] - 6.0).abs() < 1e-3, "{lo:?} {hi:?}");
}

#[test]
fn dragging_a_corner_in_the_rules_form_keeps_the_rectangle_square() {
    let (mut h, id) = sketch_rect();
    click(&mut h, "Drawing constraints");
    let (from, to) = (h.state().to_screen([7.0, 6.0]), h.state().to_screen([8.0, 8.0]));
    let steps = h.state().undo.len();
    drag(&mut h, from, to);
    assert!(is_square(h.state(), id), "{:?}", h.state().doc.vectors[0].paths[0].pts);
    let (_, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - 8.0).abs() < 0.05 && (hi[1] - 8.0).abs() < 0.05, "{hi:?}");
    assert_eq!(h.state().undo.len(), steps + 1, "one step of undo");
    h.state_mut().do_undo();
    let (_, back) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((back[0] - 7.0).abs() < 1e-6, "{back:?}");
}

#[test]
fn dragging_a_side_moves_it_and_a_dimension_stops_it() {
    use crate::constraints::{DimMode, Elem};
    let (mut h, id) = sketch_rect();
    click(&mut h, "Drawing constraints");
    // The right side, by its middle.
    let (from, to) = (h.state().to_screen([7.0, 5.0]), h.state().to_screen([8.0, 5.0]));
    drag(&mut h, from, to);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - 8.0).abs() < 0.05 && (lo[0] - 3.0).abs() < 0.05, "{lo:?} {hi:?}");
    assert!(is_square(h.state(), id));
    // With the width set, the same drag moves the whole shape instead of stretching it.
    assert!(h.state_mut().make_dimension(&[Elem::Seg { v: id, path: 0, seg: 0 }], DimMode::Smart));
    h.state_mut().dim_edit = None;
    h.run();
    let (from, to) = (h.state().to_screen([8.0, 5.0]), h.state().to_screen([9.0, 5.0]));
    drag(&mut h, from, to);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((hi[0] - lo[0] - 5.0).abs() < 1e-2, "the width holds: {lo:?} {hi:?}");
}

#[test]
fn a_fixed_shape_will_not_be_dragged_and_a_circle_drags_by_centre_or_edge() {
    use crate::constraints::{Elem, Kind};
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().snap = false;
    h.state_mut().snap_shapes = false;
    let prim = Prim::Circle { c: [5.0, 5.0], r: 1.0 };
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    h.run();
    click(&mut h, "Drawing constraints");
    // By the edge: the size changes, the centre stays.
    let (from, to) = (h.state().to_screen([6.0, 5.0]), h.state().to_screen([7.0, 5.0]));
    drag(&mut h, from, to);
    let (c, r) = crate::bulk::as_circle(&h.state().doc.vectors[0].paths[0]).unwrap();
    assert!((r - 2.0).abs() < 0.05 && (c[0] - 5.0).abs() < 0.01, "{c:?} {r}");
    // By the centre: it moves, at the same size.
    let (from, to) = (h.state().to_screen([5.0, 5.0]), h.state().to_screen([6.0, 6.0]));
    drag(&mut h, from, to);
    let (c, r2) = crate::bulk::as_circle(&h.state().doc.vectors[0].paths[0]).unwrap();
    assert!((c[0] - 6.0).abs() < 0.05 && (r2 - r).abs() < 0.01, "{c:?} {r2}");
    // Locked in place, the centre no longer follows.
    assert!(h.state_mut().add_constraint(Kind::Fixed, vec![Elem::Centre { v: id }]));
    h.run();
    let (from, to) = (h.state().to_screen([6.0, 6.0]), h.state().to_screen([8.0, 8.0]));
    drag(&mut h, from, to);
    let (c, _) = crate::bulk::as_circle(&h.state().doc.vectors[0].paths[0]).unwrap();
    assert!((c[0] - 6.0).abs() < 0.05, "{c:?}");
}

#[test]
fn a_click_without_moving_still_only_picks_and_leaves_no_undo_step() {
    let (mut h, _) = sketch_rect();
    click(&mut h, "Drawing constraints");
    let steps = h.state().undo.len();
    let at = h.state().to_screen([7.0, 5.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().picks.len(), 1);
    assert_eq!(h.state().undo.len(), steps);
}

#[test]
fn badges_stay_clear_of_dimensions_and_each_other_and_can_be_chosen_and_deleted() {
    use crate::constraints::{DimMode, Elem, Kind};
    let (mut h, id) = sketch_rect();
    assert!(h.state_mut().make_dimension(&[Elem::Seg { v: id, path: 0, seg: 1 }], DimMode::Smart));
    h.state_mut().dim_edit = None;
    click(&mut h, "Drawing constraints");
    let chips = h.state().rule_chips();
    assert_eq!(chips.len(), 4, "the square corners of the rectangle");
    for (i, (_, a)) in chips.iter().enumerate() {
        for (_, b) in &chips[i + 1..] {
            assert!(!a.intersects(*b), "{a:?} {b:?}");
        }
    }
    let labels: Vec<_> = h.state().doc.constraints.iter().filter(|c| c.kind.has_value()).collect();
    assert_eq!(labels.len(), 1);
    // Click a badge: its rule is chosen, not the shape; Delete takes only the rule.
    let (rule, r) = chips[0];
    let count = h.state().doc.constraints.len();
    click_at(&mut h, r.center());
    assert_eq!(h.state().selected_rule, Some(rule));
    assert!(h.state().picks.is_empty());
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(h.state().doc.constraints.len(), count - 1);
    assert_eq!(h.state().doc.vectors.len(), 1, "the shape stays");
    assert_eq!(h.state().selected_rule, None);
    let _ = Kind::Fixed;
}

#[test]
fn the_form_tells_what_to_do_and_why_a_rule_was_refused() {
    use crate::constraints::{Elem, Kind};
    let (mut h, id) = sketch_rect();
    click(&mut h, "Drawing constraints");
    h.get_by_label("Click things on the sheet, or drag them to see what can still move. The rules that fit stay lit.");
    assert!(h.state_mut().add_constraint(Kind::Horizontal, vec![Elem::Seg { v: id, path: 0, seg: 0 }]));
    assert!(!h.state_mut().add_constraint(Kind::Horizontal, vec![Elem::Seg { v: id, path: 0, seg: 0 }]));
    h.run();
    h.get_by_label("This horizontal rule is already there.");
    // Moving on clears it.
    let empty = h.state().to_screen([0.5, 9.5]);
    click_at(&mut h, empty);
    assert!(h.state().sketch_note.is_none());
}

#[test]
fn the_planetcnc_format_is_in_the_list_and_uses_arcs() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().doc.set_units(crate::app::Units::Mm);
    let circle = Prim::Circle { c: [100.0, 100.0], r: 40.0 };
    let id = h.state_mut().doc.add_vector(circle.paths(), circle);
    h.state_mut().selected = vec![id];
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::Save;
    h.state_mut().right_open = true;
    let planet = crate::toolpath::POSTS.iter().position(|p| p.0 == crate::planetcnc::NAME).unwrap();
    h.state_mut().post_index = planet;
    h.state_mut().output_arcs = true;
    h.run();
    h.get_by_label("Laid out like your own PlanetCNC file, in millimetres.");
    let text = h.state().gcode_text_named(None, "sample").unwrap();
    assert!(text.starts_with("(sample)\r\n(Created: "), "{}", &text[..40]);
    assert!(text.contains("G21 G17 G40 G49 G90 G91.1 G94"));
    assert!(text.lines().any(|l| l.starts_with("G02 ") || l.starts_with("G03 ")));
    assert!(text.ends_with("G53 G00 Z#<_tc_safeheight>\r\nM2\r\n"));
    assert_eq!(h.state().post_extension(planet), "gcode");
}

#[test]
fn a_folder_of_post_processors_is_read_with_its_subfolders() {
    let dir = std::env::temp_dir().join(format!("burin-posts-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("01-Misc/deep")).unwrap();
    for f in ["a.pp", "notes.txt", "01-Misc/b.pp", "01-Misc/deep/c.PP"] {
        std::fs::write(dir.join(f), "x").unwrap();
    }
    let names: Vec<String> = App::post_files_in(&dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(names, ["a.pp", "b.pp", "c.PP"]);
}

#[test]
fn a_drawing_file_in_an_older_character_set_still_comes_in() {
    let dir = std::env::temp_dir().join(format!("burin-latin-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plan.dxf");
    let mut bytes = b"999\nd\xE9coupe\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n2\n21\n0\n0\nENDSEC\n0\nEOF\n".to_vec();
    bytes.extend_from_slice(b"\n");
    std::fs::write(&file, bytes).unwrap();
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    h.state_mut().import_drawing(&file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(h.state().doc.vectors.len(), 1);
}

#[test]
fn a_project_from_the_original_opens_as_a_new_job() {
    use crate::crv3d::tests::{material, sample};
    let dir = std::env::temp_dir().join(format!("burin-crv3d-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sample.crv3d");
    std::fs::write(&file, crate::crv3d::tests::sample_compound(&material([0.0, -200.0, -13.0], [300.0, 0.0, 0.0], 3), &sample())).unwrap();
    let mut h = harness();
    h.state_mut().open_path(file.clone()).unwrap();
    h.run();
    std::fs::remove_dir_all(&dir).unwrap();
    let a = h.state();
    assert_eq!(a.screen, Screen::Work);
    let j = &a.doc.job;
    assert_eq!((j.width, j.height, j.thickness), (300.0, 200.0, 13.0));
    assert_eq!(j.units, crate::app::Units::Mm);
    assert_eq!(j.datum, Datum::TopLeft);
    assert!(!j.use_offset, "the file's zero is the top left corner");
    assert!(!j.z_on_bed);
    assert_eq!(j.origin(), [0.0, 200.0]);
    assert_eq!(a.doc.vectors.len(), 3);
    assert_eq!(a.doc.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["per\u{e7}age", "courbes"]);
    assert_eq!(a.doc.layers[0].color, [255, 0, 0]);
    assert_eq!(a.doc.vectors.iter().map(|v| v.layer).collect::<Vec<_>>(), [0, 0, 1]);
    // On the sheet, measured from its bottom left, and the same distance from the machine's zero.
    let (lo, hi) = crate::geom::bbox(&a.doc.vectors[0].paths).unwrap();
    assert_eq!((lo, hi), ([10.0, 140.0], [110.0, 190.0]));
    assert_eq!([lo[0] - j.origin()[0], hi[1] - j.origin()[1]], [10.0, -10.0], "as in the file");
    assert!(a.path.is_none(), "saving won't overwrite the original");
    assert!(a.status.contains("3 shapes on 2 layers, 0 toolpaths"), "{}", a.status);
    // Circles are recognised, so the circle tool can edit them.
    assert!(crate::bulk::as_circle(&a.doc.vectors[1].paths[0]).is_some());
}

#[test]
fn a_zero_away_from_the_corners_is_kept_exactly() {
    use crate::crv3d::tests::{material, sample, sample_compound};
    let dir = std::env::temp_dir().join(format!("burin-crv3d-off-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, min, max, datum, origin, bed) in [
        ("centre.crv3d", [-500.0, -500.0, -3.0], [500.0, 500.0, 0.0], 4, [500.0, 500.0], false),
        ("right.crv3d", [-1000.0, -415.0, -5.0], [0.0, 0.0, 0.0], 2, [1000.0, 415.0], false),
        ("bed.crv3d", [0.0, 0.0, 0.0], [400.0, 100.0, 12.0], 0, [0.0, 0.0], true),
        // Said to be at the bottom left, but with the zero 20 in from it.
        ("off.crv3d", [-20.0, -30.0, -3.0], [380.0, 170.0, 0.0], 0, [20.0, 30.0], false),
    ] {
        let file = dir.join(name);
        std::fs::write(&file, sample_compound(&material(min, max, datum), &sample())).unwrap();
        let mut a = App::default();
        a.open_path(file).unwrap();
        assert_eq!(a.doc.job.origin(), origin, "{name}");
        assert_eq!(a.doc.job.z_on_bed, bed, "{name}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_project_that_cant_be_read_leaves_the_current_job_alone() {
    let dir = std::env::temp_dir().join(format!("burin-crv3d-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("broken.crv3d");
    std::fs::write(&file, b"not really a project").unwrap();
    let (mut h, _) = job_with_shape(Prim::Circle { c: [3.0, 3.0], r: 1.0 });
    let e = h.state_mut().open_path(file).unwrap_err();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(e.contains("not a project file"), "{e}");
    assert_eq!(h.state().doc.vectors.len(), 1);
}

/// A project with a toolpath, opened: the sheet's top left corner is the file's zero.
fn opened_with_toolpath(dir_name: &str) -> App {
    use crate::crv3d::tests::{material, sample_with_toolpath};
    let dir = std::env::temp_dir().join(format!("burin-{dir_name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("sample.crv3d");
    std::fs::write(&file, sample_with_toolpath(&material([0.0, -200.0, -13.0], [300.0, 0.0, 0.0], 3))).unwrap();
    let mut a = App::default();
    a.open_path(file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    a
}

#[test]
fn opening_a_project_brings_its_toolpaths_with_their_tool_settings_and_moves() {
    use crate::toolpath::{OpKind, Side};
    let a = opened_with_toolpath("tp");
    assert_eq!(a.doc.toolpaths.len(), 1);
    assert!(a.status.contains("1 toolpaths"), "{}", a.status);
    assert!(a.right_open, "the list is shown");
    let t = &a.doc.toolpaths[0];
    assert_eq!(t.op.name, "Contour \u{e9}");
    assert_eq!(t.op.kind, OpKind::Profile);
    assert_eq!(t.op.side, Side::Outside);
    assert_eq!((t.op.start_depth, t.op.cut_depth, t.op.allowance, t.op.dwell), (0.0, 6.0, 0.25, 1.5));
    let tool = &t.op.tool;
    assert_eq!((tool.name.as_str(), tool.dia, tool.feed, tool.plunge, tool.rpm, tool.number), ("Fraise 3", 3.0, 800.0, 200.0, 18000.0, 3));
    assert!((tool.stepover - 0.4).abs() < 1e-9, "a share of the diameter here: {}", tool.stepover);
    assert_eq!((a.doc.machine.safe_z, a.doc.machine.plunge_gap), (15.0, 2.0));
    // Moves sit on the sheet like the shapes do: 200 up from the file's Y.
    assert_eq!(a.doc.machine_on_job().home, [0.0, 200.0, 20.0]);
    assert_eq!(a.doc.machine.home, [0.0, 0.0, 20.0], "measured from the zero, at the sheet's top left here");
    assert_eq!(t.moves[0], crate::toolpath::Move { p: [0.0, 200.0, 20.0], rapid: true });
    assert_eq!(t.moves[1], crate::toolpath::Move { p: [10.0, 190.0, 15.0], rapid: true });
    // It comes down fast to just above the material, then slowly.
    assert_eq!(t.moves[2], crate::toolpath::Move { p: [10.0, 190.0, 2.0], rapid: true });
    assert_eq!(t.moves[3], crate::toolpath::Move { p: [10.0, 190.0, -6.0], rapid: false });
    // It knows which shapes it was made from.
    assert_eq!(t.op.source.len(), 1);
    assert!(!t.op.paths.is_empty());
}

#[test]
fn the_machine_file_uses_the_projects_own_arcs_and_positions() {
    let mut a = opened_with_toolpath("tp-file");
    a.post_index = crate::toolpath::POSTS.iter().position(|p| p.0 == crate::planetcnc::NAME).unwrap();
    a.output_arcs = true;
    let text = a.gcode_text_named(None, "sample").unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("G0")).collect();
    // Positions are back in the file's own coordinates, counted from its zero.
    assert!(lines.contains(&"G00 X10.000 Y-10.000 Z15.000"), "{lines:?}");
    assert!(lines.contains(&"G00 Z2.000"), "{lines:?}");
    assert!(lines.contains(&"G01 Z-6.000 F200.0"), "{lines:?}");
    assert!(lines.contains(&"G01 X110.000 F800.0"), "{lines:?}");
    // The two small sides of the curve are one arc, as the file records it.
    assert_eq!(lines.iter().filter(|l| l.starts_with("G03") || l.starts_with("G02")).count(), 1, "{lines:?}");
    assert!(text.contains("S18000.00 M3") && text.contains("T3 M6 (Fraise 3)"));
}

#[test]
fn its_moves_stay_exact_through_saving_until_the_toolpath_is_calculated_here() {
    let a = opened_with_toolpath("tp-save");
    let dir = std::env::temp_dir().join(format!("burin-tp-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("copie.burin");
    let mut a = a;
    a.save_to(file.clone()).unwrap();
    let mut b = App::default();
    b.open_path(file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(b.doc.toolpaths[0].moves, a.doc.toolpaths[0].moves);
    assert!(b.doc.toolpaths[0].kept_steps.is_some());
    // Changing the shape it was made from works it out here instead.
    let id = b.doc.toolpaths[0].op.source[0];
    if let Some(v) = b.doc.vectors.iter_mut().find(|v| v.id == id) {
        for q in v.paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
            q[0] += 5.0;
        }
    }
    assert_eq!(b.relink_toolpaths(), 1);
    assert!(b.doc.toolpaths[0].kept_moves.is_none() && b.doc.toolpaths[0].kept_steps.is_none());
}

#[test]
fn a_change_of_units_carries_the_kept_moves_along() {
    let mut a = opened_with_toolpath("tp-units");
    a.doc.set_units(crate::app::Units::Inches);
    let t = &a.doc.toolpaths[0];
    assert!((t.moves[1].p[0] - 10.0 / 25.4).abs() < 1e-9);
    assert_eq!(t.kept_moves.as_ref().unwrap()[1], t.moves[1]);
    assert!(matches!(t.kept_steps.as_ref().unwrap()[1], crate::arcs::Step::Straight(m) if (m.p[0] - 10.0 / 25.4).abs() < 1e-9));
}

#[test]
fn the_carve_preview_runs_on_imported_moves() {
    let mut a = opened_with_toolpath("tp-sim");
    a.preview(vec![0]);
    a.finish_preview();
    let g = &a.sim.as_ref().unwrap().grid;
    // The straight cut from (10, 190) to (110, 190), 6 deep with a 3 wide tool.
    assert!((g.sample(60.0, 190.0) + 6.0).abs() < 0.1, "{}", g.sample(60.0, 190.0));
    assert_eq!(g.sample(60.0, 150.0), 0.0);
}

fn profile_on(h: &mut Harness<'static, App>, depth: f64) {
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(h, "Profile toolpath");
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.cut_depth = depth;
    }
    h.run();
}

#[test]
fn a_cut_through_the_material_asks_first() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick + 0.1);
    click(&mut h, "Calculate");
    assert!(h.state().through_warning.is_some());
    assert!(h.state().doc.toolpaths.is_empty(), "nothing calculated yet");
    h.get_by_label_contains("Material thickness");
    click(&mut h, "Cancel");
    assert!(h.state().through_warning.is_none());
    assert!(h.state().doc.toolpaths.is_empty(), "cancelled");
    assert!(matches!(h.state().right, RightMode::Op { .. }), "back to the form to change the depth");
    click(&mut h, "Calculate");
    click(&mut h, "Calculate Anyway");
    assert_eq!(h.state().doc.toolpaths.len(), 1);
}

#[test]
fn a_cut_to_exactly_the_thickness_does_not_ask() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick);
    click(&mut h, "Calculate");
    assert!(h.state().through_warning.is_none());
    assert_eq!(h.state().doc.toolpaths.len(), 1);
}

#[test]
fn the_preview_warns_of_toolpaths_now_deeper_than_the_material() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick);
    click(&mut h, "Calculate");
    assert!(h.state().message.is_none());
    h.state_mut().preview(vec![0]);
    assert!(h.state().message.is_none(), "still fits");
    h.state_mut().doc.job.thickness = thick / 2.0;
    h.state_mut().preview(vec![0]);
    let name = h.state().doc.toolpaths[0].op.name.clone();
    let told = h.state().message.clone().expect("a warning");
    assert!(told.contains("cut through") && told.contains(&name), "{told}");
}

#[test]
fn passes_set_in_their_window_are_the_passes_cut() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick);
    click(&mut h, "Edit Passes ...");
    assert!(h.state().passes_dialog.is_some());
    // Cancel changes nothing.
    h.state_mut().passes_dialog.as_mut().unwrap().count = 5;
    h.run();
    click(&mut h, "Use Number");
    h.get_all_by_label("Cancel").last().unwrap().click();
    h.run();
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert!(op.passes.is_empty());
    // OK keeps them, with one taken out.
    click(&mut h, "Edit Passes ...");
    h.state_mut().passes_dialog.as_mut().unwrap().count = 5;
    h.run();
    click(&mut h, "Use Number");
    assert_eq!(h.state().passes_dialog.as_ref().unwrap().list.len(), 5);
    h.get_all_by_label("Delete").last().unwrap().click();
    h.run();
    assert_eq!(h.state().passes_dialog.as_ref().unwrap().list.len(), 4);
    h.get_all_by_label("OK").last().unwrap().click();
    h.run();
    assert!(h.state().passes_dialog.is_none());
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert_eq!(op.passes.len(), 4);
    assert!((op.passes[0] - thick * 2.0 / 5.0).abs() < 1e-9, "the first was taken out: {:?}", op.passes);
    click(&mut h, "Calculate");
    let moves = h.state().doc.toolpaths[0].moves.clone();
    let levels: std::collections::BTreeSet<i64> = moves.iter().filter(|m| !m.rapid).map(|m| (m.p[2] * 1e6).round() as i64).collect();
    assert_eq!(levels.len(), 4, "{levels:?}");
}

#[test]
fn the_last_pass_cannot_be_removed_and_clearing_leaves_one() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick);
    click(&mut h, "Edit Passes ...");
    click(&mut h, "Clear All Passes");
    assert_eq!(h.state().passes_dialog.as_ref().unwrap().list, [thick]);
    h.get_all_by_label("Delete").last().unwrap().click();
    h.run();
    assert_eq!(h.state().passes_dialog.as_ref().unwrap().list, [thick], "the full depth stays");
}

#[test]
fn the_selector_chooses_by_kind_and_keeps_following_the_drawing() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let hole = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([2.0, 2.0], 0.2))], Prim::Free);
    let plate = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 2.0, 2.0, 0.0))], Prim::Free);
    h.run();
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    click(&mut h, "Selector ...");
    click(&mut h, "Closed Vectors ...");
    assert_eq!(h.state().selected, [hole, plate]);
    click(&mut h, "Only Circles");
    assert_eq!(h.state().selected, [hole], "follows the filters at once");
    click(&mut h, "Associate with toolpath");
    h.get_all_by_label("Close").last().unwrap().click();
    h.run();
    assert!(h.state().selector_dialog.is_none());
    // A circle drawn afterwards is picked up by the next calculation.
    let later = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([7.0, 2.0], 0.2))], Prim::Free);
    h.run();
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths[0].op.source, [hole, later]);
}

#[test]
fn every_tab_of_the_profile_form_changes_the_cut() {
    use crate::toolpath::{RampKind, StartAt};
    let mut h = job_with_square([5.0, 5.0]);
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    h.state_mut().show_advanced = true;
    h.run();
    click(&mut h, "Add ramps to toolpath");
    click(&mut h, "Zig Zag");
    click(&mut h, "Leads");
    click(&mut h, "Add leads");
    click(&mut h, "Circular");
    click(&mut h, "Do lead out");
    click(&mut h, "Order");
    click(&mut h, "Shortest Path");
    click(&mut h, "Start At");
    click(&mut h, "Move Start Points to be closest to selected point on vector bounding box");
    click(&mut h, "Start point 5");
    click(&mut h, "Corners");
    click(&mut h, "Sharp external corners");
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert_eq!(op.ramp_kind, RampKind::ZigZag);
    assert!(op.ramp && op.lead && op.lead_circular && op.lead_out && op.sharp_corners);
    assert!(op.order_choices.unwrap().shortest);
    assert_eq!(op.start_at, StartAt::Near(4));
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    assert!(!h.state().doc.toolpaths[0].moves.is_empty());
}

#[test]
fn the_form_windows_hold_still_once_open() {
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    profile_on(&mut h, thick + 0.1);
    let place = |h: &Harness<'static, App>, label: &str| h.get_by_label(label).rect();
    click(&mut h, "Selector ...");
    let first = place(&h, "Open Vectors");
    h.run();
    h.run();
    assert_eq!(first, place(&h, "Open Vectors"));
    h.get_all_by_label("Close").last().unwrap().click();
    h.run();
    click(&mut h, "Edit Passes ...");
    let first = place(&h, "Clear All Passes");
    h.run();
    h.run();
    assert_eq!(first, place(&h, "Clear All Passes"));
}

fn square_with_profile() -> Harness<'static, App> {
    let mut h = job_with_square([3.0, 3.0]);
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    click(&mut h, "Calculate");
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    h
}

fn cut_right_edge(h: &Harness<'static, App>) -> f64 {
    h.state().doc.toolpaths[0].moves.iter().filter(|m| !m.rapid).map(|m| m.p[0]).fold(f64::MIN, f64::max)
}

#[test]
fn a_toolpath_follows_a_shape_dragged_somewhere_else() {
    let mut h = square_with_profile();
    let before = cut_right_edge(&h);
    let (from, to) = {
        let a = h.state();
        (a.to_screen([4.0, 3.0]), a.to_screen([6.0, 3.0]))
    };
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    h.hover_at(to);
    h.run();
    assert_eq!(cut_right_edge(&h), before, "not while the shape is still being dragged");
    h.drop_at(to);
    h.run();
    assert!((cut_right_edge(&h) - before - 2.0).abs() < 0.03, "{before} then {}", cut_right_edge(&h));
    assert!(h.state().status.contains("Recalculated 1"), "{}", h.state().status);
    // Undo brings the path back with the shape.
    h.state_mut().do_undo();
    h.run();
    assert!((cut_right_edge(&h) - before).abs() < 1e-9);
}

#[test]
fn a_toolpath_follows_any_other_change_of_its_shape() {
    let mut h = square_with_profile();
    let before = cut_right_edge(&h);
    // Changed from anywhere, not through a form the toolpaths are told about.
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.run();
    assert!((cut_right_edge(&h) - before - 1.0).abs() < 0.03);
}

#[test]
fn recalculating_says_what_it_did_and_clears_the_old_carving() {
    let mut h = square_with_profile();
    h.state_mut().preview(vec![0]);
    h.state_mut().anim.playing = false;
    assert!(h.state().sim.is_some());
    h.run();
    click(&mut h, "Recalculate all toolpaths");
    assert!(h.state().status.contains("Recalculated 1"), "{}", h.state().status);
    assert!(h.state().sim.is_none(), "the carving came from the old moves");
}

#[test]
fn a_drawing_change_clears_a_carving_made_from_the_old_moves() {
    let mut h = square_with_profile();
    h.state_mut().preview(vec![0]);
    h.state_mut().anim.playing = false;
    h.run();
    assert!(h.state().sim.is_some());
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[1] += 1.0;
    }
    h.run();
    assert!(h.state().sim.is_none());
}

#[test]
fn an_unrelated_shape_leaves_the_toolpath_and_its_carving_alone() {
    let mut h = square_with_profile();
    h.state_mut().preview(vec![0]);
    h.state_mut().anim.playing = false;
    h.run();
    let moves = h.state().doc.toolpaths[0].moves.clone();
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([8.0, 8.0], 0.5))], Prim::Free);
    h.run();
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves));
    assert!(h.state().sim.is_some());
}

#[test]
fn a_toolpath_from_a_project_file_follows_its_shape_and_keeps_its_settings() {
    let mut a = opened_with_toolpath("tp-follow");
    a.doc.toolpaths[0].op.climb = false;
    let saved = a.doc.toolpaths[0].moves.clone();
    // Nothing changed: its moves stay exactly as they came, even when asked to recalculate.
    a.follow_shapes();
    a.recalculate_all();
    assert!(std::sync::Arc::ptr_eq(&saved, &a.doc.toolpaths[0].moves) || saved == a.doc.toolpaths[0].moves);
    assert!(a.status.contains("kept as saved"), "{}", a.status);
    let id = a.doc.toolpaths[0].op.source[0];
    for q in a.doc.vectors.iter_mut().filter(|v| v.id == id).flat_map(|v| v.paths.iter_mut()).flat_map(|p| p.pts.iter_mut()) {
        q[0] += 5.0;
    }
    a.follow_shapes();
    let t = &a.doc.toolpaths[0];
    assert!(t.kept_moves.is_none(), "worked out again");
    assert_ne!(saved, t.moves);
    assert!(!t.op.climb, "with the file's own settings");
}

#[test]
fn a_toolpath_that_cannot_follow_says_so_when_recalculated() {
    let mut a = opened_with_toolpath("tp-cannot");
    a.doc.toolpaths[0].op.source.clear();
    let saved = a.doc.toolpaths[0].moves.clone();
    for q in a.doc.vectors.iter_mut().flat_map(|v| v.paths.iter_mut()).flat_map(|p| p.pts.iter_mut()) {
        q[0] += 5.0;
    }
    a.follow_shapes();
    a.recalculate_all();
    assert_eq!(saved, a.doc.toolpaths[0].moves, "kept, not silently emptied");
    assert!(a.status.contains("can't follow"), "{}", a.status);
}

fn profile_of_square() -> (Harness<'static, App>, u64) {
    let mut h = square_with_profile();
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected_tp = Some(0);
    h.run();
    (h, id)
}

#[test]
fn opening_a_toolpath_selects_its_shapes_again() {
    let (mut h, id) = profile_of_square();
    h.state_mut().selected.clear();
    h.run();
    h.state_mut().edit_toolpath();
    h.run();
    assert_eq!(h.state().selected, [id]);
    // Calculating straight away keeps it on the same shape.
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths[0].op.source, [id]);
}

#[test]
fn opening_a_toolpath_forgets_other_shapes_that_were_selected() {
    let (mut h, id) = profile_of_square();
    let other = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([8.0, 8.0], 0.5))], Prim::Free);
    h.state_mut().selected = vec![other];
    h.state_mut().edit_toolpath();
    assert_eq!(h.state().selected, [id], "not the circle that happened to be selected");
    // And another shape can still be chosen while in it.
    h.state_mut().selected = vec![other];
    h.run();
    click(&mut h, "Calculate");
    assert_eq!(h.state().doc.toolpaths[0].op.source, [other]);
}

fn square_left(h: &Harness<'static, App>) -> f64 {
    h.state().doc.vectors[0].paths[0].pts.iter().map(|p| p[0]).fold(f64::MAX, f64::min)
}

#[test]
fn the_arrow_keys_move_the_selection_further_with_modifiers() {
    use egui::{Key, Modifiers};
    let mut h = job_with_square([5.0, 5.0]);
    let x = square_left(&h);
    let top = |h: &Harness<'static, App>| h.state().doc.vectors[0].paths[0].pts.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
    let y = top(&h);
    h.key_press(Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x - 0.05).abs() < 1e-9, "one step");
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x - 0.55).abs() < 1e-9, "ten steps");
    h.key_press_modifiers(Modifiers::COMMAND, Key::ArrowLeft);
    h.run();
    assert!((square_left(&h) - x + 4.45).abs() < 1e-9, "a hundred steps");
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x + 4.445).abs() < 1e-9, "a tenth of a step");
    h.key_press(Key::ArrowUp);
    h.run();
    assert!((top(&h) - y - 0.05).abs() < 1e-9);
    h.key_press(Key::ArrowDown);
    h.key_press(Key::ArrowDown);
    h.run();
    assert!((top(&h) - y + 0.05).abs() < 1e-9);
}

#[test]
fn an_arrow_step_is_a_round_number_in_millimetres() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.set_units(Units::Mm);
    h.run();
    let x = square_left(&h);
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x - 1.0).abs() < 1e-9);
}

#[test]
fn arrows_need_a_selection_and_leave_typing_alone() {
    let mut h = job_with_square([5.0, 5.0]);
    let x = square_left(&h);
    h.state_mut().selected.clear();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(square_left(&h), x);
    assert!(h.state().undo.is_empty() || h.state().undo.len() == 1 && !h.state().nudging);
    // With the cursor in a number box the arrow belongs to the box.
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().start_xform(Xform::Move);
    h.run();
    h.get_all_by_value("0.0").next().unwrap().click();
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(square_left(&h), x);
}

#[test]
fn holding_an_arrow_is_one_undo_and_the_toolpath_waits_for_the_end() {
    use egui::{Event, Key, Modifiers};
    let (mut h, _) = profile_of_square();
    let (x, cut, steps) = (square_left(&h), cut_right_edge(&h), h.state().undo.len());
    let down = |repeat: bool| Event::Key { key: Key::ArrowRight, physical_key: None, pressed: true, repeat, modifiers: Modifiers::NONE };
    h.event(down(false));
    h.run();
    for _ in 0..4 {
        h.event(down(true));
        h.run();
    }
    assert!((square_left(&h) - x - 0.25).abs() < 1e-9, "five steps");
    assert_eq!(cut_right_edge(&h), cut, "not while the key is held");
    h.event(Event::Key { key: Key::ArrowRight, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run();
    assert!((cut_right_edge(&h) - cut - 0.25).abs() < 0.03, "follows on release: {}", cut_right_edge(&h) - cut);
    assert_eq!(h.state().undo.len(), steps + 1);
    h.state_mut().do_undo();
    h.run();
    assert_eq!(square_left(&h), x);
    assert!((cut_right_edge(&h) - cut).abs() < 1e-9);
}

#[test]
fn the_zero_can_go_anywhere_and_comes_back() {
    let mut j = Job::default();
    for datum in [Datum::BottomLeft, Datum::TopRight, Datum::Centre, Datum::TopLeft, Datum::BottomRight] {
        j.datum = datum;
        j.set_origin([3.0, 7.0]);
        assert_eq!(j.origin(), [3.0, 7.0], "{datum:?}");
    }
}

#[test]
fn the_move_form_moves_the_zero_and_the_gcode_counts_from_it() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().start_xform(Xform::Move);
    h.run();
    click(&mut h, "Zero to Anchor");
    // Anchored at the middle of the square by default or its corner, it is where the anchor says.
    let (lo, hi) = h.state().selected_bbox().unwrap();
    let anchor = match &h.state().left { LeftForm::Xform(f) => f.anchor, _ => panic!() };
    assert_eq!(h.state().doc.job.origin(), crate::edit::anchor_point(lo, hi, anchor));
    assert!(!h.state().undo.is_empty());
    click(&mut h, "Back to Corner");
    assert_eq!(h.state().doc.job.origin(), [0.0, 0.0]);
    h.state_mut().doc.job.set_origin([2.0, 1.0]);
    h.state_mut().right = RightMode::List;
    h.state_mut().right_open = true;
    h.state_mut().close_left_form();
    h.run();
    // Seen in the toolpath panel, and one click from the settings.
    h.get_by_label_contains("X: 2.0  Y: 1.0");
    h.get_all_by_label("X/Y zero").last().unwrap().click();
    h.run();
    assert!(matches!(h.state().right, RightMode::Material(_)));
}

#[test]
fn the_zero_is_also_in_the_edit_menu() {
    let mut h = job_with_square([5.0, 5.0]);
    click(&mut h, "Edit");
    click(&mut h, "Move X/Y Zero...");
    assert!(matches!(&h.state().left, LeftForm::Xform(f) if f.kind == Xform::Move));
}

#[test]
fn the_list_shows_what_kind_each_toolpath_is() {
    let mut h = job_with_square([5.0, 5.0]);
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    click(&mut h, "Calculate");
    h.state_mut().right = RightMode::Op { op: Box::new(crate::toolpath::blank_op(crate::toolpath::OpKind::Pocket, "P".into(), crate::toolpath::default_tools()[0].clone(), 0.1, Vec::new())), editing: None, tab: 0, error: None };
    h.run();
    click(&mut h, "Calculate");
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    h.get_by_label("2D Profile Toolpath");
    h.get_by_label("Pocket Toolpath");
}

#[test]
fn rates_can_be_shown_per_minute_or_per_second_and_stay_per_minute_inside() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.set_units(Units::Mm);
    h.run();
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    h.state_mut().edit_passes = true;
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.tool.feed = 3000.0;
    }
    h.run();
    assert!(h.get_all_by_label("mm/min").next().is_some());
    click(&mut h, "mm/s");
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert!(op.tool.per_second);
    assert_eq!(op.tool.feed, 3000.0, "showing it another way doesn't change it");
    assert!(h.get_all_by_value("50.0").next().is_some(), "3000 mm/min is shown as 50 mm/s");
    // The G-code's feed is still per minute.
    click(&mut h, "Calculate");
    h.state_mut().post_index = 1;
    let a = h.state();
    assert!(a.doc.toolpaths[0].op.tool.per_second);
    let text = a.gcode_text_named(Some(0), "x").unwrap_or_default();
    assert!(text.contains("F3000"), "{}", &text[..text.len().min(500)]);
}

#[test]
fn a_new_job_starts_in_millimetres_with_mm_tools() {
    let mut h = harness();
    h.state_mut().metric_by_default = true;
    h.run();
    click(&mut h, "Create a new file");
    click(&mut h, "OK");
    let j = &h.state().doc.job;
    assert_eq!((j.units, j.width, j.height, j.thickness), (Units::Mm, 600.0, 400.0, 18.0));
    assert!((h.state().doc.machine.safe_z - 5.08).abs() < 1e-9);
    h.state_mut().selected.clear();
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert!(op.tool.metric, "{}", op.tool.label());
    assert_eq!(op.cut_depth, 18.0);
    // Still a choice: inches are one click away and convert.
}

#[test]
fn the_sample_opens_in_millimetres_too() {
    let mut a = App::default();
    a.metric_by_default = true;
    a.load_sample();
    assert_eq!(a.doc.job.units, Units::Mm);
    assert!((a.doc.job.width - 254.0).abs() < 1e-6);
    assert!(a.doc.toolpaths.iter().all(|t| t.op.tool.metric));
}

/// Lets the work away from the window finish, as frames go by.
fn wait_for_recalc(h: &mut Harness<'static, App>) {
    for _ in 0..4000 {
        h.step();
        if h.state().recalc.is_none() {
            h.step();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the work never finished");
}

#[test]
fn toolpaths_are_worked_out_away_from_the_window_with_the_same_result() {
    let (mut h, id) = profile_of_square();
    let cut = cut_right_edge(&h);
    h.state_mut().background_recalc = true;
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.step();
    assert!(h.state().recalc.is_some(), "the shape's change starts the work");
    wait_for_recalc(&mut h);
    assert!((cut_right_edge(&h) - cut - 1.0).abs() < 0.03);
    assert!(h.state().status.contains("Recalculated 1"), "{}", h.state().status);
    assert_eq!(h.state().doc.toolpaths[0].op.source, [id]);
    // The button too.
    h.get_by_label("Recalculate all toolpaths").click();
    h.step();
    h.step();
    assert!(h.state().recalc.is_some());
    wait_for_recalc(&mut h);
    assert!(h.state().status.contains("Recalculated 1 toolpath"), "{}", h.state().status);
    // And the form's Calculate, ending in the preview as it should.
    h.state_mut().selected_tp = Some(0);
    h.state_mut().edit_toolpath();
    h.step();
    h.step();
    h.get_by_label("Calculate").click();
    h.step();
    h.step();
    assert!(h.state().recalc.is_some());
    wait_for_recalc(&mut h);
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    assert!(matches!(h.state().right, RightMode::Preview));
    assert!(h.state().view3d);
}

/// Work that is under way and goes no further until the sender says so.
fn hold_recalc(h: &mut Harness<'static, App>, finish: crate::recalc::Finish) -> std::sync::mpsc::Sender<crate::recalc::Message> {
    let (send, messages) = std::sync::mpsc::channel();
    h.state_mut().recalc = Some(crate::recalc::Running {
        total: 3,
        results: Vec::new(),
        current: "Contour 1".into(),
        messages,
        stop: Default::default(),
        started: std::time::Instant::now() - std::time::Duration::from_secs(2),
        finish: Some(finish),
    });
    send
}

#[test]
fn a_bar_shows_how_far_it_has_gone_and_nothing_else_can_be_touched() {
    use crate::recalc::{Finish, Message};
    let (mut h, _) = profile_of_square();
    let op = h.state().doc.toolpaths[0].op.clone();
    let x = square_left(&h);
    let send = hold_recalc(&mut h, Finish::Follow);
    h.step();
    h.step();
    h.get_by_label_contains("Toolpath 1 of 3: Contour 1");
    send.send(Message::Done(Box::new((0, op.clone(), Ok(Vec::new()))))).unwrap();
    send.send(Message::Starting("Poche 2".into())).unwrap();
    h.step();
    h.step();
    h.get_by_label_contains("Toolpath 2 of 3: Poche 2");
    // The drawing can't be moved meanwhile, with the mouse or the keys.
    let (from, to) = {
        let a = h.state();
        (a.to_screen([4.0, 3.0]), a.to_screen([7.0, 3.0]))
    };
    h.hover_at(from);
    h.step();
    h.drag_at(from);
    h.step();
    h.hover_at(to);
    h.step();
    h.drop_at(to);
    h.step();
    h.key_press(egui::Key::ArrowRight);
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.step();
    assert_eq!(square_left(&h), x);
    // The end of the work takes the bar away and keeps what was done.
    drop(send);
    h.step();
    h.step();
    assert!(h.state().recalc.is_none());
    assert!(h.state().doc.toolpaths[0].moves.is_empty(), "what came back is kept");
    assert!(h.state().status.contains("Stopped"), "only one of three came back: {}", h.state().status);
}

#[test]
fn stopping_keeps_the_old_moves_of_what_isnt_finished() {
    use crate::recalc::Finish;
    let (mut h, _) = profile_of_square();
    let before = h.state().doc.toolpaths[0].moves.clone();
    let send = hold_recalc(&mut h, Finish::All { unchanged: 0, no_shapes: 0, warn: true });
    h.step();
    h.step();
    h.get_by_label("Stop").click();
    h.step();
    h.step();
    assert!(h.state().recalc.is_none());
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().doc.toolpaths[0].moves));
    assert!(h.state().status.contains("Stopped"), "{}", h.state().status);
    assert!(send.send(crate::recalc::Message::Starting(String::new())).is_err(), "the work is told to stop");
}

#[test]
fn stopping_a_calculation_leaves_the_toolpath_and_its_form() {
    use crate::recalc::Finish;
    let (mut h, _) = profile_of_square();
    let before = h.state().doc.toolpaths[0].moves.clone();
    h.state_mut().selected_tp = Some(0);
    h.state_mut().edit_toolpath();
    let RightMode::Op { op, .. } = h.state().right.clone() else { panic!() };
    let _send = hold_recalc(&mut h, Finish::Calculate { form: op, editing: Some(0), tab: 0 });
    h.step();
    h.step();
    h.get_by_label("Stop").click();
    h.step();
    h.step();
    assert!(matches!(h.state().right, RightMode::Op { .. }), "still in the form");
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().doc.toolpaths[0].moves));
}

#[test]
fn a_failed_calculation_in_the_background_says_why_in_the_form() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().selected.clear();
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    h.state_mut().background_recalc = true;
    h.get_by_label("Calculate").click();
    h.step();
    h.step();
    wait_for_recalc(&mut h);
    assert!(matches!(&h.state().right, RightMode::Op { error: Some(_), .. }));
}

fn deep_profile() -> Harness<'static, App> {
    let mut h = job_with_square([3.0, 3.0]);
    let thick = h.state().doc.job.thickness;
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.cut_depth = thick + 0.03;
    }
    h.run();
    click(&mut h, "Calculate");
    click(&mut h, "Calculate Anyway");
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    assert!(h.state().message.is_none(), "already asked when it was calculated");
    h
}

#[test]
fn recalculating_warns_of_every_toolpath_past_the_material() {
    let mut h = deep_profile();
    click(&mut h, "Recalculate all toolpaths");
    let told = h.state().message.clone().expect("a warning");
    assert!(told.contains("cut through") && told.contains("Profile 1") && told.contains("0.53") && told.contains("0.5 "), "{told}");
    assert!(told.contains("sacrificial"), "it may well be meant: {told}");
    // It only warns: the toolpath is still there, and is cleared with OK.
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    h.get_all_by_label("OK").last().unwrap().click();
    h.run();
    assert!(h.state().message.is_none());
}

#[test]
fn a_toolpath_following_its_shape_warns_too_but_not_one_within_the_material() {
    let mut h = deep_profile();
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.run();
    assert!(h.state().message.as_deref().unwrap_or("").contains("Profile 1"), "{:?}", h.state().message);
    // Within the material nothing is said.
    let mut h = square_with_profile();
    click(&mut h, "Recalculate all toolpaths");
    assert!(h.state().message.is_none());
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.run();
    assert!(h.state().message.is_none());
}

#[test]
fn only_the_toolpaths_that_went_too_deep_and_were_redone_are_named() {
    let mut h = deep_profile();
    h.state_mut().doc.toolpaths[0].op.name = "Traverse".into();
    // A second toolpath within the material, on another shape.
    let other = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([8.0, 8.0], 0.5))], Prim::Free);
    h.state_mut().selected = vec![other];
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    h.state_mut().calculate_op();
    h.state_mut().doc.toolpaths[1].op.name = "Dedans".into();
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    click(&mut h, "Recalculate all toolpaths");
    let told = h.state().message.clone().unwrap();
    assert!(told.contains("Traverse") && !told.contains("Dedans"), "{told}");
    h.state_mut().message = None;
    // Moving the other shape redoes only that one: it stays within the material, so nothing is said.
    for q in h.state_mut().doc.vectors[1].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] -= 1.0;
    }
    h.run();
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

#[test]
fn a_recalculation_in_the_background_warns_when_it_ends() {
    let mut h = deep_profile();
    h.state_mut().background_recalc = true;
    h.get_by_label("Recalculate all toolpaths").click();
    h.step();
    h.step();
    assert!(h.state().message.is_none(), "not before it's done");
    wait_for_recalc(&mut h);
    assert!(h.state().message.as_deref().unwrap_or("").contains("Profile 1"));
}

#[test]
fn a_long_list_of_toolpaths_past_the_material_fits_the_warning() {
    let mut h = deep_profile();
    let one = h.state().doc.toolpaths[0].clone();
    for _ in 0..30 {
        h.state_mut().doc.toolpaths.push(one.clone());
    }
    h.state_mut().warn_if_too_deep(&(0..31).collect::<Vec<_>>());
    let told = h.state().message.clone().unwrap();
    assert!(told.contains("and 19 more"), "{told}");
    assert!(told.lines().count() < 25);
}

#[test]
fn a_project_opening_is_not_a_warning_yet() {
    let mut h = deep_profile();
    let dir = std::env::temp_dir().join(format!("burin-open-deep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("job.burin");
    h.state_mut().doc.toolpaths[0].moves = Default::default();
    std::fs::write(&file, serde_json::to_string(&h.state().doc).unwrap()).unwrap();
    let mut a = App::default();
    a.open_path(file).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(a.message.is_none());
    assert!(!a.doc.toolpaths[0].moves.is_empty(), "its moves are worked out again all the same");
    // The preview still tells.
    a.preview(vec![0]);
    assert!(a.message.as_deref().unwrap_or("").contains("cut through"));
}

#[test]
fn the_carve_preview_is_as_fine_as_the_smallest_tool_needs() {
    let mut h = job_with_square([3.0, 3.0]);
    assert_eq!(h.state().sim_points(), 1_000_000, "nothing to cut yet");
    // A big sheet in millimetres with a 3 mm tool: 0.375 mm a cell.
    h.state_mut().doc.set_units(Units::Mm);
    h.state_mut().doc.job.width = 600.0;
    h.state_mut().doc.job.height = 400.0;
    let mut op = crate::toolpath::blank_op(crate::toolpath::OpKind::Profile, "p".into(), crate::toolpath::default_tools()[0].in_units(true), 3.0, Vec::new());
    op.tool.dia = 3.0;
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: op.clone(), visible: true, color: [0, 0, 0], moves: Default::default(), kept_moves: None, kept_steps: None });
    let points = h.state().sim_points();
    let cell = (600.0f64 * 400.0 / points as f64).sqrt();
    assert!((cell - 0.375).abs() < 1e-3, "{cell}");
    // A huge one stops at what a computer can hold.
    h.state_mut().doc.job.width = 3000.0;
    h.state_mut().doc.job.height = 2000.0;
    assert_eq!(h.state().sim_points(), 12_000_000);
    // A small job with a big tool needs no less than the usual.
    h.state_mut().doc.job.width = 100.0;
    h.state_mut().doc.job.height = 100.0;
    h.state_mut().doc.toolpaths[0].op.tool.dia = 25.0;
    assert_eq!(h.state().sim_points(), 1_000_000);
    // The simulation really uses it.
    h.state_mut().doc.job.width = 600.0;
    h.state_mut().doc.job.height = 400.0;
    h.state_mut().doc.toolpaths[0].op.tool.dia = 3.0;
    h.state_mut().preview(vec![0]);
    assert!(h.state().sim.as_ref().unwrap().grid.data.len() >= points);
}

#[test]
fn the_grid_and_the_snap_count_from_the_moved_zero() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.job.set_origin([3.137, 2.271]);
    h.state_mut().snap = true;
    h.state_mut().snap_shapes = false;
    h.state_mut().show_guides = false;
    h.run();
    let step = h.state().grid_steps().0;
    let got = h.state().snapped([6.0, 7.0]);
    for (value, zero) in [(got[0], 3.137), (got[1], 2.271)] {
        let lines = (value - zero) / step;
        assert!((lines - lines.round()).abs() < 1e-9, "{value} is {lines} steps from the zero");
    }
    // Back in the corner, as it always was.
    h.state_mut().doc.job.set_origin([0.0, 0.0]);
    let got = h.state().snapped([6.0, 7.0]);
    assert!((got[0] / step - (got[0] / step).round()).abs() < 1e-9);
}

fn carved_square() -> Harness<'static, App> {
    let mut h = square_with_profile();
    h.state_mut().doc.toolpaths[0].visible = false;
    h.state_mut().right = RightMode::Preview;
    h.state_mut().view3d = true;
    h.state_mut().preview(vec![0]);
    h.state_mut().finish_preview();
    h.state_mut().surface_in_background = true;
    h
}

fn wait_for_picture(h: &mut Harness<'static, App>) {
    h.step();
    for _ in 0..4000 {
        h.step();
        if h.state().surface_job.is_none() {
            h.step();
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the picture never came");
}

#[test]
fn the_sharp_3d_picture_is_made_away_from_the_window() {
    let mut h = carved_square();
    h.step();
    // Ordered, and the window goes on while it is made.
    assert!(h.state().surface_job.is_some() || h.state().mesh.is_some());
    wait_for_picture(&mut h);
    assert!(h.state().mesh_is_sharp());
    // Nothing new is ordered while nothing changes.
    h.step();
    assert!(h.state().surface_job.is_none());
}

#[test]
fn moving_the_view_shows_a_quick_picture_and_the_sharp_one_comes_after() {
    let mut h = carved_square();
    wait_for_picture(&mut h);
    let centre = h.state().to_screen([0.0, 0.0]);
    let _ = centre;
    let at = egui::pos2(900.0, 500.0);
    h.hover_at(at);
    h.step();
    h.drag_at(at);
    h.step();
    for k in 1..6 {
        h.hover_at(egui::pos2(900.0 + k as f32 * 8.0, 500.0));
        h.step();
        assert!(h.state().surface_job.is_none(), "nothing is waited for while it moves");
        assert!(!h.state().mesh_is_sharp(), "the quick picture");
    }
    h.drop_at(egui::pos2(950.0, 500.0));
    wait_for_picture(&mut h);
    assert!(h.state().mesh_is_sharp());
}

#[test]
fn going_to_the_end_of_a_big_carve_never_holds_the_window_long() {
    let mut h = square_with_profile();
    // Many tiny moves, as in a real job.
    let cut: Vec<crate::toolpath::Move> = (0..400_000).map(|i| {
        let a = i as f64 * 0.001;
        crate::toolpath::Move { p: [5.0 + 3.0 * a.cos(), 5.0 + 3.0 * a.sin(), -0.1], rapid: false }
    }).collect();
    h.state_mut().doc.toolpaths[0].moves = std::sync::Arc::new(cut);
    h.state_mut().doc.toolpaths[0].visible = false;
    h.state_mut().right = RightMode::Preview;
    h.state_mut().surface_in_background = true;
    h.state_mut().preview(vec![0]);
    h.state_mut().rush_preview();
    h.step();
    let mut frames = 1;
    while h.state().anim.playing {
        assert!(frames < 20_000, "it never ends");
        h.step();
        frames += 1;
    }
    assert!(!h.state().anim.rush);
    // All of it was cut in the end, whatever the number of frames it took.
    let mut whole = square_with_profile();
    whole.state_mut().doc.toolpaths[0].moves = h.state().doc.toolpaths[0].moves.clone();
    whole.state_mut().preview(vec![0]);
    whole.state_mut().finish_preview();
    assert!(h.state().sim.as_ref().unwrap().grid.data == whole.state().sim.as_ref().unwrap().grid.data, "the same carve");
}

#[test]
fn starting_something_new_drops_the_picture_being_made() {
    let mut h = carved_square();
    h.step();
    h.state_mut().new_file();
    assert!(h.state().surface_job.is_none());
}

#[test]
fn a_toolpath_deeper_than_its_settings_say_warns_from_its_moves() {
    // Such as a V-carve, whose depth comes from the width of the shape.
    let mut h = square_with_profile();
    let thick = h.state().doc.job.thickness;
    h.state_mut().doc.toolpaths[0].op.kind = crate::toolpath::OpKind::VCarve;
    h.state_mut().doc.toolpaths[0].op.flat_depth = 0.0;
    h.state_mut().doc.toolpaths[0].moves = std::sync::Arc::new(vec![crate::toolpath::Move { p: [1.0, 1.0, 1.0], rapid: true }, crate::toolpath::Move { p: [1.0, 1.0, -thick - 0.2], rapid: false }]);
    h.state_mut().warn_if_too_deep(&[0]);
    assert!(h.state().message.as_deref().unwrap_or("").contains("cut through"), "{:?}", h.state().message);
}

#[test]
fn a_new_toolpath_that_only_turns_out_too_deep_warns_once_calculated() {
    // A V-carve goes as deep as the shape is wide: nothing to ask about beforehand.
    let mut h = job_with_square([5.0, 5.0]);
    let thick = h.state().doc.job.thickness;
    h.state_mut().start_op(crate::toolpath::OpKind::VCarve);
    let RightMode::Op { op, .. } = &h.state().right else { panic!() };
    assert!(crate::toolpath::deepest_cut(op) <= thick, "its settings see no danger");
    h.state_mut().confirm_depth_then_calculate();
    assert!(h.state().through_warning.is_none());
    let deep = crate::toolpath::deepest_move(&h.state().doc.toolpaths[0].moves);
    assert!(deep > thick, "2 wide with this bit goes deeper than {thick}: {deep}");
    assert!(h.state().message.as_deref().unwrap_or("").contains("cut through"), "{:?}", h.state().message);
}

#[test]
fn a_saved_ramp_keeps_its_own_kind_and_limit_when_followed() {
    let mut a = opened_with_toolpath("tp-ramp");
    let saved = &mut a.doc.toolpaths[0].op;
    saved.ramp = true;
    saved.ramp_kind = crate::toolpath::RampKind::ZigZag;
    saved.ramp_distance = 5.0;
    let id = a.doc.toolpaths[0].op.source[0];
    for q in a.doc.vectors.iter_mut().filter(|v| v.id == id).flat_map(|v| v.paths.iter_mut()).flat_map(|p| p.pts.iter_mut()) {
        q[0] += 5.0;
    }
    a.follow_shapes();
    assert_eq!(a.doc.toolpaths[0].op.ramp_kind, crate::toolpath::RampKind::ZigZag);
}

/// A job of 13 thick with a square, in millimetres; and a contour to the given depth.
fn job_on_board(board: f64, depth: f64) -> Harness<'static, App> {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.set_units(Units::Mm);
    h.state_mut().doc.job.width = 600.0;
    h.state_mut().doc.job.height = 400.0;
    h.state_mut().doc.job.thickness = 13.0;
    h.state_mut().doc.job.spoil_board = board;
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.cut_depth = depth;
    }
    h.run();
    h
}

#[test]
fn cutting_into_the_sacrificial_board_brings_no_warning() {
    let mut h = job_on_board(3.0, 13.8);
    click(&mut h, "Calculate");
    assert!(h.state().through_warning.is_none());
    assert_eq!(h.state().doc.toolpaths.len(), 1, "calculated without asking");
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    click(&mut h, "Recalculate all toolpaths");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    h.state_mut().preview(vec![0]);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

#[test]
fn right_down_to_the_bottom_of_the_board_is_still_fine() {
    let mut h = job_on_board(3.0, 16.0);
    click(&mut h, "Calculate");
    assert!(h.state().through_warning.is_none());
    assert_eq!(h.state().doc.toolpaths.len(), 1);
}

#[test]
fn going_past_the_board_warns_at_every_step() {
    let mut h = job_on_board(3.0, 16.5);
    click(&mut h, "Calculate");
    assert_eq!(h.state().through_warning, Some(16.5), "asks first");
    h.get_by_label_contains("Sacrificial board: 3.0 mm, 16.0 mm in all");
    click(&mut h, "Calculate Anyway");
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    click(&mut h, "Recalculate all toolpaths");
    let told = h.state().message.clone().expect("a warning");
    assert!(told.contains("through the sacrificial board") && told.contains("13.0 mm + board: 3.0 mm = 16.0 mm") && told.contains("0.5 mm past the sacrificial board"), "{told}");
}

#[test]
fn with_no_board_the_warning_starts_at_the_material_and_says_where_to_enter_one() {
    let mut h = job_on_board(0.0, 13.8);
    click(&mut h, "Calculate");
    assert_eq!(h.state().through_warning, Some(13.8));
    click(&mut h, "Calculate Anyway");
    h.state_mut().view3d = false;
    h.state_mut().right = RightMode::List;
    h.run();
    click(&mut h, "Recalculate all toolpaths");
    let told = h.state().message.clone().expect("a warning");
    assert!(told.contains("through the material") && told.contains("job's settings"), "{told}");
}

#[test]
fn the_board_is_set_in_the_job_and_material_forms_and_converts_with_units() {
    let mut h = harness();
    click(&mut h, "Create a new file");
    h.get_by_label_contains("Cutting into it brings no warning");
    click(&mut h, "OK");
    h.state_mut().doc.job.spoil_board = 0.125;
    h.state_mut().doc.set_units(Units::Mm);
    assert!((h.state().doc.job.spoil_board - 3.175).abs() < 1e-9);
    h.state_mut().right_open = true;
    h.state_mut().right = RightMode::Material(Box::new(h.state().doc.clone()));
    h.run();
    h.get_by_label_contains("Cutting into it brings no warning");
    // A project saved before there was such a thing opens with none.
    let text = serde_json::to_string(&h.state().doc).unwrap().replace("\"spoil_board\":3.175,", "");
    assert!(!text.contains("spoil_board"));
    let old: crate::app::Doc = serde_json::from_str(&text).unwrap();
    assert_eq!(old.job.spoil_board, 0.0);
}

#[test]
fn changing_where_a_toolpath_starts_moves_its_first_cut() {
    use crate::toolpath::Move;
    let first = |moves: &[Move]| moves.windows(2).find(|w| w[0].rapid && !w[1].rapid).map(|w| w[1].p).unwrap();
    let mut h = job_with_square([5.0, 5.0]);
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    click(&mut h, "Profile toolpath");
    h.state_mut().show_advanced = true;
    h.run();
    click(&mut h, "Start At");
    click(&mut h, "Move Start Points to be closest to selected point on vector bounding box");
    click(&mut h, "Start point 1");
    click(&mut h, "Calculate");
    h.run();
    let top_left = first(&h.state().doc.toolpaths[0].moves);
    // Opened again and changed, the same toolpath starts somewhere else.
    h.state_mut().selected_tp = Some(0);
    h.state_mut().edit_toolpath();
    h.run();
    click(&mut h, "Start At");
    click(&mut h, "Start point 5");
    click(&mut h, "Calculate");
    h.run();
    assert_eq!(h.state().doc.toolpaths.len(), 1);
    let bottom_right = first(&h.state().doc.toolpaths[0].moves);
    assert!(bottom_right[0] > top_left[0] + 0.5 && bottom_right[1] < top_left[1] - 0.5, "{top_left:?} then {bottom_right:?}");
}

#[test]
fn a_file_of_several_toolpaths_goes_home_once_and_home_follows_the_zero() {
    let mut h = job_with_square([5.0, 5.0]);
    h.get_all_by_label("Toolpaths").last().unwrap().click();
    h.run();
    let square = h.state().doc.vectors[0].id;
    for _ in 0..2 {
        h.state_mut().selected = vec![square];
        h.state_mut().start_op(crate::toolpath::OpKind::Profile);
        h.run();
        click(&mut h, "Calculate");
        h.run();
    }
    assert_eq!(h.state().doc.toolpaths.len(), 2);
    let moved = |code: &str| -> Vec<(f64, f64)> {
        let word = |line: &str, letter: char| line.split_whitespace().find_map(|w| w.strip_prefix(letter).and_then(|v| v.parse::<f64>().ok()));
        let (mut x, mut y) = (None, None);
        let mut out = Vec::new();
        for line in code.lines().filter(|l| l.starts_with("G0") || l.starts_with("G1") || l.starts_with('X') || l.starts_with('Y')) {
            x = word(line, 'X').or(x);
            y = word(line, 'Y').or(y);
            if let (Some(a), Some(b)) = (x, y) {
                out.push((a, b));
            }
        }
        out
    };
    let times_at_home = |points: &[(f64, f64)], home: (f64, f64)| {
        let mut stops = 0;
        let mut was_there = false;
        for p in points {
            let here = (p.0 - home.0).abs() < 1e-3 && (p.1 - home.1).abs() < 1e-3;
            stops += usize::from(here && !was_there);
            was_there = here;
        }
        stops
    };
    let code = h.state().gcode_text(None).unwrap();
    let points = moved(&code);
    assert_eq!(points[0], (0.0, 0.0), "starts at home: {code}");
    assert_eq!(times_at_home(&points, (0.0, 0.0)), 2, "there at the start and end only");
    // With the zero moved, home is still X0 Y0 for the machine, and so is a new place on the job.
    h.state_mut().doc.job.set_origin([3.0, 2.0]);
    let code = h.state().gcode_text(None).unwrap();
    let points = moved(&code);
    assert_eq!(points[0], (0.0, 0.0), "{code}");
    assert_eq!(*points.last().unwrap(), (0.0, 0.0), "{code}");
    assert_eq!(times_at_home(&points, (0.0, 0.0)), 2, "{code}");
    let on_job = h.state().doc.machine_on_job().home;
    assert_eq!((on_job[0], on_job[1]), (3.0, 2.0));
}

#[test]
fn a_panel_section_folds_away_and_comes_back() {
    let mut h = job_with_square([5.0, 5.0]);
    assert!(h.query_by_label("Draw circle").is_some());
    click(&mut h, "Create Vectors");
    assert!(h.query_by_label("Draw circle").is_none(), "its tools are hidden");
    assert!(h.query_by_label("Smart dimension").is_some(), "the others stay");
    click(&mut h, "Create Vectors");
    assert!(h.query_by_label("Draw circle").is_some());
}

