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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([5.0, 5.0], 2.0))], Prim::Free);
    h.state_mut().select_all();
    h.run();
    click(&mut h, "3D Model");
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
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    click(&mut h, "Library");
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
    click(&mut h, "Start a new project");
    click(&mut h, "Cancel");
    assert_eq!(h.state().screen, Screen::Start);
}

#[test]
fn view_tools_stay_clear_of_the_right_panel_in_a_narrow_window() {
    let mut h = Harness::builder().with_size(vec2(943.0, 700.0)).with_accessibility_check(false).build_ui_state(|ui, app: &mut App| app.frame(ui), App::default());
    crate::ui::setup_style(&h.ctx);
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    let last = h.get_by_label("Tile Windows Horizontal").rect();
    assert!(last.right() <= 943.0 - 282.0, "the last tool ends at {}", last.right());
    click(&mut h, "Show grid");
    assert!(h.state().grid);
}

#[test]
fn every_toolpath_form_opens_and_explains_what_it_needs() {
    let mut h = harness();
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
    h.run();
    click(&mut h, "Create fillets");
    let corner = h.state().to_screen([3.0, 3.0]);
    click_at(&mut h, corner);
    assert!(h.state().doc.vectors[0].paths[0].pts.len() > 10);
}

#[test]
fn tools_can_be_kept_in_a_group_of_their_own() {
    let mut h = harness();
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.run();
    click(&mut h, "Profile toolpath");
    click(&mut h, "Select ...");
    click(&mut h, "End Mill (0.5 inch)");
    click(&mut h, "New Group");
    // The name starts as "New Group"; Apply puts the chosen tool in it.
    h.get_all_by_value("New Group").next().unwrap().focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_all_by_value("New Group").next().unwrap().type_text("Roughing");
    h.run();
    click(&mut h, "Apply");
    // An empty group at first, as the original's New Group makes; tools are dragged onto it.
    assert!(h.query_by_label("Roughing").is_some(), "shown as a folder of its own");
    let tools = h.state().tool_edit.clone().unwrap();
    assert!(tools.iter().all(|t| t.group.is_empty()), "nothing in it yet");
    let from = h.get_all_by_label("End Mill (0.5 inch)").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    let to = h.get_all_by_label("Roughing").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    drag(&mut h, from, to);
    let tools = h.state().tool_edit.clone().unwrap();
    assert_eq!(tools.iter().find(|t| t.label() == "End Mill (0.5 inch)").unwrap().group, "Roughing");
    // Dragged onto the group's folder, another tool joins it.
    let from = h.get_all_by_label("End Mill (0.25 inch)").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    let to = h.get_all_by_label("Roughing").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    drag(&mut h, from, to);
    let tools = h.state().tool_edit.clone().unwrap();
    assert_eq!(tools.iter().find(|t| t.label() == "End Mill (0.25 inch)").unwrap().group, "Roughing", "dragged in");
    // And dragged back onto the units' folder, it leaves.
    let from = h.get_all_by_label("End Mill (0.25 inch)").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    let to = h.get_by_label("Imperial Tools").rect().center();
    drag(&mut h, from, to);
    let tools = h.state().tool_edit.clone().unwrap();
    assert_eq!(tools.iter().find(|t| t.label() == "End Mill (0.25 inch)").unwrap().group, "", "dragged out");
    let tree = h.get_all_by_label("End Mill (0.5 inch)").map(|n| n.rect().center()).min_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
    click_at(&mut h, tree);
    h.run();
    click(&mut h, "Use This Tool");
    assert!(h.state().doc.tools.iter().any(|t| t.group == "Roughing"), "kept with the tools");
    assert_eq!(h.state().doc.tool_groups, ["Roughing"], "and the group with the project");
    let text = serde_json::to_string(&h.state().doc.tools).unwrap();
    assert_eq!(text.matches("\"group\"").count(), 1, "only grouped tools say so");
}

#[test]
fn the_tool_list_chooses_a_tool_for_the_form() {
    let mut h = harness();
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    assert!(h.query_by_label("SUBSTITUTE is not applied.").is_none());
    click(&mut h, "Remove This One");
    assert!(h.state().loaded_posts.is_empty());
    assert_eq!(h.state().post_index, 0);
}

#[test]
fn trimming_a_ruled_shape_with_the_mouse_leaves_it_in_place() {
    use crate::constraints::{DimMode, Elem, Kind};
    let mut h = harness();
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
        click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    h.state_mut().selected = vec![id];
    h.run();
    (h, id)
}

#[test]
fn clicking_a_tool_with_its_kind_of_shape_selected_edits_it() {
    let text = Prim::Text { at: [2.0, 2.0], height: 0.8, text: "Ab".into(), style: Default::default() };
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
        click(&mut h, "Start a new project");
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
fn toolpaths_wait_for_recalculate_when_a_shape_is_edited_this_way() {
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
    let drilled = |h: &Harness<'static, App>| h.state().doc.toolpaths[0].moves.iter().any(|m| !m.rapid && (m.p[0] - 7.0).abs() < 1e-6 && (m.p[1] - 7.0).abs() < 1e-6);
    assert!(!drilled(&h), "nothing is worked out on its own");
    assert_eq!(h.state().behind, [0], "it's noted as out of date");
    h.state_mut().recalculate_all();
    h.run();
    assert!(drilled(&h), "the drill moved with the circle");
    assert!(h.state().behind.is_empty());
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Start a new project");
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
    assert_eq!(t.moves[0], crate::toolpath::Move { p: [0.0, 200.0, 20.0], rapid: true, join: false });
    assert_eq!(t.moves[1], crate::toolpath::Move { p: [10.0, 190.0, 15.0], rapid: true, join: false });
    // It comes down fast to just above the material, then slowly.
    assert_eq!(t.moves[2], crate::toolpath::Move { p: [10.0, 190.0, 2.0], rapid: true, join: false });
    assert_eq!(t.moves[3], crate::toolpath::Move { p: [10.0, 190.0, -6.0], rapid: false, join: false });
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
    b.recalculate_all();
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
    click(&mut h, "Start a new project");
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
fn a_toolpath_stays_as_it_is_when_its_shape_is_dragged_until_recalculate() {
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
    h.drop_at(to);
    h.run();
    assert_eq!(cut_right_edge(&h), before, "moving a shape doesn't recalculate");
    assert_eq!(h.state().behind, [0]);
    assert!(h.state().status.contains("no longer match"), "{}", h.state().status);
    click(&mut h, "Recalculate all toolpaths");
    assert!((cut_right_edge(&h) - before - 2.0).abs() < 0.03, "{before} then {}", cut_right_edge(&h));
    assert!(h.state().status.contains("Recalculated 1"), "{}", h.state().status);
    assert!(h.state().behind.is_empty());
    // Undo brings the path back with the shape.
    h.state_mut().do_undo();
    h.run();
    assert!((cut_right_edge(&h) - before).abs() < 1e-9);
}

#[test]
fn a_toolpath_stays_as_it_is_after_any_other_change_of_its_shape() {
    let mut h = square_with_profile();
    let before = cut_right_edge(&h);
    // Changed from anywhere, not through a form the toolpaths are told about.
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.run();
    assert_eq!(cut_right_edge(&h), before);
    assert_eq!(h.state().behind, [0]);
    click(&mut h, "Recalculate all toolpaths");
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
fn a_carving_stays_until_its_toolpaths_are_recalculated() {
    let mut h = square_with_profile();
    h.state_mut().preview(vec![0]);
    h.state_mut().anim.playing = false;
    h.run();
    assert!(h.state().sim.is_some());
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[1] += 1.0;
    }
    h.run();
    assert!(h.state().sim.is_some(), "its toolpath hasn't changed");
    h.state_mut().recalculate_all();
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
    a.recalculate_all();
    a.recalculate_all();
    assert!(std::sync::Arc::ptr_eq(&saved, &a.doc.toolpaths[0].moves) || saved == a.doc.toolpaths[0].moves);
    assert!(a.status.contains("kept as saved"), "{}", a.status);
    let id = a.doc.toolpaths[0].op.source[0];
    for q in a.doc.vectors.iter_mut().filter(|v| v.id == id).flat_map(|v| v.paths.iter_mut()).flat_map(|p| p.pts.iter_mut()) {
        q[0] += 5.0;
    }
    a.recalculate_all();
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
    a.recalculate_all();
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
fn ctrl_and_shift_with_an_arrow_move_a_fixed_distance() {
    use egui::{Key, Modifiers};
    let both = Modifiers::COMMAND | Modifiers::SHIFT;
    let mut h = job_with_square([5.0, 5.0]);
    let x = square_left(&h);
    h.key_press_modifiers(both, Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x - 0.125).abs() < 1e-9, "in inches");
    h.state_mut().doc.set_units(Units::Mm);
    h.run();
    let x = square_left(&h);
    h.key_press_modifiers(both, Key::ArrowRight);
    h.run();
    assert!((square_left(&h) - x - 0.1).abs() < 1e-9, "in millimetres");
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
fn holding_an_arrow_is_one_undo_and_the_toolpath_stays() {
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
    assert!(h.state().behind.is_empty(), "nor is it even looked at yet");
    h.event(Event::Key { key: Key::ArrowRight, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run();
    assert_eq!(cut_right_edge(&h), cut, "nor on release");
    assert_eq!(h.state().behind, [0]);
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
    h.get_by_label_contains("X0 Y0: 2.0, 1.0");
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
    click(&mut h, "Start a new project");
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
    assert!(h.state().recalc.is_none(), "the shape's change starts nothing");
    h.get_by_label("Recalculate all toolpaths").click();
    h.step();
    h.step();
    assert!(h.state().recalc.is_some(), "the button does");
    wait_for_recalc(&mut h);
    assert!((cut_right_edge(&h) - cut - 1.0).abs() < 0.03);
    assert_eq!(h.state().doc.toolpaths[0].op.source, [id]);
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
    let send = hold_recalc(&mut h, Finish::All { unchanged: 0, no_shapes: 0, warn: false });
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
fn recalculating_after_a_shape_changed_warns_too_but_not_within_the_material() {
    let mut h = deep_profile();
    for q in h.state_mut().doc.vectors[0].paths.iter_mut().flat_map(|p| p.pts.iter_mut()) {
        q[0] += 1.0;
    }
    h.run();
    h.state_mut().recalculate_all();
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
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: op.clone(), visible: true, color: [0, 0, 0], moves: Default::default(), kept_moves: None, kept_steps: None, face: 0, sheet: 0 });
    let points = h.state().sim_points();
    let cell = (600.0f64 * 400.0 / points as f64).sqrt();
    assert!((cell - 0.375).abs() < 1e-3, "{cell}");
    // A huge one stops at what a computer can hold.
    h.state_mut().doc.job.width = 3000.0;
    h.state_mut().doc.job.height = 2000.0;
    assert_eq!(h.state().sim_points(), crate::app::Quality3d::Balanced.most_points());
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

/// In a browser, which has no threads, the sharp picture is made a slice at a time between frames,
/// so the page keeps answering: it takes several frames, none of them long, and moving the view
/// meanwhile drops it at once.
#[test]
fn without_threads_the_sharp_picture_comes_a_slice_at_a_time() {
    let mut h = carved_square();
    h.state_mut().surface_sliced = true;
    h.step();
    assert!(h.state().surface_job.is_some(), "ordered, not made at once");
    let mut frames = 0;
    let mut longest = 0f32;
    while h.state().surface_job.is_some() {
        let started = std::time::Instant::now();
        h.step();
        longest = longest.max(started.elapsed().as_secs_f32() * 1000.0);
        frames += 1;
        assert!(frames < 20_000, "the picture never came");
    }
    h.step();
    assert!(h.state().mesh_is_sharp());
    assert!(frames > 1, "made over {frames} frames");
    // A frame also draws the window; the slice itself is far shorter than a whole picture.
    eprintln!("sharp picture over {frames} frames, the longest {longest:.0} ms");

    // Moving the view drops the picture on its way and shows a quick one instead.
    h.state_mut().cam.yaw += 0.2;
    h.step();
    assert!(h.state().surface_job.is_some());
    let at = egui::pos2(900.0, 500.0);
    h.hover_at(at);
    h.step();
    h.drag_at(at);
    h.step();
    h.hover_at(egui::pos2(920.0, 500.0));
    h.step();
    assert!(h.state().surface_job.is_none(), "nothing is waited for while it moves");
    h.drop_at(egui::pos2(930.0, 500.0));
    wait_for_picture(&mut h);
    assert!(h.state().mesh_is_sharp());
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
        crate::toolpath::Move { p: [5.0 + 3.0 * a.cos(), 5.0 + 3.0 * a.sin(), -0.1], rapid: false, join: false }
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
    h.state_mut().doc.toolpaths[0].moves = std::sync::Arc::new(vec![crate::toolpath::Move { p: [1.0, 1.0, 1.0], rapid: true, join: false }, crate::toolpath::Move { p: [1.0, 1.0, -thick - 0.2], rapid: false, join: false }]);
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
    a.recalculate_all();
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
    click(&mut h, "Start a new project");
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
    click(&mut h, "Shapes");
    assert!(h.query_by_label("Draw circle").is_none(), "its tools are hidden");
    assert!(h.query_by_label("Smart dimension").is_some(), "the others stay");
    click(&mut h, "Shapes");
    assert!(h.query_by_label("Draw circle").is_some());
}


#[test]
fn leaving_unsaved_work_asks_first() {
    use crate::app::Leave;
    let mut h = harness();
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    // A new job with nothing in it has nothing to lose.
    h.state_mut().leave_for(Leave::New);
    assert_eq!(h.state().leave, None);

    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([3.0, 3.0], 1.0))], Prim::Circle { c: [3.0, 3.0], r: 1.0 });
    h.state_mut().checkpoint();
    assert!(h.state().has_unsaved_work());
    h.state_mut().leave_for(Leave::New);
    h.run();
    assert_eq!(h.state().leave, Some(Leave::New));
    // A form behind it may have its own: the window on top is the last one.
    h.get_all_by_label("Cancel").last().unwrap().click();
    h.run();
    assert_eq!(h.state().leave, None);
    assert!(h.state().doc.vectors.iter().any(|v| v.id == id), "cancel keeps it");

    h.state_mut().leave_for(Leave::New);
    h.run();
    click(&mut h, "Don't Save");
    h.run();
    assert!(h.state().doc.vectors.is_empty(), "the person chose to drop it");
    assert!(!h.state().has_unsaved_work());
}

#[test]
fn a_project_saved_over_itself_is_whole_and_leaves_nothing_beside_it() {
    let dir = std::env::temp_dir().join(format!("burin-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("one.burin");
    let mut a = App::default();
    a.load_sample();
    a.save_to(file.clone()).unwrap();
    a.save_to(file.clone()).unwrap();
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
    let mut b = App::default();
    b.open_path(file).unwrap();
    assert_eq!(b.doc.vectors.len(), a.doc.vectors.len());
    // Somewhere it can't be written is said, and the work still counts as unsaved.
    a.dirty = true;
    assert!(a.save_to(dir.join("missing").join("two.burin")).is_err());
    assert!(a.dirty);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_machine_file_is_refused_when_it_would_cut_the_wrong_thing() {
    use crate::postp::PostDef;
    use crate::toolpath::{blank_op, default_tools, generate, JobSize, OpKind};
    let mut a = App::default();
    let tools = default_tools();
    let id = a.doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([3.0, 3.0], 1.0))], Prim::Circle { c: [3.0, 3.0], r: 1.0 });
    let paths = a.doc.vectors[0].paths.clone();
    let (machine, job) = (a.doc.machine_on_job(), JobSize { width: 10.0, height: 10.0, thickness: 1.0 });
    let add = |a: &mut App, tool: crate::toolpath::Tool, name: &str| {
        let mut op = blank_op(OpKind::Profile, name.to_string(), tool, 0.1, paths.clone());
        op.source = vec![id];
        let moves = generate(&op, machine, &job, None).unwrap();
        a.keep_for_test(op, moves);
    };
    add(&mut a, tools[0].clone(), "First");
    assert_eq!(a.output_problem(None), None);

    // The shape moved and the toolpath didn't manage to follow.
    let keep = a.doc.vectors[0].paths.clone();
    a.doc.vectors[0].paths = vec![crate::geom::Path::closed(crate::geom::circle([6.0, 6.0], 1.0))];
    assert!(a.output_problem(None).is_some_and(|m| m.contains("First")));
    // Only the toolpath being saved counts.
    a.doc.toolpaths[0].visible = false;
    assert_eq!(a.output_problem(None), None);
    a.doc.toolpaths[0].visible = true;
    a.doc.vectors.clear();
    assert!(a.output_problem(None).is_some_and(|m| m.contains("First")), "its shape has gone");
    a.doc.add_vector(keep, Prim::Free);
    let new_id = a.doc.vectors[0].id;
    a.doc.toolpaths[0].op.source = vec![new_id];
    assert_eq!(a.output_problem(None), None);

    // Two different tools, through a post that has no way to ask for the second.
    let other = tools.iter().find(|t| t.dia != tools[0].dia).unwrap().clone();
    add(&mut a, other, "Second");
    a.doc.toolpaths[1].op.source = vec![new_id];
    let base = "POST_NAME = \"Plain (*.nc)\"\nFILE_EXTENSION = \"nc\"\nUNITS = \"INCHES\"\nVAR X_POSITION = [X|A|X|1.3]\nVAR Y_POSITION = [Y|A|Y|1.3]\nVAR Z_POSITION = [Z|A|Z|1.3]\nbegin RAPID_MOVE\n\"G0[X][Y][Z]\"\nbegin FEED_MOVE\n\"G1[X][Y][Z]\"\n";
    a.loaded_posts.push(PostDef::parse(base, &|_| None).unwrap());
    a.post_index = crate::toolpath::POSTS.len();
    assert!(a.output_problem(None).is_some_and(|m| m.contains("can't change tool")));
    assert_eq!(a.output_problem(Some(0)), None, "one at a time is fine");
    a.loaded_posts[0] = PostDef::parse(&format!("{base}begin TOOLCHANGE\n\"M6\"\n"), &|_| None).unwrap();
    assert_eq!(a.output_problem(None), None);
    // The built-in formats all stop for the tool, so they're never refused for it.
    a.post_index = 0;
    assert_eq!(a.output_problem(None), None);
}

#[test]
fn the_look_stays_dark_on_a_computer_set_to_light() {
    let ctx = egui::Context::default();
    ctx.set_theme(egui::ThemePreference::Light);
    crate::ui::setup_style(&ctx);
    assert_eq!(ctx.options(|o| o.theme_preference), egui::ThemePreference::Dark);
    assert_eq!(ctx.global_style().visuals.panel_fill, crate::theme::PANEL);
    assert!(ctx.global_style().visuals.dark_mode);
}

#[test]
fn the_3d_quality_can_be_chosen_from_the_preview_panel() {
    use crate::app::Quality3d;
    let mut h = carved_square();
    wait_for_picture(&mut h);
    assert_eq!(h.state().quality3d, Quality3d::Balanced);
    assert!(h.state().mesh_is_sharp());
    assert!(h.state().picture_seconds.is_some(), "the time it took is told");
    h.state_mut().set_quality3d(Quality3d::Light);
    h.run();
    wait_for_picture(&mut h);
    assert!(h.state().mesh_is_sharp(), "the picture is made again for the new quality");
}

/// How long each tab is in the carve along the middle of the cut, and how many there are.
fn tabs_in_the_carve(app: &App) -> (f64, usize) {
    let sim = app.sim.as_ref().unwrap();
    let height = |x: f64, y: f64| {
        let (i, j) = ((x / sim.grid.cell).round() as usize, (y / sim.grid.cell).round() as usize);
        sim.grid.data[j * sim.grid.nx + i] as f64
    };
    let moves = crate::toolpath::working_moves(&app.doc.toolpaths[0].moves);
    let bottom = moves.iter().position(|m| m.p[2] < -11.9).unwrap();
    let (mut standing, mut count, mut inside) = (0.0, 0, false);
    for w in moves[bottom..].windows(2).take_while(|w| !w[1].rapid) {
        let length = (w[1].p[0] - w[0].p[0]).hypot(w[1].p[1] - w[0].p[1]);
        let steps = (length / 0.02).ceil() as usize;
        for s in 0..steps {
            let t = s as f64 / steps as f64;
            let there = height(w[0].p[0] + (w[1].p[0] - w[0].p[0]) * t, w[0].p[1] + (w[1].p[1] - w[0].p[1]) * t) > -11.5;
            if there {
                standing += length / steps as f64;
            }
            count += usize::from(there && !inside);
            inside = there;
        }
    }
    (standing / count.max(1) as f64, count)
}

#[test]
fn a_tab_is_as_long_in_the_carve_as_was_asked() {
    let mut app = App::default();
    crate::tab_demo(&mut app, 6.0);
    let (long, count) = tabs_in_the_carve(&app);
    println!("with the fix: {count} tabs, each {long:.2} mm");
    assert_eq!(count, 4);
    assert!((long - 6.0).abs() < 0.6, "{long}");
    // The tool stopping only the tab's length apart, as it did before.
    let mut before = App::default();
    crate::tab_demo(&mut before, 0.001);
    let (long, count) = tabs_in_the_carve(&before);
    println!("before the fix: {count} tabs, each {long:.2} mm");
    assert!(long < 0.6, "{long}");
}

#[test]
fn g_groups_and_u_ungroups_from_the_keyboard() {
    use egui::{Key, Modifiers};
    let mut h = job_with_square([5.0, 5.0]);
    let c = [2.0, 2.0];
    let other = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.5))], Prim::Circle { c, r: 0.5 });
    let first = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![first, other];
    h.run();
    h.key_press(Key::G);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 1);
    assert!(h.state().doc.vectors[0].is_group());
    // Clicking any shape in it takes the whole group, which moves as one.
    h.key_press(Key::ArrowRight);
    h.run();
    h.key_press(Key::U);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 2);
    let circle = h.state().doc.vectors.iter().find(|v| v.id == other).unwrap().prim.clone();
    let Prim::Circle { c: moved, .. } = circle else { panic!("{circle:?}") };
    assert!((moved[0] - 2.05).abs() < 1e-9, "{moved:?}");
    // With Shift it takes every level.
    h.key_press(Key::G);
    h.run();
    let inner = h.state().selected[0];
    let extra = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([8.0, 8.0], 0.5))], Prim::Free);
    h.state_mut().selected = vec![inner, extra];
    h.run();
    h.key_press(Key::G);
    h.run();
    h.key_press_modifiers(Modifiers::SHIFT, Key::U);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 3);
    assert!(h.state().doc.vectors.iter().all(|v| !v.is_group()));
}

#[test]
fn edit_objects_has_buttons_to_group_and_ungroup() {
    let mut h = job_with_square([5.0, 5.0]);
    use egui_kittest::kittest::NodeT;
    let enabled = |h: &Harness<'static, App>, label: &str| !h.get_by_label(label).accesskit_node().is_disabled();
    h.state_mut().selected.clear();
    h.run();
    assert!(!enabled(&h, "Group") && !enabled(&h, "Ungroup") && !enabled(&h, "Ungroup All"), "nothing to do yet");
    let c = [2.0, 2.0];
    let (first, other) = (h.state().doc.vectors[0].id, h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.5))], Prim::Circle { c, r: 0.5 }));
    h.state_mut().selected = vec![first, other];
    h.run();
    assert!(enabled(&h, "Group"));
    click(&mut h, "Group");
    assert!(h.state().doc.vectors[0].is_group());
    assert!(!enabled(&h, "Group"), "one group selected: nothing to group it with");
    assert!(enabled(&h, "Ungroup") && enabled(&h, "Ungroup All"));
    click(&mut h, "Ungroup");
    assert_eq!(h.state().doc.vectors.len(), 2);
    click(&mut h, "Group");
    let inner = h.state().selected[0];
    let extra = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([8.0, 8.0], 0.5))], Prim::Free);
    h.state_mut().selected = vec![inner, extra];
    h.run();
    click(&mut h, "Group");
    click(&mut h, "Ungroup All");
    assert_eq!(h.state().doc.vectors.len(), 3, "every level");
    assert!(h.state().doc.vectors.iter().all(|v| !v.is_group()));
}

/// Three shapes: a part's outline and hole in a piece, and something beside them.
fn sheet_with_a_piece() -> (Harness<'static, App>, u64, u64, u64) {
    let mut h = job_with_square([3.0, 3.0]);
    let outline = h.state().doc.vectors[0].id;
    let c = [3.0, 3.0];
    let hole = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.25))], Prim::Circle { c, r: 0.25 });
    let c = [8.0, 8.0];
    let beside = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.25))], Prim::Circle { c, r: 0.25 });
    h.state_mut().selected = vec![outline, hole];
    h.run();
    click(&mut h, "Make Piece");
    (h, outline, hole, beside)
}

fn left_of(h: &Harness<'static, App>, id: u64) -> f64 {
    crate::geom::bbox(&h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().paths).unwrap().0[0]
}

#[test]
fn a_piece_keeps_each_shape_selectable_alone_but_moves_together() {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    assert_eq!(h.state().doc.vectors.len(), 3, "still separate shapes");
    // Clicking the hole takes the hole alone, as a toolpath needs.
    h.state_mut().selected.clear();
    h.run();
    let at = h.state().to_screen([3.25, 3.0]);
    click_at(&mut h, at);
    assert_eq!(h.state().selected, [hole]);
    // Moving it brings the rest of the piece, and nothing else.
    let (o, b, s) = (left_of(&h, outline), left_of(&h, hole), left_of(&h, beside));
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert!((left_of(&h, outline) - o - 0.05).abs() < 1e-9 && (left_of(&h, hole) - b - 0.05).abs() < 1e-9);
    assert_eq!(left_of(&h, beside), s);
    assert_eq!(h.state().selected, [hole], "still only the hole selected");
    // Its own circle goes along as a circle.
    let v = h.state().doc.vectors.iter().find(|v| v.id == hole).unwrap();
    assert!(matches!(v.prim, Prim::Circle { c, .. } if (c[0] - 3.05).abs() < 1e-9));
}

#[test]
fn a_shape_can_be_adjusted_alone_in_its_piece() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    h.state_mut().selected = vec![hole];
    h.run();
    click(&mut h, "Adjust Alone");
    let (o, b) = (left_of(&h, outline), left_of(&h, hole));
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(left_of(&h, outline), o, "the rest stays where it is");
    assert!((left_of(&h, hole) - b - 0.05).abs() < 1e-9);
    click(&mut h, "Adjust Alone");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert!((left_of(&h, outline) - o - 0.05).abs() < 1e-9, "linked again");
    // Its size only ever changes for what's selected.
    let width = |h: &Harness<'static, App>, id| {
        let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().paths).unwrap();
        hi[0] - lo[0]
    };
    let before = width(&h, outline);
    h.state_mut().resize_each(Some(1.0), None, None, Datum::Centre);
    h.run();
    assert!((width(&h, hole) - 1.0).abs() < 1e-6);
    assert_eq!(width(&h, outline), before);
}

#[test]
fn a_piece_can_be_selected_whole_and_released() {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    h.state_mut().selected = vec![hole];
    h.run();
    click(&mut h, "Select Piece");
    let mut got = h.state().selected.clone();
    got.sort_unstable();
    let mut want = vec![outline, hole];
    want.sort_unstable();
    assert_eq!(got, want);
    click(&mut h, "Release Piece");
    assert!(h.state().doc.vectors.iter().all(|v| v.piece.is_none()));
    h.state_mut().selected = vec![hole];
    h.run();
    let o = left_of(&h, outline);
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(left_of(&h, outline), o, "no longer linked");
    let _ = beside;
}

#[test]
fn a_group_becomes_a_piece_of_its_own_shapes_with_their_kinds() {
    let mut h = job_with_square([3.0, 3.0]);
    let outline = h.state().doc.vectors[0].id;
    let c = [3.0, 3.0];
    let hole = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.25))], Prim::Circle { c, r: 0.25 });
    h.state_mut().selected = vec![outline, hole];
    h.run();
    click(&mut h, "Group");
    assert_eq!(h.state().doc.vectors.len(), 1);
    click(&mut h, "Make Piece");
    assert_eq!(h.state().doc.vectors.len(), 2);
    let piece = h.state().doc.vectors[0].piece;
    assert!(piece.is_some() && h.state().doc.vectors.iter().all(|v| v.piece == piece && !v.is_group()));
    assert!(matches!(h.state().doc.vectors.iter().find(|v| v.id == hole).unwrap().prim, Prim::Circle { .. }));
}

#[test]
fn a_whole_piece_copied_is_a_new_piece_and_part_of_one_stays_in_it() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    h.state_mut().selected = vec![outline, hole];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.run();
    let copies = h.state().selected.clone();
    let piece_of = |h: &Harness<'static, App>, id: u64| h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().piece;
    assert_eq!(copies.len(), 2);
    assert_eq!(piece_of(&h, copies[0]), piece_of(&h, copies[1]));
    assert!(piece_of(&h, copies[0]).is_some());
    assert_ne!(piece_of(&h, copies[0]), piece_of(&h, outline), "moves on its own");
    // Another hole copied within the piece belongs to it.
    h.state_mut().selected = vec![hole];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.run();
    let extra = h.state().selected[0];
    assert_eq!(piece_of(&h, extra), piece_of(&h, outline));
}

#[test]
fn moving_a_piece_leaves_its_toolpaths_until_recalculate_and_the_file_says_why() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    h.state_mut().selected = vec![hole];
    h.state_mut().start_op(crate::toolpath::OpKind::Drill);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.state_mut().view3d = false;
    h.run();
    let moves = h.state().doc.toolpaths[0].moves.clone();
    assert_eq!(h.state().doc.toolpaths[0].op.source, [hole], "made from the hole alone");
    h.state_mut().selected = vec![outline];
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves), "not recalculated");
    assert_eq!(h.state().behind, [0], "the hole went with the piece");
    assert!(h.state_mut().output_problem(None).is_some_and(|m| m.contains("Recalculate")));
    click(&mut h, "Recalculate all toolpaths");
    assert!(h.state().behind.is_empty());
    assert_eq!(h.state_mut().output_problem(None), None);
}

#[test]
fn a_piece_is_saved_with_the_project() {
    let (h, outline, hole, _) = sheet_with_a_piece();
    let text = serde_json::to_string(&h.state().doc).unwrap();
    let back: crate::app::Doc = serde_json::from_str(&text).unwrap();
    let piece = |id: u64| back.vectors.iter().find(|v| v.id == id).unwrap().piece;
    assert!(piece(outline).is_some());
    assert_eq!(piece(outline), piece(hole));
}

#[test]
fn a_piece_leaves_every_shape_on_its_own_layer() {
    let mut h = job_with_square([3.0, 3.0]);
    let outline = h.state().doc.vectors[0].id;
    h.state_mut().doc.layers.push(crate::app::Layer::named("Holes"));
    h.state_mut().doc.layers.push(crate::app::Layer::named("Third"));
    h.state_mut().doc.active_layer = 1;
    let c = [3.0, 3.0];
    let hole = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.25))], Prim::Circle { c, r: 0.25 });
    h.state_mut().doc.active_layer = 2;
    let layer = |h: &Harness<'static, App>, id: u64| h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().layer;
    let (first, second) = (layer(&h, outline), layer(&h, hole));
    assert_ne!(first, second);
    h.state_mut().selected = vec![outline, hole];
    h.run();
    click(&mut h, "Make Piece");
    assert_eq!((layer(&h, outline), layer(&h, hole)), (first, second), "made directly");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!((layer(&h, outline), layer(&h, hole)), (first, second), "after moving");
    click(&mut h, "Release Piece");
    assert_eq!((layer(&h, outline), layer(&h, hole)), (first, second), "after releasing");
    // A simple group holds them on one layer for now, but a piece made from it gives them back.
    h.state_mut().selected = vec![outline, hole];
    h.run();
    click(&mut h, "Group");
    assert_eq!(h.state().doc.vectors[0].layer, 2, "the active one, as they had none in common");
    click(&mut h, "Make Piece");
    assert_eq!((layer(&h, outline), layer(&h, hole)), (first, second), "made from a group");
}

#[test]
fn changing_the_material_asks_before_recalculating() {
    let mut h = square_with_profile();
    let moves = h.state().doc.toolpaths[0].moves.clone();
    // Nothing changed: nothing is asked, and nothing is worked out.
    let before = h.state().doc.clone();
    h.state_mut().material_accepted(&before);
    h.run();
    assert!(!h.state().ask_recalc);
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves));
    // Thicker: it asks, and still hasn't worked anything out.
    h.state_mut().doc.job.thickness += 0.1;
    h.state_mut().material_accepted(&before);
    h.run();
    assert!(h.state().ask_recalc);
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves));
    click(&mut h, "Later");
    assert!(!h.state().ask_recalc);
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves), "still as it was");
    h.run();
    assert_eq!(h.state().behind, [0], "and marked out of date");
    assert!(h.state_mut().output_problem(None).is_some());
    // Asked again, the answer can be now.
    h.state_mut().material_accepted(&before);
    h.run();
    click(&mut h, "Recalculate Now");
    assert!(!h.state().all_behind);
    assert!(!std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves), "worked out");
    h.run();
    assert!(h.state().behind.is_empty());
    assert_eq!(h.state_mut().output_problem(None), None);
    // With no toolpaths there's nothing to ask.
    h.state_mut().doc.toolpaths.clear();
    h.state_mut().doc.job.thickness += 0.1;
    h.state_mut().material_accepted(&before);
    assert!(!h.state().ask_recalc);
}

#[test]
fn a_click_on_one_of_several_selected_shapes_keeps_them_all() {
    let mut h = job_with_square([5.0, 5.0]);
    let first = h.state().doc.vectors[0].id;
    let second = {
        let a = h.state_mut();
        let mut copy = a.doc.vectors[0].clone();
        copy.id = a.doc.new_id();
        let far = a.doc.job.width / 4.0;
        copy.paths.iter_mut().flat_map(|p| p.pts.iter_mut()).for_each(|p| p[1] -= far);
        let id = copy.id;
        a.doc.vectors.push(copy);
        a.selected = vec![first, id];
        id
    };
    h.run();
    let edge = {
        let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
        h.state().to_screen([hi[0], (lo[1] + hi[1]) / 2.0])
    };
    click_at(&mut h, edge);
    assert_eq!(h.state().selected, [first, second]);
    assert_eq!(h.state().handle_rects().len(), 8);
}

#[test]
fn the_select_tool_mirrors_and_turns_from_the_keyboard() {
    let mut h = job_with_square([5.0, 5.0]);
    let shape = |h: &Harness<'static, App>| crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    let first = |h: &Harness<'static, App>| h.state().doc.vectors[0].paths[0].pts[0];
    let (lo, hi) = shape(&h);
    let (w, size) = (h.state().doc.job.width, hi[0] - lo[0]);
    let start = first(&h);

    // About its own middle: it stays where it is, with its points swapped over.
    h.key_press(egui::Key::H);
    h.run();
    assert!((first(&h)[0] - (lo[0] + hi[0] - start[0])).abs() < 1e-9 && (first(&h)[1] - start[1]).abs() < 1e-9);
    assert!((shape(&h).0[0] - lo[0]).abs() < 1e-9);
    h.key_press(egui::Key::H);
    h.run();
    h.key_press(egui::Key::V);
    h.run();
    assert!((first(&h)[1] - (lo[1] + hi[1] - start[1])).abs() < 1e-9 && (first(&h)[0] - start[0]).abs() < 1e-9);
    h.key_press(egui::Key::V);
    h.run();

    // Shift: about the job's middle.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    h.run();
    assert!((shape(&h).0[0] - (w - hi[0])).abs() < 1e-9);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    h.run();

    // Ctrl: a copy, which ends up selected. Ctrl+V doesn't mirror.
    let original = h.state().doc.vectors[0].id;
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::H);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 2);
    assert!(!h.state().selected.contains(&original));
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::V);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 2, "Ctrl+V doesn't make a mirrored copy");
    h.state_mut().doc.vectors.truncate(1);
    h.state_mut().selected = vec![original];
    h.run();

    // 9 anticlockwise and 0 clockwise, by 45 degrees about the middle.
    h.key_press(egui::Key::Num9);
    h.run();
    let (a, b) = shape(&h);
    assert!((b[0] - a[0] - size * 2f64.sqrt()).abs() < 1e-6);
    assert!(((a[0] + b[0]) - (lo[0] + hi[0])).abs() < 1e-6);
    let turned = first(&h);
    let (x, y) = (start[0] - (lo[0] + hi[0]) / 2.0, start[1] - (lo[1] + hi[1]) / 2.0);
    let k = std::f64::consts::FRAC_1_SQRT_2;
    assert!((turned[0] - ((lo[0] + hi[0]) / 2.0 + (x - y) * k)).abs() < 1e-6, "anticlockwise");
    h.key_press(egui::Key::Num0);
    h.run();
    assert!((first(&h)[0] - start[0]).abs() < 1e-6 && (first(&h)[1] - start[1]).abs() < 1e-6);
}

#[test]
fn escape_drops_the_selection_only_from_the_select_tool() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().tool = crate::app::Tool2::Nodes;
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().tool == crate::app::Tool2::Select);
    assert_eq!(h.state().selected, [id], "it only left the other tool");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().selected.is_empty());
}

#[test]
fn clicking_what_is_selected_changes_between_moving_and_resizing() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected.clear();
    h.run();
    let edge = {
        let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
        h.state().to_screen([hi[0], (lo[1] + hi[1]) / 2.0])
    };
    click_at(&mut h, edge);
    assert_eq!(h.state().selected, [id]);
    assert!(h.state().handle_rects().is_empty(), "the first click is for moving");
    click_at(&mut h, edge);
    assert_eq!(h.state().selected, [id], "still selected");
    assert_eq!(h.state().handle_rects().len(), 8, "the next shows the handles");
    click_at(&mut h, edge);
    assert!(h.state().handle_rects().is_empty(), "and the next takes them away");
    click_at(&mut h, edge);
    assert_eq!(h.state().handle_rects().len(), 8);
    // Selecting afresh starts again for moving.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().handle_rects().is_empty());
    click_at(&mut h, edge);
    let empty = h.state().to_screen([0.2, 0.2]);
    click_at(&mut h, empty);
    assert!(h.state().selected.is_empty());
    click_at(&mut h, edge);
    assert_eq!(h.state().selected, [id]);
    assert!(h.state().handle_rects().is_empty(), "what was on before doesn't come back");
}

#[test]
fn a_turned_square_can_be_resized_by_its_handles() {
    // Resizing a turned square used to stretch it millions of times from a side handle and stop the program.
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().handles_on = Some(vec![id]);
    h.run();
    let turn = h.state().rotate_handles()[2];
    let centre = h.state().to_screen(h.state().rotation_centre().unwrap());
    drag(&mut h, turn, centre + (turn - centre).rot90() * 0.3 + (turn - centre) * 0.7);
    let size = |h: &Harness<'static, App>| {
        let (a, b) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
        (b[0] - a[0], b[1] - a[1])
    };
    for k in 0..8 {
        h.state_mut().selected = vec![id];
        h.state_mut().handles_on = Some(vec![id]);
        h.run();
        let (w, tall) = size(&h);
        let (at, _) = h.state().handle_rects()[k];
        let away = if k >= 4 { if k % 2 == 0 { egui::vec2(0.0, 30.0) } else { egui::vec2(30.0, 0.0) } } else { egui::vec2(25.0, 25.0) };
        drag(&mut h, at, at + away);
        let (nw, nh) = size(&h);
        assert!(nw < 20.0 && nh < 20.0, "handle {k}: {nw} x {nh}");
        if k == 4 || k == 6 {
            assert!((nw - w).abs() < 1e-6 && (nh - tall).abs() > 1e-3, "a top or bottom handle only changes the height: {w} {tall} -> {nw} {nh}");
        }
        if k == 5 || k == 7 {
            assert!((nh - tall).abs() < 1e-6 && (nw - w).abs() > 1e-3, "a side handle only changes the width: {w} {tall} -> {nw} {nh}");
        }
    }
}

/// Dashes are drawn only where the line crosses what shows, however far it reaches.
#[test]
fn a_huge_dashed_outline_draws_only_what_shows() {
    let view = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let far = [egui::pos2(-1e12, 300.0), egui::pos2(1e12, 300.0), egui::pos2(f32::NAN, 0.0), egui::pos2(10.0, 10.0)];
    let shapes = crate::canvas::dashed_within(&far, egui::Stroke::new(1.0, egui::Color32::BLACK), 4.0, 3.0, view);
    assert!(!shapes.is_empty() && shapes.len() < 200, "{} dashes", shapes.len());
}

#[test]
fn with_handles_a_side_stretches_one_way_and_without_them_it_only_moves() {
    let mut h = job_with_square([5.0, 5.0]);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    let (w, tall) = (hi[0] - lo[0], hi[1] - lo[1]);
    let side = h.state().to_screen([hi[0], (lo[1] + hi[1]) / 2.0]);
    let further = h.state().to_screen([hi[0] + 1.0, (lo[1] + hi[1]) / 2.0]);
    let size = |h: &Harness<'static, App>| {
        let (a, b) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
        (a, b[0] - a[0], b[1] - a[1])
    };
    let id = h.state().doc.vectors[0].id;
    // Selected, no handles: dragging there moves it, keeping its size.
    h.state_mut().selected = vec![id];
    h.state_mut().handles_on = None;
    h.run();
    drag(&mut h, side, further);
    let (at, nw, nh) = size(&h);
    assert!((nw - w).abs() < 1e-6 && (nh - tall).abs() < 1e-6, "same size");
    assert!(at[0] > lo[0] + 0.5, "moved");
    h.state_mut().do_undo();
    h.state_mut().selected = vec![id];
    h.run();
    // With them: the left side stays, and only the width changes.
    h.state_mut().handles_on = Some(vec![id]);
    h.run();
    // The handle is drawn 8 pixels out from the side, as the original does.
    let out = egui::vec2(8.0, 0.0);
    assert!((h.state().handle_rects()[5].0 - (side + out)).length() < 0.01);
    drag(&mut h, side + out, further + out);
    let (at, nw, nh) = size(&h);
    assert!((at[0] - lo[0]).abs() < 1e-6, "the far side stays");
    assert!((nw - w - 1.0).abs() < 0.1, "{nw}");
    assert!((nh - tall).abs() < 1e-6, "the height doesn't change");
}

/// The test's language goes back to English when it ends, whatever happens.
struct InFrench;
impl InFrench {
    fn start() -> InFrench {
        crate::i18n::set_language(crate::i18n::Lang::French);
        InFrench
    }
}
impl Drop for InFrench {
    fn drop(&mut self) {
        crate::i18n::set_language(crate::i18n::Lang::English);
    }
}

#[test]
fn the_keys_can_be_changed_from_the_preferences() {
    use crate::keys::Action;
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().prefs_open = true;
    h.run();

    // Press a new key for Delete.
    h.state_mut().prefs_waiting = Some(Action::Delete);
    h.run();
    h.key_press(egui::Key::K);
    h.run();
    assert_eq!(h.state().prefs_waiting, None);
    assert_eq!(h.state().keys.text(Action::Delete), "K");

    // A key that's taken is refused and it says by what, and the list keeps waiting.
    h.state_mut().prefs_waiting = Some(Action::Delete);
    h.run();
    h.key_press(egui::Key::G);
    h.run();
    assert!(h.state().prefs_note.contains("Group"), "{}", h.state().prefs_note);
    assert_eq!(h.state().keys.text(Action::Delete), "K");
    assert_eq!(h.state().prefs_waiting, Some(Action::Delete));

    // So is one with a use of its own, and Escape gives up.
    h.key_press(egui::Key::Space);
    h.run();
    assert!(h.state().prefs_note.contains("own use"), "{}", h.state().prefs_note);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().prefs_waiting, None);
    assert_eq!(h.state().keys.text(Action::Delete), "K");

    // While the window is open, keys do nothing to the drawing.
    h.key_press(egui::Key::K);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 1);

    // Afterwards the new key works, and the old one doesn't.
    h.state_mut().prefs_open = false;
    h.state_mut().selected = vec![id];
    h.run();
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 1, "that's not the key any more");
    h.key_press(egui::Key::K);
    h.run();
    assert!(h.state().doc.vectors.is_empty());

    // The second Delete key is still there until it's removed too, and the defaults come back.
    h.state_mut().prefs_open = true;
    h.run();
    click(&mut h, "Restore Default Keys");
    assert_eq!(h.state().keys, crate::keys::Keys::default());
}

#[test]
fn a_key_can_be_chosen_by_clicking_its_button() {
    use crate::keys::Action;
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().prefs_open = true;
    h.run();
    click(&mut h, "Ctrl+S");
    assert_eq!(h.state().prefs_waiting, Some(Action::Save));
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::K);
    h.run();
    assert_eq!(h.state().keys.text(Action::Save), "Ctrl+Shift+K");
}

#[test]
fn the_language_changes_at_once_from_the_preferences() {
    let _english_again = InFrench::start();
    crate::i18n::set_language(crate::i18n::Lang::English);
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().prefs_open = true;
    h.run();
    h.get_by_label("Language");
    click(&mut h, "Français");
    h.run();
    assert_eq!(crate::i18n::language(), crate::i18n::Lang::French);
    h.get_by_label("Langue");
    h.get_by_label("Raccourcis clavier");
    click(&mut h, "English");
    h.run();
    h.get_by_label("Language");
}

#[test]
fn the_interface_can_be_used_in_french() {
    let _french = InFrench::start();
    let mut h = harness();
    click(&mut h, "Commencer un projet");
    h.get_by_label("Une face");
    click(&mut h, "OK");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 2.0, 2.0, 0.0))], crate::app::Prim::Free);
    h.state_mut().selected = vec![id];
    h.run();
    h.get_by_label("Modifier les formes");
    h.get_by_label("Créer une pièce");
    assert_eq!(h.state().doc.layers[0].name, "Couche 1");
    // A message with something filled in.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    h.run();
    assert!(h.state().status.starts_with("Sélection déplacée de"), "{}", h.state().status);
    assert!(!h.state().status.contains("Moved"));
    // The saved quality isn't translated, only what's shown of it.
    assert_eq!(h.state().quality3d.name(), crate::app::Quality3d::Balanced.name());
}

#[test]
fn files_for_the_machine_are_the_same_in_either_language() {
    let mut a = App::default();
    a.load_sample();
    let english = a.gcode_text(None).unwrap();
    let _french = InFrench::start();
    assert_eq!(a.gcode_text(None).unwrap(), english);
}

/// What runs out of a side panel or the window on a screen of the program, not counting the menu bar
/// and rulers, which go across the panels' edges anyway.
fn out_of_place(state: &str) -> Vec<String> {
    use egui_kittest::kittest::NodeT;
    let mut h = harness();
    crate::open_state(h.state_mut(), state);
    for _ in 0..4 {
        h.step();
    }
    let (w, left, right) = (1910.0, (crate::ui::RAIL_W + crate::ui::SIDE_W) as f64, 1910.0 - crate::ui::RIGHT_W as f64);
    let mut found = Vec::new();
    for node in h.root().children_recursive() {
        let node = node.accesskit_node();
        let Some(b) = node.bounding_box() else { continue };
        if b.y0 > 34.0 && b.x1 - b.x0 < 1000.0 && (b.x1 > w + 1.0 || (b.x0 < left - 1.0 && b.x1 > left + 1.0) || (b.x0 < right - 1.0 && b.x1 > right + 1.0)) {
            found.push(format!("{state}: {:?} {:?} {:.0}..{:.0}", node.label(), node.value(), b.x0, b.x1));
        }
    }
    found
}

#[test]
fn french_wordings_stay_inside_their_panels() {
    let mut too_wide = Vec::new();
    for state in ["smooth", "modeltrim", "textfit", "fonts", "variables", "printsetup", "help", "shadows", "checks", "drawmoves", "backups", "feedback", "crashes", "merge", "rotate", "support", "missinglayers", "pieces", "savepiece", "3d", "align", "array", "board", "boardalert", "boardmaterial", "carved", "chamfer", "circle", "clipart", "corners", "cut", "distort", "distorted", "drawing", "drill", "editcircle", "engrave", "finish", "flute", "grid", "grid3d", "grouped", "handles", "job", "layers", "leads", "lines", "material", "material-changed", "millimetres", "model", "modeling", "move", "order", "origin", "origin_set", "passes", "paths2d", "piece", "pocket", "posts", "preferences", "profile", "progress", "ramp", "relief", "relief3d", "rough", "rules", "save", "selector", "sharp3d", "sharp3dfar", "similar", "sketch", "startat", "sweep", "tabs", "tabs-before", "tee_after", "tee_before", "through", "toodeep", "toolrates", "tools", "trace", "traced", "trim_after", "trim_before", "unsaved", "vcarve", "zero"] {
        let english = out_of_place(state).len();
        let french = {
            let _french = InFrench::start();
            out_of_place(state)
        };
        if french.len() > english {
            too_wide.extend(french);
        }
    }
    assert!(too_wide.is_empty(), "{}", too_wide.join("\n"));
}

#[test]
fn the_first_keys_open_the_tools_and_views() {
    use egui::{Key, Modifiers};
    use crate::app::{LeftTab, Tool2};
    let mut h = job_with_square([1.0, 1.0]);
    let id = h.state().doc.vectors[0].id;
    let form = |h: &Harness<'static, App>| match &h.state().left {
        LeftForm::Xform(f) => Some(f.kind),
        _ => None,
    };
    for (key, kind) in [(Key::M, Xform::Move), (Key::T, Xform::Scale), (Key::R, Xform::Rotate), (Key::J, Xform::Join), (Key::F10, Xform::Align)] {
        h.key_press(key);
        h.run();
        assert_eq!(form(&h), Some(kind), "{key:?}");
    }
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().selected = vec![id];
    h.run();

    // F9 puts the selection in the middle of the material.
    h.key_press(Key::F9);
    h.run();
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    let job = h.state().doc.job.clone();
    assert!((lo[0] + hi[0] - job.width).abs() < 1e-9 && (lo[1] + hi[1] - job.height).abs() < 1e-9);

    // N goes to editing points and back.
    h.key_press(Key::N);
    h.run();
    assert!(h.state().tool == Tool2::Nodes);
    h.key_press(Key::N);
    h.run();
    assert!(h.state().tool == Tool2::Select);

    // The tabs and views.
    h.key_press_modifiers(Modifiers::COMMAND, Key::L);
    h.run();
    assert!(h.state().left_tab == LeftTab::Layers);
    h.key_press(Key::F8);
    h.run();
    assert!(h.state().left_tab == LeftTab::Modeling);
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run();
    assert!(h.state().left_tab == LeftTab::Drawing);
    h.key_press(Key::F3);
    h.run();
    assert!(h.state().view3d);
    h.key_press(Key::F2);
    h.run();
    assert!(!h.state().view3d);
    let open = h.state().left_open;
    h.key_press(Key::F11);
    h.run();
    assert_ne!(h.state().left_open, open);
}

#[test]
fn ctrl_shift_v_makes_a_mirrored_copy_about_the_job_centre() {
    let mut h = job_with_square([1.0, 1.0]);
    let height = h.state().doc.job.height;
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::V);
    h.run();
    assert_eq!(h.state().doc.vectors.len(), 2);
    let (a, b) = crate::geom::bbox(&h.state().doc.vectors[1].paths).unwrap();
    assert!((a[1] - (height - hi[1])).abs() < 1e-9 && (b[1] - (height - lo[1])).abs() < 1e-9);
}

#[test]
fn letters_start_the_drawing_tools() {
    use crate::app::{DrawKind, LeftForm};
    let mut h = job_with_square([1.0, 1.0]);
    for (key, kind) in [
        (egui::Key::L, DrawKind::Polyline),
        (egui::Key::C, DrawKind::Circle),
        (egui::Key::E, DrawKind::Ellipse),
        (egui::Key::Q, DrawKind::Rect),
        (egui::Key::P, DrawKind::Polygon),
        (egui::Key::I, DrawKind::Star),
        (egui::Key::A, DrawKind::Arc),
        (egui::Key::B, DrawKind::Curve),
        (egui::Key::S, DrawKind::Spiral),
        (egui::Key::X, DrawKind::Text),
    ] {
        h.key_press(key);
        h.run();
        assert!(matches!(&h.state().left, LeftForm::Draw(f) if f.kind == kind), "{key:?}");
    }
    // With Ctrl, they stay what they were: Ctrl+A selects everything, and doesn't draw an arc.
    h.key_press(egui::Key::Escape);
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert!(matches!(h.state().left, LeftForm::None));
    assert_eq!(h.state().selected.len(), h.state().doc.vectors.len());
}

#[test]
fn locked_guides_stay_where_they_are() {
    let mut h = job_with_square([1.0, 1.0]);
    h.state_mut().doc.guides.push(crate::snap::Guide { vertical: true, at: 6.0 });
    h.run();
    let at = h.state().to_screen([6.0, 6.0]);
    assert_eq!(h.state().guide_at(at), Some(0));
    h.state_mut().guides_locked = true;
    assert_eq!(h.state().guide_at(at), None);
}

#[test]
fn the_menus_have_the_entries_of_the_original() {
    let mut h = job_with_square([1.0, 1.0]);
    for (menu, entries) in [
        ("File", &["Close", "Increment and Save", "Import", "Export", "Recent Files", "Print..."][..]),
        ("Edit", &["Selection", "Align Selected Objects", "Join Vectors", "Curve Fit Vectors", "Snap Options", "Preferences..."][..]),
        ("Model", &["Create Component", "Export as STL", "Sculpt Visible Model", "Selected Component Properties"][..]),
        ("Toolpaths", &["Toolpath Drawing", "Preview Simulation Quality", "Templates", "Install Post Processor..."][..]),
        ("View", &["Guide Lines", "Refresh 2D View", "Draw Origin", "Save Shaded Image"][..]),
        ("Help", &["Keyboard Shortcuts", "Help Contents", "About"][..]),
    ] {
        h.get_all_by_role_and_label(egui::accesskit::Role::Button, menu).next().unwrap().click();
        h.run();
        for entry in entries {
            h.get_by_label(entry);
        }
        h.key_press(egui::Key::Escape);
        h.run();
    }
}

fn box_of(h: &Harness<'static, App>, id: u64) -> ([f64; 2], [f64; 2]) {
    crate::geom::bbox(&h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().paths).unwrap()
}

#[test]
fn a_whole_piece_is_aligned_once_and_keeps_its_shape() {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    let apart = left_of(&h, hole) - left_of(&h, outline);
    h.state_mut().selected = vec![outline, hole, beside];
    h.state_mut().align_to_last_edge(crate::edit::Edge::Left, true);
    h.run();
    let target = left_of(&h, beside);
    assert!((left_of(&h, outline) - target).abs() < 1e-9, "the piece's own left side reaches it");
    assert!((left_of(&h, hole) - left_of(&h, outline) - apart).abs() < 1e-9, "the piece didn't change");
    assert!((target - 7.75).abs() < 1e-9, "the shape it's lined up with stays");
}

#[test]
fn a_shape_is_aligned_alone_to_another_of_its_piece() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    let before = box_of(&h, outline);
    h.state_mut().selected = vec![hole, outline];
    h.state_mut().align_to_last_edge(crate::edit::Edge::Left, true);
    h.run();
    assert_eq!(box_of(&h, outline), before, "the outline stays");
    assert!((left_of(&h, hole) - before.0[0]).abs() < 1e-9);
}

#[test]
fn turning_or_flipping_a_shape_takes_its_piece_along_unless_adjusting() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    // The hole, off to one side, then shows whether the piece turned around its own middle.
    h.state_mut().adjust_piece = true;
    h.state_mut().selected = vec![hole];
    h.state_mut().translate_selected(0.5, 0.0);
    h.state_mut().adjust_piece = false;
    h.run();
    let (lo, hi) = box_of(&h, outline);
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let centre = |h: &Harness<'static, App>, id: u64| {
        let (a, b) = box_of(h, id);
        [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
    };
    let hole_at = centre(&h, hole);
    h.state_mut().selected = vec![hole];
    h.state_mut().turn_selected(90.0);
    h.run();
    let turned = centre(&h, hole);
    let (x, y) = (hole_at[0] - middle[0], hole_at[1] - middle[1]);
    assert!((turned[0] - (middle[0] - y)).abs() < 1e-6 && (turned[1] - (middle[1] + x)).abs() < 1e-6, "{turned:?}");
    assert!((centre(&h, outline)[0] - middle[0]).abs() < 1e-6, "the outline turned on the spot");
    // Flipping the same way, and it comes back.
    let before = centre(&h, hole);
    h.state_mut().mirror(true, None, false, false);
    h.run();
    assert!((centre(&h, hole)[0] - (2.0 * middle[0] - before[0])).abs() < 1e-6);
    h.state_mut().mirror(true, None, false, false);
    h.run();
    // With adjusting on, only the shape chosen turns.
    h.state_mut().adjust_piece = true;
    let outline_before = box_of(&h, outline);
    let hole_before = centre(&h, hole);
    h.state_mut().turn_selected(45.0);
    h.run();
    assert_eq!(box_of(&h, outline), outline_before);
    assert!((centre(&h, hole)[0] - hole_before[0]).abs() < 1e-6, "on its own middle");
}

#[test]
fn a_mirrored_copy_of_a_hole_lands_across_its_piece_and_belongs_to_it() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    h.state_mut().adjust_piece = true;
    h.state_mut().selected = vec![hole];
    h.state_mut().translate_selected(0.5, 0.0);
    h.state_mut().adjust_piece = false;
    let (lo, hi) = box_of(&h, outline);
    let (a, b) = box_of(&h, hole);
    h.state_mut().mirror(true, None, false, true);
    h.run();
    let copy = h.state().selected[0];
    assert_ne!(copy, hole);
    let (c, d) = box_of(&h, copy);
    assert!((c[0] - (lo[0] + hi[0] - b[0])).abs() < 1e-6 && (d[0] - (lo[0] + hi[0] - a[0])).abs() < 1e-6);
    let piece = |id: u64| h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().piece;
    assert_eq!(piece(copy), piece(outline));
}

#[test]
fn centring_one_shape_centres_its_piece() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    h.state_mut().adjust_piece = true;
    h.state_mut().selected = vec![hole];
    h.state_mut().translate_selected(0.5, 0.0);
    h.state_mut().adjust_piece = false;
    h.state_mut().align_selected(Some(0), Some(0));
    h.run();
    let (lo, hi) = box_of(&h, outline);
    let job = h.state().doc.job.clone();
    assert!((lo[0] + hi[0] - job.width).abs() < 1e-9 && (lo[1] + hi[1] - job.height).abs() < 1e-9);
}

#[test]
fn nesting_and_spacing_count_a_piece_once() {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    let apart = left_of(&h, hole) - left_of(&h, outline);
    h.state_mut().selected = vec![outline, hole, beside];
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    h.run();
    assert!((left_of(&h, hole) - left_of(&h, outline) - apart).abs() < 1e-9, "a piece isn't pulled apart");
    assert!(h.state().status.contains('2'), "two parts: {}", h.state().status);
    // Three holes of one piece are spaced inside it.
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    let add = |h: &mut Harness<'static, App>, x: f64| {
        let c = [x, 3.0];
        let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.1))], Prim::Circle { c, r: 0.1 });
        let piece = h.state().doc.vectors.iter().find(|v| v.id == outline).unwrap().piece;
        h.state_mut().doc.put_in_piece(&[id], piece);
        id
    };
    let (two, three) = (add(&mut h, 3.2), add(&mut h, 3.8));
    let before = box_of(&h, outline);
    h.state_mut().selected = vec![hole, two, three];
    h.state_mut().space_selection(true);
    h.run();
    assert_eq!(box_of(&h, outline), before, "the outline wasn't selected");
    let middle = |h: &Harness<'static, App>, id: u64| {
        let (a, b) = box_of(h, id);
        (a[0] + b[0]) / 2.0
    };
    assert!((middle(&h, two) - (middle(&h, hole) + middle(&h, three)) / 2.0).abs() < 1e-6);
}

#[test]
fn what_is_made_from_a_piece_stays_in_it_and_one_shape_left_is_no_piece() {
    let (mut h, outline, hole, _) = sheet_with_a_piece();
    let piece = h.state().doc.vectors.iter().find(|v| v.id == outline).unwrap().piece;
    let c = [3.0, 2.0];
    let third = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle(c, 0.25))], Prim::Circle { c, r: 0.25 });
    h.state_mut().doc.put_in_piece(&[third], piece);
    h.state_mut().selected = vec![outline, third];
    h.state_mut().offset_selected(0.1, true);
    let offset = h.state().selected[0];
    assert_eq!(h.state().doc.vectors.iter().find(|v| v.id == offset).unwrap().piece, piece);
    // Deleting all but one ends the piece.
    h.state_mut().selected = vec![outline, third, offset];
    h.state_mut().delete_selected();
    h.run();
    assert_eq!(h.state().doc.vectors.iter().find(|v| v.id == hole).unwrap().piece, None);
    // And undo brings it all back.
    h.state_mut().do_undo();
    h.run();
    assert_eq!(h.state().doc.vectors.iter().find(|v| v.id == hole).unwrap().piece, piece);
}

#[test]
fn a_double_click_selects_the_whole_piece_and_keys_work_too() {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    h.state_mut().selected.clear();
    h.run();
    let (a, b) = box_of(&h, hole);
    let at = h.state().to_screen([b[0], (a[1] + b[1]) / 2.0]);
    // The test's clock takes big steps, so a double click has to be allowed more time here. The
    // wait keeps the click on the button before out of it, or this would count as a triple click.
    h.ctx.options_mut(|o| o.input_options.max_double_click_delay = 3.0);
    for _ in 0..20 {
        h.step();
    }
    click_at(&mut h, at);
    assert_eq!(h.state().selected, [hole], "one click is one shape");
    click_at(&mut h, at);
    let mut got = h.state().selected.clone();
    got.sort_unstable();
    let mut want = vec![outline, hole];
    want.sort_unstable();
    assert_eq!(got, want);
    assert!(!got.contains(&beside) && h.state().handle_rects().is_empty());
    // A second click taking its time still changes between moving and resizing.
    h.ctx.options_mut(|o| o.input_options.max_double_click_delay = 0.0);
    h.state_mut().selected.clear();
    h.run();
    click_at(&mut h, at);
    assert_eq!(h.state().selected, [hole]);
    click_at(&mut h, at);
    assert_eq!(h.state().selected, [hole]);
    assert_eq!(h.state().handle_rects().len(), 8);
    // The keys.
    h.state_mut().selected = vec![hole];
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::G);
    h.run();
    assert_eq!(h.state().selected.len(), 2);
    h.key_press(egui::Key::D);
    h.run();
    assert!(h.state().adjust_piece);
    h.key_press(egui::Key::D);
    h.run();
    assert!(!h.state().adjust_piece);
    h.key_press_modifiers(egui::Modifiers::COMMAND | egui::Modifiers::ALT, egui::Key::G);
    h.run();
    assert!(h.state().doc.vectors.iter().all(|v| v.piece.is_none()));
    h.state_mut().selected = vec![outline, hole];
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::G);
    h.run();
    assert!(h.state().doc.vectors.iter().find(|v| v.id == outline).unwrap().piece.is_some());
}

/// A sheet with the piece, its hole drilled and its outline cut.
fn cut_piece() -> (Harness<'static, App>, u64, u64, u64) {
    let (mut h, outline, hole, beside) = sheet_with_a_piece();
    h.state_mut().selected = vec![hole];
    h.state_mut().start_op(crate::toolpath::OpKind::Drill);
    h.state_mut().calculate_op();
    h.state_mut().selected = vec![outline];
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.state_mut().view3d = false;
    h.state_mut().selected.clear();
    h.run();
    (h, outline, hole, beside)
}

#[test]
fn a_copied_piece_joins_the_toolpaths_of_what_it_came_from() {
    let (mut h, outline, hole, beside) = cut_piece();
    let moves = h.state().doc.toolpaths[0].moves.clone();
    h.state_mut().selected = vec![outline, hole];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.run();
    let copies = h.state().selected.clone();
    assert_eq!(copies.len(), 2);
    let (hole_copy, outline_copy) = (copies[1], copies[0]);
    assert_eq!(h.state().doc.toolpaths[0].op.source, [hole, hole_copy]);
    assert_eq!(h.state().doc.toolpaths[1].op.source, [outline, outline_copy]);
    // Not worked out yet, as for any change: it says so and waits.
    assert!(std::sync::Arc::ptr_eq(&moves, &h.state().doc.toolpaths[0].moves));
    assert_eq!(h.state().behind, [0, 1]);
    h.state_mut().recalculate_all();
    h.run();
    assert!(h.state().behind.is_empty());
    assert_eq!(h.state().doc.toolpaths[0].op.paths.len(), 2);
    // A shape in no piece stays out of it, however it's copied.
    h.state_mut().selected = vec![beside];
    h.state_mut().start_op(crate::toolpath::OpKind::Drill);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.state_mut().view3d = false;
    h.state_mut().selected = vec![beside];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.run();
    assert_eq!(h.state().doc.toolpaths[2].op.source, [beside]);
    // Undo takes the copy back out of the toolpath.
    h.state_mut().selected = vec![outline, hole];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.state_mut().do_undo();
    h.run();
    assert_eq!(h.state().doc.toolpaths[0].op.source, [hole, hole_copy]);
}

#[test]
fn pieces_are_named_and_listed_with_what_cuts_them() {
    let (mut h, outline, hole, _) = cut_piece();
    h.state_mut().left_tab = crate::app::LeftTab::Layers;
    h.run();
    h.get_by_label_contains("2 shapes. Toolpaths: ");
    click(&mut h, "Select Piece 1");
    let mut got = h.state().selected.clone();
    got.sort_unstable();
    let mut want = vec![outline, hole];
    want.sort_unstable();
    assert_eq!(got, want);
    // A copy is the next piece, the name is kept with the project, and goes with the piece.
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.run();
    let rows = h.state().doc.pieces();
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Piece 1", "Piece 2"]);
    let first = rows[0].id;
    h.state_mut().doc.piece_names.insert(first, "Roue avant".into());
    h.run();
    h.get_by_label("Select Roue avant");
    let text = serde_json::to_string(&h.state().doc).unwrap();
    let back: crate::app::Doc = serde_json::from_str(&text).unwrap();
    assert_eq!(back.piece_name(first), "Roue avant");
    click(&mut h, "Release Roue avant");
    assert_eq!(h.state().doc.pieces().len(), 1);
    assert!(!h.state().doc.piece_names.contains_key(&first));
    // A project from before pieces had names still shows them.
    h.state_mut().doc.piece_names.clear();
    assert_eq!(h.state().doc.pieces()[0].name, "Piece 1");
    assert_eq!(h.state().doc.next_piece_name(), "Piece 2");
}

#[test]
fn the_toolpaths_of_one_piece_can_be_recalculated_from_the_list() {
    let (mut h, _, hole, beside) = cut_piece();
    // Something else is out of date too, and stays so.
    h.state_mut().selected = vec![beside];
    h.state_mut().start_op(crate::toolpath::OpKind::Drill);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.state_mut().view3d = false;
    h.state_mut().left_tab = crate::app::LeftTab::Layers;
    h.state_mut().selected = vec![beside];
    h.state_mut().translate_selected(0.5, 0.0);
    h.state_mut().selected = vec![hole];
    h.state_mut().translate_selected(0.5, 0.0);
    h.run();
    assert_eq!(h.state().behind, [0, 1, 2]);
    click(&mut h, "Recalculate for Piece 1");
    assert_eq!(h.state().behind, [2]);
}

#[test]
fn one_piece_is_cut_alone_and_the_project_stays_as_it_was() {
    let (mut h, outline, hole, _) = cut_piece();
    h.state_mut().selected = vec![outline, hole];
    h.state_mut().copy_selected();
    h.state_mut().paste();
    h.state_mut().translate_selected(4.0, 0.0);
    h.state_mut().recalculate_all();
    h.run();
    let rows = h.state().doc.pieces();
    let (first, second) = (rows[0].id, rows[1].id);
    let across = |moves: &[crate::toolpath::Move]| {
        let cut: Vec<f64> = moves.iter().filter(|m| m.p[2] < -1e-9).map(|m| m.p[0]).collect();
        (cut.iter().copied().fold(f64::MAX, f64::min), cut.iter().copied().fold(f64::MIN, f64::max))
    };
    let whole = across(&h.state().doc.toolpaths[1].moves);
    assert!(whole.1 - whole.0 > 4.0, "the shared toolpath cuts both");
    let kept = h.state().doc.toolpaths[1].moves.clone();
    let made = h.state_mut().piece_toolpaths(first, None).unwrap();
    assert_eq!(made.len(), 2);
    let (lo, hi) = across(&made[1].moves);
    assert!(hi - lo < 3.0 && hi < 6.0, "the first alone: {lo} to {hi}");
    let (lo2, _) = across(&h.state_mut().piece_toolpaths(second, None).unwrap()[1].moves);
    assert!(lo2 > lo + 3.0, "the second alone");
    assert!(std::sync::Arc::ptr_eq(&kept, &h.state().doc.toolpaths[1].moves), "the project's toolpath is untouched");
    let text = h.state_mut().piece_gcode(first, None, "piece").unwrap();
    assert!(text.len() < h.state().gcode_text(None).unwrap().len());
    // Only ticked toolpaths, and none that cut it means no file.
    for t in &mut h.state_mut().doc.toolpaths {
        t.visible = false;
    }
    assert!(h.state_mut().piece_gcode(first, None, "piece").is_err());
    assert_eq!(h.state_mut().piece_toolpaths(first, Some(0)).unwrap().len(), 1);
}

#[test]
fn the_save_form_can_choose_one_piece() {
    let (mut h, _, _, _) = cut_piece();
    let first = h.state().doc.pieces()[0].id;
    h.state_mut().right = RightMode::Save;
    h.run();
    h.state_mut().save_piece = Some(first);
    h.run();
    // A piece that's gone isn't kept as the choice.
    h.state_mut().selected = h.state().doc.pieces()[0].shapes.clone();
    h.state_mut().release_piece();
    h.run();
    assert_eq!(h.state().save_piece, None);
}

#[test]
fn a_toolpath_with_no_moves_is_not_written() {
    let (mut h, _, _, _) = cut_piece();
    assert_eq!(h.state_mut().output_problem(None), None);
    h.state_mut().doc.toolpaths[0].moves = std::sync::Arc::new(Vec::new());
    h.state_mut().doc.toolpaths[0].kept_moves = None;
    let name = h.state().doc.toolpaths[0].op.name.clone();
    let problem = h.state_mut().output_problem(None).unwrap();
    assert!(problem.contains("no moves") && problem.contains(&name), "{problem}");
    // Not if it isn't going in the file.
    assert_eq!(h.state_mut().output_problem(Some(1)), None);
}

#[test]
fn the_icon_is_a_rounded_square_with_clear_corners() {
    let icon = crate::icon();
    assert_eq!((icon.width, icon.height), (512, 512));
    let alpha = |x: u32, y: u32| icon.rgba[((y * icon.width + x) * 4 + 3) as usize];
    assert_eq!(alpha(0, 0), 0, "the corner is clear");
    assert_eq!(alpha(256, 256), 255, "the middle is solid");
    // The first screen makes its small copy without trouble.
    let mut h = harness();
    h.run();
    assert!(h.state().start_logo.is_some());
}

/// Two toolpaths with the same tool: a pocket inside a square, and the square's outline; another
/// square further on with the same again, so the file has two parts.
#[cfg(test)]
fn two_parts_in_two_toolpaths(h: &mut Harness<'static, App>) {
    use crate::toolpath::{self, Move, OpKind};
    let square = |x: f64, y: f64, size: f64, z: f64| -> Vec<Move> {
        let mut m = vec![Move { p: [x, y, 0.2], rapid: true, join: false }, Move { p: [x, y, z], rapid: false, join: false }];
        for p in [[x + size, y], [x + size, y + size], [x, y + size], [x, y]] {
            m.push(Move { p: [p[0], p[1], z], rapid: false, join: false });
        }
        m.push(Move { p: [x, y, 0.8], rapid: true, join: false });
        m
    };
    let home = Move { p: [0.0, 0.0, 0.8], rapid: true, join: false };
    let path = |parts: Vec<Vec<Move>>| {
        let mut m = vec![home];
        m.extend(parts.into_iter().flatten());
        m.push(home);
        std::sync::Arc::new(m)
    };
    let tool = toolpath::default_tools().remove(0);
    let inner = path(vec![square(2.0, 2.0, 1.0, -0.1), square(7.0, 2.0, 1.0, -0.1)]);
    let outer = path(vec![square(1.0, 1.0, 3.0, -0.5), square(6.0, 1.0, 3.0, -0.5)]);
    for (name, moves) in [("Holes", inner), ("Outlines", outer)] {
        let op = toolpath::blank_op(OpKind::Profile, name.into(), tool.clone(), 0.5, Vec::new());
        h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op, visible: true, color: [0, 0, 0], moves, kept_moves: None, kept_steps: None, face: 0, sheet: 0 });
    }
}

#[cfg(test)]
fn merged_depths(h: &Harness<'static, App>) -> Vec<(f64, f64)> {
    // Where each cut goes in: its first move down, in order.
    let moves = &h.state().doc.toolpaths.last().unwrap().moves;
    moves.windows(2).filter(|w| w[0].rapid && !w[1].rapid).map(|w| (w[1].p[0], w[1].p[2])).collect()
}

#[test]
fn merging_keeps_each_part_together_in_toolpath_order() {
    let mut h = job_with_square([1.0, 1.0]);
    two_parts_in_two_toolpaths(&mut h);
    h.state_mut().open_merge_form();
    h.run();
    assert_eq!(h.state().merge_form.as_ref().unwrap().name, "Merged Toolpath 1");
    click(&mut h, "Merge");
    assert!(h.state().merge_form.is_none(), "{:?}", h.state().merge_form);
    let list = &h.state().doc.toolpaths;
    assert_eq!(list.len(), 3);
    assert!(!list[0].visible && !list[1].visible && list[2].visible, "only the merged one still shows");
    assert_eq!(list[2].op.name, "Merged Toolpath 1");
    assert!(list[2].op.merged);
    assert_eq!(h.state().selected_tp, Some(2));
    // Each part's hole, then its outline; no order ticked, so the parts stay as the toolpaths had them.
    assert_eq!(merged_depths(&h), [(2.0, -0.1), (1.0, -0.5), (7.0, -0.1), (6.0, -0.5)]);
    let moves = &h.state().doc.toolpaths[2].moves;
    assert_eq!(moves.first(), moves.last(), "out from home and back");
    assert!(moves[0].rapid);
    let cutting = |m: &[crate::toolpath::Move]| m.iter().filter(|m| !m.rapid).count();
    let sources: usize = h.state().doc.toolpaths[..2].iter().map(|t| cutting(&t.moves)).sum();
    assert_eq!(cutting(moves), sources, "every cutting move is still there");
    // Any fast move across happens at the clearance height.
    let safe = h.state().doc.machine_on_job().safe_z;
    for w in moves.windows(2) {
        let across = (w[0].p[0] - w[1].p[0]).hypot(w[0].p[1] - w[1].p[1]) > 1e-9;
        if w[1].rapid && across && w[1].p != moves[0].p {
            assert!(w[0].p[2] >= safe - 1e-9 && w[1].p[2] >= safe - 1e-9, "{w:?}");
        }
    }
}

#[test]
fn merging_without_parts_can_mix_the_cuts_and_says_so() {
    let mut h = job_with_square([1.0, 1.0]);
    two_parts_in_two_toolpaths(&mut h);
    h.state_mut().merge_choices.by_part = false;
    h.state_mut().merge_choices.left_to_right = true;
    h.state_mut().open_merge_form();
    h.run();
    h.get_by_label("Warning: cuts can change order, within a toolpath and between toolpaths.");
    click(&mut h, "Merge");
    let depths = merged_depths(&h);
    assert_eq!(depths.len(), 4);
    assert_eq!(depths[0].0, 1.0, "the left-most first, whichever toolpath it was in");
    assert!(depths[1].0 < 3.0 && depths[3].0 > 5.0, "{depths:?}");
}

#[test]
fn merging_says_why_it_cannot() {
    let mut h = job_with_square([1.0, 1.0]);
    h.state_mut().open_merge_form();
    h.run();
    click(&mut h, "Merge");
    h.get_by_label("No Toolpaths");
    two_parts_in_two_toolpaths(&mut h);
    h.state_mut().doc.toolpaths[1].visible = false;
    h.run();
    click(&mut h, "Merge");
    h.get_by_label("Two visible toolpaths at least are needed to merge.");
    h.state_mut().doc.toolpaths[1].visible = true;
    h.state_mut().doc.toolpaths[1].op.tool.dia *= 2.0;
    h.run();
    click(&mut h, "Merge");
    h.get_by_label("All the toolpaths to merge must use the same tool. The visible ones don't.");
    h.state_mut().doc.toolpaths[1].op.tool.dia /= 2.0;
    h.state_mut().doc.toolpaths[1].moves = Default::default();
    h.run();
    click(&mut h, "Merge");
    assert_eq!(h.state().doc.toolpaths.len(), 2, "nothing was added");
    assert!(h.state().merge_form.as_ref().unwrap().problem.as_ref().unwrap().contains("Outlines"));
}

#[test]
fn a_merged_toolpath_warns_of_rates_and_stays_as_it_is() {
    let mut h = job_with_square([1.0, 1.0]);
    two_parts_in_two_toolpaths(&mut h);
    h.state_mut().doc.toolpaths[1].op.tool.feed *= 2.0;
    h.state_mut().open_merge_form();
    h.run();
    let name = h.state().doc.toolpaths[0].op.tool.name.clone();
    h.get_by_label(&format!("These toolpaths have different feed rates or spindle speeds. The merged toolpath uses those of the first: {name}"));
    click(&mut h, "Merge");
    let moves = h.state().doc.toolpaths[2].moves.clone();

    // Editing is refused, and calculating everything again leaves it alone.
    h.state_mut().selected_tp = Some(2);
    h.state_mut().edit_toolpath();
    assert!(h.state().message.as_ref().unwrap().contains("merged"));
    h.state_mut().message = None;
    h.state_mut().recalculate_all();
    h.run();
    assert_eq!(*h.state().doc.toolpaths[2].moves, *moves);

    // The next one gets the next free name, and the choices are remembered.
    h.state_mut().merge_choices.grid = true;
    h.state_mut().open_merge_form();
    h.run();
    assert_eq!(h.state().merge_form.as_ref().unwrap().name, "Merged Toolpath 2");
    assert!(h.state().merge_choices.grid);
}

#[test]
fn merging_real_toolpaths_keeps_every_cut_and_makes_a_machine_file() {
    use crate::geom::{rect, Path};
    use crate::toolpath::{self, JobSize, OpKind};
    let mut h = job_with_square([1.0, 1.0]);
    let tool = toolpath::default_tools().remove(0);
    let squares = |size: f64| vec![Path::closed(rect([2.5, 2.5], size, size, 0.0)), Path::closed(rect([7.5, 2.5], size, size, 0.0))];
    let (machine, job) = {
        let d = &h.state().doc;
        (d.machine_on_job(), JobSize { width: d.job.width, height: d.job.height, thickness: d.job.thickness })
    };
    for (kind, name, depth, paths) in [(OpKind::Pocket, "Inside", 0.1, squares(1.0)), (OpKind::Profile, "Around", 0.4, squares(3.0))] {
        let op = toolpath::blank_op(kind, name.into(), tool.clone(), depth, paths);
        let moves = toolpath::generate(&op, machine, &job, None).unwrap();
        assert!(!moves.is_empty());
        h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op, visible: true, color: [0, 0, 0], moves: std::sync::Arc::new(moves), kept_moves: None, kept_steps: None, face: 0, sheet: 0 });
    }
    h.state_mut().open_merge_form();
    h.run();
    click(&mut h, "Merge");
    let list = &h.state().doc.toolpaths;
    let moves = list[2].moves.clone();
    let cutting = |m: &[toolpath::Move]| m.iter().filter(|m| !m.rapid).count();
    assert_eq!(cutting(&moves), cutting(&list[0].moves) + cutting(&list[1].moves));

    // Everything on the left is cut before anything on the right, and each part goes shallow, then deep.
    let cuts: Vec<(f64, f64)> = toolpath::cuts_of(&moves, 0).iter().map(|c| (c.moves[1].p[0], c.moves.iter().map(|m| m.p[2]).fold(f64::MAX, f64::min))).collect();
    let part = |x: f64| x > 5.0;
    assert!(cuts.windows(2).all(|w| part(w[0].0) <= part(w[1].0)), "{cuts:?}");
    for side in [false, true] {
        let deepest: Vec<f64> = cuts.iter().filter(|c| part(c.0) == side).map(|c| c.1).collect();
        assert!(deepest.first().unwrap() > deepest.last().unwrap(), "the pocket before the outline: {deepest:?}");
    }
    let chosen = [&h.state().doc.toolpaths[2]];
    let file = h.state().gcode_from(&chosen, "merged.nc");
    assert!(file.is_some_and(|f| f.lines().count() as f64 > cutting(&moves) as f64 / 2.0));
}

#[test]
fn corners_keep_proportions_unless_alt_is_held() {
    use crate::canvas::{keep_proportions, turned_degrees};
    // A box 4 wide, 2 high, from its bottom left corner: the shorter way wins.
    let got = keep_proportions([4.0, 2.0], [8.0, 2.5], [0.0, 0.0]);
    assert!((got[0] - 5.0).abs() < 1e-9 && (got[1] - 2.5).abs() < 1e-9, "{got:?}");
    let got = keep_proportions([4.0, 2.0], [3.0, 6.0], [0.0, 0.0]);
    assert!((got[0] - 3.0).abs() < 1e-9 && (got[1] - 1.5).abs() < 1e-9, "{got:?}");
    // Across the anchor it comes out mirrored, still in proportion.
    let got = keep_proportions([4.0, 2.0], [-8.0, -1.0], [0.0, 0.0]);
    assert!((got[0] + 2.0).abs() < 1e-9 && (got[1] + 1.0).abs() < 1e-9, "{got:?}");

    // Anticlockwise is positive on the sheet; Alt goes in steps of 15.
    assert!((turned_degrees([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], false) - 90.0).abs() < 1e-9);
    assert!((turned_degrees([0.0, 0.0], [1.0, 0.0], [0.0, -1.0], false) + 90.0).abs() < 1e-9);
    let at = |d: f64| [d.to_radians().cos(), d.to_radians().sin()];
    for (mouse, wanted) in [(22.0, 15.0), (23.0, 30.0), (-22.0, -15.0), (-23.0, -30.0), (5.0, 0.0)] {
        assert!((turned_degrees([0.0, 0.0], [1.0, 0.0], at(mouse), true) - wanted).abs() < 1e-9, "{mouse}");
    }
}

#[test]
fn dragging_a_turning_handle_turns_the_selection_about_its_centre() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    assert!(h.state().rotate_handles().is_empty(), "nothing until a click on the selection");
    h.state_mut().handles_on = Some(vec![id]);
    h.run();
    let handles = h.state().rotate_handles();
    assert_eq!(handles.len(), 4);
    assert_eq!(h.state().handle_rects().len(), 8, "the resizing handles are still there");
    // Beyond the corner, on the outside.
    assert!(handles[0].x < h.state().to_screen(lo).x && handles[0].y > h.state().to_screen(lo).y);

    // A quarter turn round the middle.
    let start = handles[2];
    let c = h.state().to_screen(middle);
    let turned = c + egui::vec2(start.y - c.y, c.x - start.x);
    drag(&mut h, start, turned);
    let (a, b) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((a[0] - lo[0]).abs() < 1e-3 && (b[1] - hi[1]).abs() < 1e-3, "a square turned a quarter is the same square: {a:?} {b:?}");
    let first = h.state().doc.vectors[0].paths[0].pts[0];
    h.state_mut().do_undo();
    h.run();
    let back = h.state().doc.vectors[0].paths[0].pts[0];
    assert!((first[0] - back[0]).abs() + (first[1] - back[1]).abs() > 0.1, "the points really moved, and undo takes them back");

    // The centre can be moved, and then that's what it turns about.
    h.state_mut().selected = vec![id];
    h.state_mut().handles_on = Some(vec![id]);
    h.run();
    let c = h.state().to_screen(h.state().rotation_centre().unwrap());
    let elsewhere = h.state().to_screen([lo[0] - 1.0, lo[1] - 1.0]);
    drag(&mut h, c, elsewhere);
    let moved = h.state().rotation_centre().unwrap();
    assert!((moved[0] - (lo[0] - 1.0)).abs() < 0.2 && (moved[1] - (lo[1] - 1.0)).abs() < 0.2, "{moved:?}");
    assert!(crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap().0[0] == lo[0], "moving the centre moves nothing else");
    let start = h.state().rotate_handles()[2];
    let c = h.state().to_screen(moved);
    drag(&mut h, start, c + egui::vec2(start.y - c.y, c.x - start.x));
    let (a, _) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!((a[0] - lo[0]).abs() > 0.5, "it swung round the new centre: {a:?}");
}

#[test]
fn a_piece_turns_as_one_from_the_handles() {
    let mut h = job_with_square([3.0, 3.0]);
    let first = h.state().doc.vectors[0].id;
    let second = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([7.0, 3.0], 1.0, 1.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![first, second];
    h.state_mut().make_piece();
    h.state_mut().selected = vec![first];
    h.state_mut().handles_on = Some(vec![first]);
    h.run();
    let other = |h: &Harness<'static, App>| h.state().doc.vectors.iter().find(|v| v.id == second).unwrap().paths[0].pts[0];
    let before = other(&h);
    let c = h.state().to_screen(h.state().rotation_centre().unwrap());
    let start = h.state().rotate_handles()[2];
    drag(&mut h, start, c + egui::vec2(start.y - c.y, c.x - start.x));
    assert!((other(&h)[0] - before[0]).abs() > 0.5, "the rest of the piece went round too");
}

#[cfg(test)]
fn a_layer_and_a_toolpath(h: &mut Harness<'static, App>) -> usize {
    use crate::toolpath::{self, OpKind};
    h.state_mut().doc.layers.push(crate::app::Layer::named("Outlines"));
    let on = h.state().doc.layers.len() - 1;
    let id = h.state().doc.vectors[0].id;
    h.state_mut().doc.vectors[0].layer = on;
    let paths = h.state().doc.vectors[0].paths.clone();
    let mut op = toolpath::blank_op(OpKind::Profile, "Around".into(), toolpath::default_tools().remove(0), 0.25, paths);
    op.source = vec![id];
    let doc = &h.state().doc;
    let (machine, job) = (doc.machine_on_job(), toolpath::JobSize { width: doc.job.width, height: doc.job.height, thickness: doc.job.thickness });
    let moves = toolpath::generate(&op, machine, &job, None).unwrap();
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op, visible: true, color: [0, 0, 0], moves: std::sync::Arc::new(moves), kept_moves: None, kept_steps: None, face: 0, sheet: 0 });
    h.state_mut().selected_tp = Some(0);
    on
}

#[test]
fn a_template_keeps_the_settings_and_finds_the_shapes_on_its_layer() {
    let mut h = job_with_square([3.0, 3.0]);
    a_layer_and_a_toolpath(&mut h);
    let file = h.state().template_of(true).unwrap();
    assert_eq!(file.toolpaths[0].layer.as_deref(), Some("Outlines"));
    assert!(file.toolpaths[0].op.paths.is_empty() && file.toolpaths[0].op.source.is_empty(), "no shapes go in the file");
    let text = serde_json::to_string(&file).unwrap();

    // Into a job with the layer: chosen by layer, and not calculated until asked.
    let mut other = job_with_square([6.0, 6.0]);
    other.state_mut().doc.layers.push(crate::app::Layer::named("Outlines"));
    let at = other.state().doc.layers.len() - 1;
    other.state_mut().doc.vectors[0].layer = at;
    let shape = other.state().doc.vectors[0].id;
    other.state_mut().load_template(&text).unwrap();
    other.run();
    let tp = &other.state().doc.toolpaths[0];
    assert_eq!(tp.op.name, "Around");
    assert_eq!(tp.op.source, [shape]);
    assert!(tp.moves.is_empty());
    assert!(tp.op.selector.as_ref().unwrap().layers == [at]);
    assert_eq!(other.state().selected_tp, Some(0));
    assert!(other.state().template_waiting.is_none());
}

#[test]
fn a_template_asks_about_layers_this_job_does_not_have() {
    let mut h = job_with_square([3.0, 3.0]);
    a_layer_and_a_toolpath(&mut h);
    let text = serde_json::to_string(&h.state().template_of(false).unwrap()).unwrap();
    let start = || {
        let mut other = job_with_square([6.0, 6.0]);
        other.state_mut().load_template(&text).unwrap();
        other.run();
        other
    };
    // Asked first, with nothing added.
    let mut other = start();
    assert!(other.state().doc.toolpaths.is_empty());
    other.get_by_label("Outlines");
    click(&mut other, "Create these layers");
    assert!(other.state().doc.layers.iter().any(|l| l.name == "Outlines"));
    assert_eq!(other.state().doc.toolpaths.len(), 1);
    assert!(other.state().doc.toolpaths[0].op.source.is_empty(), "nothing is on the new layer yet");

    // Shapes moved onto the layer are picked up when recalculating.
    let at = other.state().doc.layers.iter().position(|l| l.name == "Outlines").unwrap();
    other.state_mut().doc.vectors[0].layer = at;
    other.state_mut().recalculate_all();
    for _ in 0..50 {
        other.run();
        if !other.state().doc.toolpaths[0].moves.is_empty() {
            break;
        }
    }
    assert!(!other.state().doc.toolpaths[0].moves.is_empty(), "{}", other.state().status);

    // Taking away what lacks a layer can leave nothing, and says so.
    let mut other = start();
    click(&mut other, "Remove their toolpaths");
    assert!(other.state().doc.toolpaths.is_empty());
    assert!(other.state().message.as_ref().unwrap().contains("No toolpaths were left"));

    // As they are: no shapes yet, to be chosen by editing.
    let mut other = start();
    let layers = other.state().doc.layers.len();
    click(&mut other, "Load them as they are");
    assert_eq!(other.state().doc.toolpaths.len(), 1);
    assert_eq!(other.state().doc.layers.len(), layers);
    other.state_mut().recalculate_all();
    other.run();
    assert!(other.state().doc.toolpaths[0].moves.is_empty());
    assert!(other.state().status.contains("can't follow the drawing"), "{}", other.state().status);
}

#[test]
fn a_template_changes_unit_with_the_job_and_refuses_what_it_cannot_keep() {
    let mut h = job_with_square([3.0, 3.0]);
    a_layer_and_a_toolpath(&mut h);
    let depth = h.state().doc.toolpaths[0].op.cut_depth;
    let dia = h.state().doc.toolpaths[0].op.tool.dia;
    let mut file = h.state().template_of(true).unwrap();
    file.metric = !file.metric;
    let text = serde_json::to_string(&file).unwrap();
    h.state_mut().load_template(&text).unwrap();
    h.run();
    let k = if h.state().doc.job.units == crate::app::Units::Mm { 25.4 } else { 1.0 / 25.4 };
    let new = &h.state().doc.toolpaths[1].op;
    assert!((new.cut_depth - depth * k).abs() < 1e-9 && (new.tool.dia - dia).abs() < 1e-9 + dia * 26.0, "{} {}", new.cut_depth, new.tool.dia);

    let mut newer = h.state().template_of(false).unwrap();
    newer.version = 2;
    let refused = h.state_mut().load_template(&serde_json::to_string(&newer).unwrap());
    assert!(refused.unwrap_err().contains("newer"));
    assert!(h.state_mut().load_template("not a template").is_err());

    h.state_mut().doc.toolpaths[0].op.merged = true;
    h.state_mut().selected_tp = Some(0);
    assert!(h.state().template_of(true).unwrap_err().contains("merged"));
    assert_eq!(h.state().template_of(false).unwrap().toolpaths.len(), 1, "others go, the merged one is left out");
}

#[test]
fn the_job_sheet_lists_the_setup_and_each_toolpath() {
    let mut h = job_with_square([3.0, 3.0]);
    a_layer_and_a_toolpath(&mut h);
    h.state_mut().doc.toolpaths[0].op.name = "Around <the> \"edge\" & more".into();
    let page = h.state().job_sheet_html("Wheel & hub");
    for part in ["<title>Wheel &amp; hub</title>", "Job layout", "<polygon", "Material setup", "Toolpath summary", "Around &lt;the&gt; &quot;edge&quot; &amp; more", "Feed rate", "Tool number", "Clearance height", "Total time"] {
        assert!(page.contains(part), "{part}");
    }
    assert!(!page.contains("<the>"), "names can't break the page");
    assert!(!page.contains("Not calculated"));
    h.state_mut().doc.toolpaths[0].moves = Default::default();
    assert!(h.state().job_sheet_html("x").contains("Not calculated"));
}

#[test]
fn support_details_say_what_helps_and_nothing_private() {
    let mut h = job_with_square([3.0, 3.0]);
    a_layer_and_a_toolpath(&mut h);
    h.state_mut().path = Some("/somewhere/private/wheel.burin".into());
    let text = h.state().support_text();
    for part in ["Burin ", "System:", "3D quality", "Whole preview", "1 toolpaths"] {
        assert!(text.contains(part), "{part}: {text}");
    }
    assert!(!text.contains("private") && !text.contains("wheel"), "{text}");
    h.state_mut().support_open = true;
    h.run();
    click(&mut h, "Copy These Details");
    assert!(h.state().status.contains("Copied"));
}

#[cfg(test)]
fn scratch_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("burin-ui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
fn a_saved_crash(dir: &std::path::Path) {
    let report = crate::crash::Report { format: "burin-crash/1".into(), kind: "panic".into(), message: "index out of bounds".into(), location: "src/a.rs:1:2".into(), version: "1".into(), system: "test".into(), signature: "abc".into(), recent_actions: vec!["Save".into()], ..Default::default() };
    crate::crash::save_in(&dir.join("crashes"), report, 1794041400);
}

#[test]
fn after_a_crash_it_says_so_and_a_report_can_be_read_and_sent_on() {
    let dir = scratch_folder("after");
    a_saved_crash(&dir);
    let mut h = harness();
    h.state_mut().data_dir = Some(dir.clone());
    h.state_mut().after_crash_checked = false;
    h.state_mut().issues_page = Some("https://example.org/issues/new".into());
    h.run();
    h.get_by_label("Burin didn't close normally last time. A report is on this computer, and nothing has been sent.");
    click(&mut h, "See the Report...");
    assert!(h.state().crashes_open);
    assert_eq!(h.state().new_crashes, 0);
    h.run();
    assert!(crate::crash::list_in(&dir.join("crashes"))[0].seen, "told once");

    click(&mut h, "Report This...");
    assert!(!h.state().crashes_open);
    {
        let form = h.state().feedback.as_ref().unwrap();
        assert_eq!(form.title_of_draft(), "[Crash] index out of bounds");
    }
    click(&mut h, "Open a Draft in the Browser");
    let address = h.state().last_draft.clone().unwrap();
    assert!(address.starts_with("https://example.org/issues/new?title=%5BCrash%5D%20index"), "{address}");
    assert!(address.contains("src%2Fa.rs"), "the report is in it: {address}");
    assert!(h.state().feedback.is_some(), "the form stays open");

    // A second start has nothing new to say.
    let mut again = harness();
    again.state_mut().data_dir = Some(dir.clone());
    again.state_mut().after_crash_checked = false;
    again.run();
    assert!(again.query_by_label("See the Report...").is_none());

    // And it can be deleted.
    let mut h = harness();
    h.state_mut().data_dir = Some(dir.clone());
    h.state_mut().after_crash_checked = false;
    h.state_mut().crashes_open = true;
    h.run();
    click(&mut h, "Delete");
    assert!(crate::crash::list_in(&dir.join("crashes")).is_empty());
    h.get_by_label("No crash reports.");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn feedback_needs_a_title_and_shows_exactly_what_would_go() {
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().issues_page = Some("https://example.org/issues/new".into());
    h.state_mut().path = Some("/somewhere/private/wheel.burin".into());
    let ctx = h.ctx.clone();
    h.state_mut().open_feedback(&ctx, None);
    h.run();
    assert!(h.state().want_picture.is_some(), "a picture from before the form is asked for");
    click(&mut h, "Open a Draft in the Browser");
    assert!(h.state().last_draft.is_none(), "not with no title");

    {
        let form = h.state_mut().feedback.as_mut().unwrap();
        form.title = "Holes".into();
        form.text = "A hole is missing".into();
    }
    h.run();
    click(&mut h, "An idea");
    click(&mut h, "Open a Draft in the Browser");
    let address = h.state().last_draft.clone().unwrap();
    assert!(address.contains("title=%5BIdea%5D%20Holes"), "{address}");
    assert!(address.contains("A%20hole%20is%20missing") && address.contains("Details"), "{address}");
    assert!(!address.contains("private") && !address.contains("wheel"), "{address}");
    click(&mut h, "Add the technical details below");
    click(&mut h, "Open a Draft in the Browser");
    assert!(!h.state().last_draft.clone().unwrap().contains("Details"));

    // With nowhere to send it, the text can still be copied.
    h.state_mut().issues_page = None;
    h.run();
    assert!(h.query_by_label("Open a Draft in the Browser").is_none());
    h.get_by_label("This copy of Burin doesn't know where to send drafts. Copy the text instead.");
    click(&mut h, "Copy the Text");
    h.get_by_label("Copied.");
}

#[test]
fn unsaved_work_is_copied_and_can_be_brought_back() {
    let dir = scratch_folder("recovery");
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().data_dir = Some(dir.clone());
    h.state_mut().after_crash_checked = true;
    h.state_mut().recovery_every = Some(0);
    h.state_mut().path = Some("/nowhere/wheel.burin".into());
    h.state_mut().dirty = true;
    h.run();
    let found = crate::recovery::find(&dir).expect("copied");
    assert_eq!(found.project.as_deref(), Some(std::path::Path::new("/nowhere/wheel.burin")));
    assert!(found.saved_at.starts_with("20"));

    // A new start offers it, and takes it back unsaved under the same name.
    let mut next = harness();
    next.state_mut().data_dir = Some(dir.clone());
    next.state_mut().after_crash_checked = false;
    next.run();
    next.get_by_label("Get It Back");
    assert!(next.query_by_label("See the Report...").is_none());
    click(&mut next, "Get It Back");
    assert_eq!(next.state().doc.vectors.len(), 1);
    assert_eq!(next.state().path.as_deref(), Some(std::path::Path::new("/nowhere/wheel.burin")));
    assert!(next.state().dirty);

    // Saving makes the copy pointless.
    let saved = dir.join("wheel.burin");
    next.state_mut().save_to(saved).unwrap();
    next.run();
    assert!(crate::recovery::find(&dir).is_none());

    // Put off, it stays for next time and isn't written over.
    crate::recovery::write(&dir, &next.state().doc, None).unwrap();
    let before = std::fs::read(dir.join("recovery.burin")).unwrap();
    let mut later = harness();
    later.state_mut().data_dir = Some(dir.clone());
    later.state_mut().after_crash_checked = false;
    later.run();
    later.get_by_label("Delete It");
    later.state_mut().recovery_every = Some(0);
    later.state_mut().recovery_left = true;
    later.state_mut().recovery = None;
    later.state_mut().load_sample();
    later.state_mut().dirty = true;
    later.run();
    assert_eq!(std::fs::read(dir.join("recovery.burin")).unwrap(), before);

    // Refused, it's gone.
    let mut last = harness();
    last.state_mut().data_dir = Some(dir.clone());
    last.state_mut().after_crash_checked = false;
    last.run();
    click(&mut last, "Delete It");
    assert!(crate::recovery::find(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
#[derive(Debug)]
struct DroppedForTest(std::path::PathBuf);

#[cfg(test)]
impl egui::DroppedFile for DroppedForTest {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn a_project_dropped_on_the_window_opens() {
    let dir = scratch_folder("dropped");
    let mut h = job_with_square([3.0, 3.0]);
    let saved = dir.join("wheel.burin");
    h.state_mut().save_to(saved.clone()).unwrap();

    let mut other = harness();
    other.input_mut().dropped_files.push(std::sync::Arc::new(DroppedForTest(saved.clone())));
    other.run();
    assert_eq!(other.state().doc.vectors.len(), 1);
    assert_eq!(other.state().path.as_ref(), Some(&saved));

    let mut wrong = harness();
    wrong.input_mut().dropped_files.push(std::sync::Arc::new(DroppedForTest(dir.join("picture.png"))));
    wrong.run();
    assert!(wrong.state().message.as_ref().unwrap().contains("picture.png"));

    // Work not saved is asked about first.
    let mut busy = job_with_square([3.0, 3.0]);
    busy.state_mut().dirty = true;
    busy.input_mut().dropped_files.push(std::sync::Arc::new(DroppedForTest(saved.clone())));
    busy.run();
    assert!(busy.state().leave.is_some());
    assert!(busy.state().path.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn notes_stay_with_the_job_and_go_on_its_sheet() {
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().doc.notes = "Screw down <well> first".into();
    let text = serde_json::to_string(&h.state().doc).unwrap();
    assert!(serde_json::from_str::<crate::app::Doc>(&text).unwrap().notes.contains("well"));
    assert!(h.state().job_sheet_html("x").contains("Screw down &lt;well&gt; first"));
    h.state_mut().doc.notes.clear();
    assert!(!h.state().job_sheet_html("x").contains("<h2>Notes</h2>"));
    h.state_mut().notes_open = true;
    h.run();
    h.get_by_label("Saved with the job, printed on its job sheet, and written where a post processor asks for them. Start with a dot to see them each time the job opens.");
}

#[test]
fn notes_starting_with_a_dot_open_with_the_job() {
    let dir = scratch_folder("dot");
    let mut h = job_with_square([3.0, 3.0]);
    let file = dir.join("job.burin");
    h.state_mut().doc.notes = "Plain notes".into();
    h.state_mut().save_to(file.clone()).unwrap();
    let mut other = harness();
    other.state_mut().open_path(file.clone()).unwrap();
    assert!(!other.state().notes_open);
    h.state_mut().doc.notes = ".Read me first".into();
    h.state_mut().save_to(file.clone()).unwrap();
    other.state_mut().open_path(file).unwrap();
    assert!(other.state().notes_open);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saving_keeps_the_earlier_version_and_it_can_be_opened_again() {
    let dir = scratch_folder("versions");
    let file = dir.join("wheel.burin");
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().save_to(file.clone()).unwrap();
    assert!(crate::backups::list(&file).is_empty(), "nothing before the first save");
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([6.0, 6.0], 1.0))], Prim::Free);
    h.state_mut().save_to(file.clone()).unwrap();
    let versions = crate::backups::list(&file);
    assert_eq!(versions.len(), 1);
    assert!(!std::fs::read_to_string(&versions[0].file).unwrap().contains(&format!("\"id\":{id},")), "the version before the circle");

    // From the window: the old one comes back unsaved, under the project's own name.
    h.state_mut().dirty = false;
    h.state_mut().backups_open = true;
    h.run();
    click(&mut h, "Open This Version");
    assert_eq!(h.state().doc.vectors.len(), 1);
    assert!(h.state().dirty);
    assert_eq!(h.state().path.as_ref(), Some(&file));
    assert!(!h.state().backups_open);

    // Saving it keeps the one with the circle, so nothing is lost either way.
    h.state_mut().save_to(file.clone()).unwrap();
    let versions = crate::backups::list(&file);
    assert_eq!(versions.len(), 2);
    let texts: Vec<String> = versions.iter().map(|v| std::fs::read_to_string(&v.file).unwrap()).collect();
    assert!(texts.iter().any(|t| t.contains(&format!("\"id\":{id},"))));

    // Unsaved changes aren't thrown away by opening a version.
    h.state_mut().dirty = true;
    h.state_mut().backups_open = true;
    h.run();
    let shapes = h.state().doc.vectors.len();
    h.get_all_by_label("Open This Version").next().unwrap().click();
    h.run();
    assert_eq!(h.state().doc.vectors.len(), shapes);
    assert!(h.state().message.as_ref().unwrap().contains("first"));

    // Turned off, nothing more is kept.
    h.state_mut().message = None;
    h.state_mut().backup_settings.on = false;
    h.state_mut().doc.vectors.clear();
    h.state_mut().save_to(file.clone()).unwrap();
    assert_eq!(crate::backups::list(&file).len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_backups_window_says_what_to_do_without_a_file() {
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().backups_open = true;
    h.run();
    h.get_by_label("Save the project first: its backups are kept beside its file.");
    h.get_by_label("Keep the earlier version each time a project is saved");
}

#[test]
fn saving_stops_on_a_danger_until_told_to_go_on() {
    use crate::toolpath::Move;
    let mut h = job_with_square([3.0, 3.0]);
    let mut op = crate::toolpath::blank_op(crate::toolpath::OpKind::Profile, "Around".into(), crate::toolpath::default_tools().remove(0), 0.25, Vec::new());
    op.tool.feed = 0.0;
    let moves = vec![Move { p: [0.0, 0.0, 0.8], rapid: true, join: false }, Move { p: [1.0, 1.0, -0.1], rapid: false, join: false }, Move { p: [2.0, 1.0, -0.1], rapid: false, join: false }, Move { p: [0.0, 0.0, 0.8], rapid: true, join: false }];
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op, visible: true, color: [0, 0, 0], moves: std::sync::Arc::new(moves), kept_moves: None, kept_steps: None, face: 0, sheet: 0 });
    h.state_mut().save_gcode_dialog();
    assert!(h.state().checks_open);
    assert_eq!(h.state().checks_for_save, Some(None));
    h.run();
    h.get_by_label("Danger");
    h.get_by_label("Save Anyway");
    // Opened from the menu, it's only a look: nothing to save.
    h.state_mut().checks_for_save = None;
    h.run();
    assert!(h.query_by_label("Save Anyway").is_none());
}

#[test]
fn each_kind_of_move_is_told_apart_for_drawing() {
    use crate::toolpath::{move_kind, Move, MoveKind};
    let at = [1.0, 1.0, 5.0];
    assert_eq!(move_kind(at, &Move { p: [1.0, 1.0, -2.0], rapid: false, join: false }), MoveKind::Plunge);
    assert_eq!(move_kind([1.0, 1.0, -2.0], &Move { p: [1.0, 1.0, 5.0], rapid: true, join: false }), MoveKind::Retract);
    assert_eq!(move_kind(at, &Move { p: [4.0, 1.0, 5.0], rapid: true, join: false }), MoveKind::Rapid);
    assert_eq!(move_kind(at, &Move { p: [1.0, 1.0, 2.0], rapid: true, join: false }), MoveKind::Rapid, "fast and down is a fast move");
    assert_eq!(move_kind(at, &Move { p: [4.0, 1.0, -2.0], rapid: false, join: false }), MoveKind::Cut, "a ramp cuts");
}

#[test]
fn another_programs_machine_file_comes_in_as_a_toolpath_that_stays_as_it_is() {
    let mut h = job_with_square([3.0, 3.0]);
    h.state_mut().doc.set_units(crate::app::Units::Mm);
    h.state_mut().import_toolpath("other.nc", "G20\nG0 X1 Y1 Z0.2\nG1 Z-0.1\nG1 X2\nG0 Z0.2\n").unwrap();
    h.run();
    let tp = h.state().doc.toolpaths.last().unwrap().clone();
    assert_eq!(tp.op.name, "other.nc");
    assert!(tp.op.imported);
    let cut: Vec<[f64; 3]> = tp.moves.iter().filter(|m| !m.rapid).map(|m| m.p).collect();
    let zero = h.state().doc.job.origin();
    assert!((cut[1][0] - (50.8 + zero[0])).abs() < 1e-9, "inches turned to millimetres: {cut:?}");
    assert!(h.state().status.contains("Check its tool"));
    h.state_mut().selected_tp = Some(h.state().doc.toolpaths.len() - 1);
    h.state_mut().edit_toolpath();
    assert!(h.state().message.as_ref().unwrap().contains("another program"));
    h.state_mut().message = None;
    h.state_mut().recalculate_all();
    h.run();
    assert_eq!(*h.state().doc.toolpaths.last().unwrap().moves, *tp.moves, "recalculating leaves it alone");
    assert!(h.state_mut().import_toolpath("x.nc", "G0 X1\nM30").is_err());
}

#[test]
fn a_value_typed_while_dragging_makes_the_move_exact() {
    use crate::canvas::Drag;
    let mut h = job_with_square([3.0, 3.0]);
    let id = h.state().doc.vectors[0].id;
    let first = |h: &Harness<'static, App>| h.state().doc.vectors[0].paths[0].pts[0];
    let before = first(&h);
    h.state_mut().selected = vec![id];
    // Dragged 1 to the right so far; typing 3,4 makes it exactly that.
    h.state_mut().translate_selected(1.0, 0.0);
    h.state_mut().drag = Drag::Move { last: [6.0, 5.0], start: [5.0, 5.0] };
    h.run();
    h.input_mut().events.push(egui::Event::Text("3,4".into()));
    h.run();
    assert_eq!(h.state().typed, "3,4");
    h.key_press(egui::Key::Enter);
    h.run();
    let now = first(&h);
    assert!((now[0] - before[0] - 3.0).abs() < 1e-9 && (now[1] - before[1] - 4.0).abs() < 1e-9, "{before:?} {now:?}");
    assert!(matches!(h.state().drag, Drag::None));

    // One number: that distance along the way it was going.
    let before = first(&h);
    h.state_mut().translate_selected(0.0, 1.0);
    h.state_mut().drag = Drag::Move { last: [5.0, 6.0], start: [5.0, 5.0] };
    h.input_mut().events.push(egui::Event::Text("2.5".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let now = first(&h);
    assert!((now[0] - before[0]).abs() < 1e-9 && (now[1] - before[1] - 2.5).abs() < 1e-9);

    // A turn: exactly the angle typed, from where it started. Keys like 9 don't turn it as well.
    let orig = vec![(id, h.state().doc.vectors[0].paths.clone())];
    let (lo, hi) = crate::geom::bbox(&orig[0].1).unwrap();
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    h.state_mut().drag = Drag::Rotate { centre, start: [hi[0], centre[1]], orig: orig.clone() };
    h.input_mut().events.push(egui::Event::Text("90".into()));
    h.key_press(egui::Key::Num9);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let p0 = orig[0].1[0].pts[0];
    let p1 = h.state().doc.vectors[0].paths[0].pts[0];
    let expect = [centre[0] - (p0[1] - centre[1]), centre[1] + (p0[0] - centre[0])];
    assert!((p1[0] - expect[0]).abs() < 1e-9 && (p1[1] - expect[1]).abs() < 1e-9, "{p1:?} {expect:?}");
    assert!(crate::canvas::typed_numbers("1,2,3").is_none() && crate::canvas::typed_numbers("-1.5").is_some());
}

#[test]
fn a_value_typed_while_dragging_a_point_places_it() {
    use crate::canvas::Drag;
    let mut h = job_with_square([3.0, 3.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().tool = crate::app::Tool2::Nodes;
    h.run();
    let point = |h: &Harness<'static, App>| h.state().doc.vectors[0].paths[0].pts[0];
    let start = point(&h);
    // Dragged a little up so far: 0.75 puts it exactly that far up from where it was.
    h.state_mut().doc.vectors[0].paths[0].pts[0] = [start[0], start[1] + 0.2];
    h.state_mut().drag = Drag::Node { id, path: 0, index: 0, start };
    h.input_mut().events.push(egui::Event::Text("0.75".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let now = point(&h);
    assert!((now[0] - start[0]).abs() < 1e-9 && (now[1] - start[1] - 0.75).abs() < 1e-9, "{start:?} {now:?}");
    assert!(matches!(h.state().drag, Drag::None));
    // Two values: by that much each way from where it was.
    let start = point(&h);
    h.state_mut().drag = Drag::Node { id, path: 0, index: 0, start };
    h.input_mut().events.push(egui::Event::Text("-1,0.5".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    let now = point(&h);
    assert!((now[0] - start[0] + 1.0).abs() < 1e-9 && (now[1] - start[1] - 0.5).abs() < 1e-9, "{start:?} {now:?}");
}

#[test]
fn a_file_can_start_again_from_a_cut() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let profile = h.state().doc.toolpaths.iter().position(|t| crate::toolpath::cuts_of(&t.moves, 0).len() >= 2).unwrap();
    let cuts = crate::toolpath::cuts_of(&h.state().doc.toolpaths[profile].moves, 0);
    assert!(cuts.len() >= 2, "{}", cuts.len());
    let whole = h.state().gcode_text(None).unwrap();
    h.state_mut().resume = Some((profile, 1));
    let from = h.state().gcode_text(None).unwrap();
    h.state_mut().resume = None;
    assert!(from.lines().count() < whole.lines().count(), "a shorter file once started later");
    // It still comes from the clearance height down to where the second cut begins.
    let entry = cuts[1].moves[0].p;
    let moves = crate::toolpath::moves_from_cut(&h.state().doc.toolpaths[profile].moves, 1, h.state().doc.machine_on_job(), h.state().doc.toolpaths[profile].op.tool.metric);
    let safe = h.state().doc.machine_on_job().safe_z;
    assert!(moves.iter().any(|m| m.rapid && m.p == [entry[0], entry[1], safe]));
    assert_eq!(moves.iter().filter(|m| !m.rapid).count(), cuts[1..].iter().map(|c| c.moves.len() - 1).sum::<usize>());
    h.state_mut().resume_open = true;
    h.run();
    h.get_by_label("Save From Here...");
}

#[test]
fn help_contents_opens_its_pages_and_the_light_settings_can_be_switched() {
    let mut h = job_with_square([1.0, 1.0]);
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Help").next().unwrap().click();
    h.run();
    h.get_by_label("Help Contents").click();
    h.run();
    assert!(h.state().help_open);
    h.get_by_label("Saving for the Machine").click();
    h.run();
    assert_eq!(h.state().help_page, 5);
    assert!(h.query_by_label_contains("Start Again From a Cut writes a file").is_some(), "the page's text shows");

    assert!(!h.state().shadow_shading && !h.state().light_follows_viewer, "both off at the start, as in the original");
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "View").next().unwrap().click();
    h.run();
    h.get_by_label("Use Shadow Shading").click();
    h.run();
    if h.query_by_label("Light Follows Viewer").is_none() {
        h.get_all_by_role_and_label(egui::accesskit::Role::Button, "View").next().unwrap().click();
        h.run();
    }
    h.get_by_label("Light Follows Viewer").click();
    h.run();
    assert!(h.state().shadow_shading && h.state().light_follows_viewer);
}

#[test]
fn a_file_picked_in_the_web_version_opens_by_its_kind() {
    let dir = std::env::temp_dir().join(format!("burin-picked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = job_with_square([3.0, 3.0]);
    let before = h.state().doc.toolpaths.len();
    let machine = dir.join("other.nc");
    std::fs::write(&machine, "G21\nG0 X1 Y1 Z2\nG1 Z-1\nG1 X5\nG0 Z2\n").unwrap();
    h.state_mut().open_picked(machine);
    h.run();
    assert_eq!(h.state().doc.toolpaths.len(), before + 1, "a machine file becomes a toolpath");
    assert!(h.state().doc.toolpaths.last().unwrap().op.imported);

    let shapes = h.state().doc.vectors.len();
    let drawing = dir.join("shape.svg");
    std::fs::write(&drawing, r#"<svg xmlns="http://www.w3.org/2000/svg" width="10mm" height="10mm" viewBox="0 0 10 10"><rect x="1" y="1" width="4" height="4"/></svg>"#).unwrap();
    h.state_mut().open_picked(drawing);
    h.run();
    assert!(h.state().doc.vectors.len() > shapes, "a drawing adds its shapes");

    h.state_mut().open_picked(dir.join("notes.odt"));
    assert!(h.state().message.as_ref().is_some_and(|m| m.contains("notes.odt")), "anything else is explained");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!crate::platform::WEB);
    assert!(crate::platform::threads(true) >= 1);
}

#[test]
fn print_setup_lays_the_drawing_on_the_paper_chosen() {
    let mut h = job_with_square([1.0, 1.0]);
    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "File").next().unwrap().click();
    h.run();
    h.get_by_label("Print Setup...").click();
    h.run();
    assert!(h.state().print_setup_open);
    assert!(h.query_by_label_contains("One page the size of the job").is_some());
    h.state_mut().print_setup.paper = crate::printing::Paper::A4;
    h.run();
    // The job is 10 x 10 inches: 254 mm, over 190 x 277 mm of A4 inside the margins.
    assert!(h.query_by_label_contains("2 pages, 2 across and 1 up").is_some());
    h.get_by_label("Shrink to one page").click();
    h.run();
    assert!(h.state().print_setup.fit);
    assert!(h.query_by_label_contains("One page, at 74.8% of the real size.").is_some());
}

#[test]
fn a_depth_typed_as_a_sum_follows_the_document_variables() {
    let mut h = job_with_square([4.0, 4.0]);
    h.state_mut().doc.variables = vec![crate::formula::Variable { name: "depth".into(), value: 0.4 }];
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.cut_depth = 0.371;
    }
    h.run();
    h.get_all_by_value("0.371").next().unwrap().focus();
    h.run();
    for _ in 0..6 {
        h.key_press(egui::Key::Backspace);
    }
    h.run();
    h.get_all_by_value("").find(|n| n.is_focused()).unwrap().type_text("{depth}/2");
    h.run();
    h.run();
    let RightMode::Op { op, .. } = &h.state().right else { panic!("the toolpath form is open") };
    assert!((op.cut_depth - 0.2).abs() < 1e-9, "worked out: {}", op.cut_depth);
    assert_eq!(op.formulas.cut_depth, "{depth}/2", "the sum is kept");
    h.state_mut().calculate_op();
    h.run();
    let behind = h.state_mut().set_variables(vec![crate::formula::Variable { name: "depth".into(), value: 0.6 }]).unwrap();
    assert_eq!(behind.len(), 1, "the toolpath using it needs recalculating");
    assert!((h.state().doc.toolpaths[0].op.cut_depth - 0.3).abs() < 1e-9);
    let broken = h.state_mut().set_variables(Vec::new());
    assert!(broken.is_err(), "a sum would lose its variable");
    assert_eq!(h.state().doc.variables.len(), 1, "nothing changed then");
}

#[test]
fn a_textures_maximums_and_stepover_can_be_sums() {
    let mut h = job_with_square([4.0, 4.0]);
    h.state_mut().doc.variables = vec![crate::formula::Variable { name: "grain".into(), value: 0.3 }];
    h.state_mut().start_op(crate::toolpath::OpKind::Texture);
    if let RightMode::Op { op, .. } = &mut h.state_mut().right {
        op.tex_length[1] = 0.417;
    }
    h.run();
    h.get_all_by_value("0.417").next().unwrap().focus();
    h.run();
    for _ in 0..6 {
        h.key_press(egui::Key::Backspace);
    }
    h.run();
    h.get_all_by_value("").find(|n| n.is_focused()).unwrap().type_text("{grain}*2");
    h.run();
    h.run();
    let RightMode::Op { op, .. } = &h.state().right else { panic!("the toolpath form is open") };
    assert!((op.tex_length[1] - 0.6).abs() < 1e-9, "worked out: {}", op.tex_length[1]);
    assert_eq!(op.formulas.texture_length, "{grain}*2");
    let mut op = op.clone();
    op.formulas.texture_depth = "{grain}/3".into();
    op.formulas.stepover = "{grain}/2".into();
    let context = h.state().formula_context();
    assert!(op.formulas.apply(&mut op.clone(), &context).unwrap());
    op.formulas.clone().apply(&mut op, &context).unwrap();
    assert!((op.tex_cut[1] - 0.1).abs() < 1e-9 && (op.stepover - 0.15).abs() < 1e-9);
    assert!(op.formulas.uses("grain"));
}

#[test]
fn single_line_text_is_engraved_along_its_lines() {
    let mut h = job_with_square([8.0, 8.0]);
    let style = crate::fonts::TextStyle { font: "Single Line Sans".into(), ..Default::default() };
    let prim = Prim::Text { at: [1.0, 1.0], height: 1.0, text: "HI".into(), style };
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    assert!(h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().paths.iter().all(|p| !p.closed));
    h.state_mut().selected = vec![id];
    h.state_mut().start_op(crate::toolpath::OpKind::Engrave);
    h.state_mut().calculate_op();
    h.run();
    let tp = h.state().doc.toolpaths.last().expect("a toolpath");
    let cuts = tp.moves.iter().filter(|m| !m.rapid).count();
    assert!(cuts > 6, "the tool follows the strokes: {cuts} cuts");
    // Saved and read back, the text keeps its font.
    let text = serde_json::to_string(&h.state().doc).unwrap();
    assert!(text.contains("Single Line Sans"));
    let back: crate::app::Doc = serde_json::from_str(&text).unwrap();
    assert!(back.vectors.iter().any(|v| matches!(&v.prim, Prim::Text { style, .. } if style.font == "Single Line Sans")));
}

#[test]
fn a_text_is_fitted_into_the_selected_shape() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().start_draw(crate::app::DrawKind::Text);
    h.run();
    assert!(h.query_by_label("Fit to Selected Shape").is_some());
    h.get_by_label("Fit to Selected Shape").click();
    h.run();
    let LeftForm::Draw(f) = &h.state().left else { panic!("the text form is open") };
    let paths = crate::fonts::text_paths(&f.text, f.text_height, f.c, &f.text_style);
    let (lo, hi) = crate::geom::bbox(paths.iter()).unwrap();
    // The square is 2 wide round 5,5: the text fills it across and sits in its middle.
    assert!((hi[0] - lo[0] - 2.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(((lo[1] + hi[1]) / 2.0 - 5.0).abs() < 1e-6);
}

#[test]
fn the_model_tools_bake_outline_trim_and_scale_the_shapes() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    let comps = app.doc.components.len();
    // A copy of what shows, the originals left as they are.
    app.component_from_visible_model();
    assert_eq!(app.doc.components.len(), comps + 1);
    assert!(app.doc.components.last().unwrap().surface.is_some());
    app.doc.components.last_mut().unwrap().visible = false;
    // The outline of the round shape, as a vector.
    app.selected_comp = Some(1);
    let vectors = app.doc.vectors.len();
    app.boundary_around_component();
    assert_eq!(app.doc.vectors.len(), vectors + 1);
    let outline = app.doc.vectors.last().unwrap().paths.clone();
    let (lo, hi) = crate::geom::bbox(outline.iter()).unwrap();
    assert!((hi[0] - lo[0] - 4.4).abs() < 0.3, "about the disc's 4.4 across: {lo:?} {hi:?}");
    // Emptied outside a small square in its middle.
    let square = app.doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 6.0], 1.0, 1.0, 0.0))], Prim::Free);
    app.selected = vec![square];
    app.selected_comp = Some(1);
    app.reset_component(false);
    let grid = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    assert!(grid.sample(5.0, 6.0) > 0.0, "kept inside");
    assert_eq!(grid.sample(4.0, 6.0), 0.0, "gone outside");
    // The whole model scaled to a new top.
    app.model_dirty = true;
    let top = app.model_top().unwrap();
    assert!(app.scale_model_height(top * 2.0));
    app.model_dirty = true;
    assert!((app.model_top().unwrap() - top * 2.0).abs() < 1e-3);
}

#[test]
fn a_component_is_smoothed_and_baked() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    // The flat plaque: its sharp edge softens.
    app.selected_comp = Some(0);
    let before = crate::relief::component_height(&app.doc.components[0], 10.0, 10.0, 40_000);
    assert!(app.smooth_component(crate::modeltools::SmoothForm { blend: 1.0, keep_empty: true }));
    let after = crate::relief::component_height(&app.doc.components[0], 10.0, 10.0, 40_000);
    let near_edge = [0.62, 5.0];
    assert!(after.sample(near_edge[0], near_edge[1]) < before.sample(near_edge[0], near_edge[1]), "the rim comes down");
    assert!((after.sample(5.0, 5.0) - before.sample(5.0, 5.0)).abs() < 1e-3, "the middle stays");
    // Baking keeps the round shape as it is, as heights of its own.
    app.selected_comp = Some(1);
    let round = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    app.bake_component();
    assert!(app.doc.components[1].surface.is_some());
    let baked = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    assert!((baked.sample(5.0, 6.0) - round.sample(5.0, 6.0)).abs() < 1e-3);
    // With nothing selected, it says so.
    app.selected_comp = None;
    assert!(!app.smooth_component(Default::default()));
    assert!(app.message.is_some());
}

#[test]
fn a_component_bends_inside_an_envelope() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    app.selected.clear();
    app.selected_comp = Some(1);
    app.start_xform(crate::app::Xform::Distort);
    app.selected_comp = Some(1);
    assert!(app.envelope_check(crate::distort::EnvelopeKind::BoundingBox).is_ok());
    let before = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    assert!(app.apply_envelope(crate::distort::EnvelopeKind::BoundingBox));
    let same = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    assert!((same.sample(5.0, 6.0) - before.sample(5.0, 6.0)).abs() < 0.02, "a new envelope leaves it where it was");
    // Pulling the top right corner out to the right: the disc reaches further that way.
    let corner = app.envelope.as_ref().unwrap().corners[2];
    let far = [corner[0] + 2.0, corner[1]];
    assert_eq!(before.sample(far[0] - 0.6, corner[1] - 0.3), 0.0);
    app.move_envelope_corner(2, far);
    let bent = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    let (lo, hi) = crate::geom::bbox(&app.doc.components[1].paths).unwrap();
    let (blo, bhi) = crate::geom::bbox(&crate::modeltools::outline_of(&before)).unwrap();
    assert!(hi[0] > bhi[0] + 0.8, "wider at the top right: {hi:?} vs {bhi:?}");
    assert!((lo[0] - blo[0]).abs() < 0.1, "the left stays");
    assert!(bent.data.iter().cloned().fold(0.0f32, f32::max) > 0.0);
    app.bake_envelope();
    app.do_undo();
    let back = crate::relief::component_height(&app.doc.components[1], 10.0, 10.0, 40_000);
    assert!((back.sample(5.0, 6.0) - before.sample(5.0, 6.0)).abs() < 0.02, "undone in one step");
}

#[test]
fn a_vector_is_spun_into_a_component() {
    let mut h = job_with_square([5.0, 5.0]);
    let app = h.state_mut();
    let arc = app.doc.add_vector(vec![crate::geom::Path { pts: vec![[2.0, 2.0], [3.0, 2.4], [4.0, 2.0]], closed: false }], Prim::Free);
    app.selected = vec![arc];
    app.open_turn_spin();
    let form = crate::modeltools::TurnForm { turn: false, scale_to_height: true, height: 0.3, ..app.turn_form.unwrap() };
    let comps = app.doc.components.len();
    assert!(app.turn_spin(form));
    assert_eq!(app.doc.components.len(), comps + 1);
    let c = app.doc.components.last().unwrap();
    assert!((c.height - 0.3).abs() < 1e-3, "scaled to the height asked: {}", c.height);
    app.selected.clear();
    assert!(!app.turn_spin(form));
    assert!(app.message.is_some());
}

#[test]
fn draft_is_added_as_a_new_component() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    app.open_draft_model();
    assert_eq!(app.draft_angle, Some(crate::modeltools::DRAFT_ANGLE));
    let comps = app.doc.components.len();
    assert!(app.draft_model(20.0));
    assert_eq!(app.doc.components.len(), comps + 1);
    assert_eq!(app.doc.components.last().unwrap().name, "Model With 20.0° Draft");
    assert_eq!(app.last_draft_angle, Some(20.0));
    app.do_undo();
    assert_eq!(app.doc.components.len(), comps);
    app.doc.components.clear();
    app.model_dirty = true;
    app.draft_angle = None;
    app.open_draft_model();
    assert!(app.draft_angle.is_none() && app.message.is_some());
}

#[test]
fn a_component_is_made_from_the_carve_preview() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    app.doc.components.clear();
    app.model_dirty = true;
    assert!(!app.component_from_preview(), "no preview yet");
    assert!(app.message.take().is_some());
    let j = app.doc.job.clone();
    let mut sim = crate::relief::Sim::new(j.width, j.height, j.thickness, 40_000);
    let g = &mut sim.grid;
    for iy in 0..g.ny {
        for ix in 0..g.nx {
            let (x, y) = (ix as f64 * g.cell, iy as f64 * g.cell);
            if (2.0..=4.0).contains(&x) && (2.0..=4.0).contains(&y) {
                g.data[iy * g.nx + ix] = -0.3;
            }
        }
    }
    app.sim = Some(sim);
    // The model's zero at the material's top: everything carved is below it.
    app.doc.machine.gap_below = None;
    app.doc.machine.gap_above = 0.0;
    assert!(!app.component_from_preview());
    assert!(app.message.take().is_some());
    // Its zero 0.1 above the bottom: the pocket floor stands 0.1 above it, the uncut top is left out.
    app.doc.machine.gap_below = Some(0.1);
    assert!(app.component_from_preview());
    let c = app.doc.components.last().unwrap();
    assert_eq!(c.name, "Toolpath Preview");
    let grid = crate::relief::component_height(c, j.width, j.height, 40_000);
    assert!((grid.sample(3.0, 3.0) - 0.1).abs() < 1e-3, "{}", grid.sample(3.0, 3.0));
    assert_eq!(grid.sample(7.0, 7.0), 0.0, "uncut, with no model: nothing");
}

#[test]
fn the_model_is_sliced_into_hidden_components_and_boundaries() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    let top = app.model_top().unwrap();
    // As thick as the model: nothing to slice.
    app.doc.job.thickness = top + 0.1;
    app.open_slice_model();
    assert!(app.slice_form.is_none() && app.message.take().is_some());
    app.doc.job.thickness = top / 2.5;
    app.open_slice_model();
    let form = app.slice_form.expect("the form opens");
    assert!(form.count >= 3 && !form.by_count);
    let (comps, layers) = (app.doc.components.len(), app.doc.layers.len());
    assert!(app.slice_model(form));
    let slices: Vec<_> = app.doc.components[comps..].iter().collect();
    assert_eq!(slices.len(), form.count, "one per slice");
    assert!(slices.iter().all(|c| !c.visible) && slices[0].name == "Slice 1");
    assert!(slices.iter().all(|c| c.height <= top / 2.5 + 1e-3), "none thicker than the material");
    assert!(app.doc.layers.len() > layers && app.doc.layers.iter().any(|l| l.name == "Slice 1"));
    app.do_undo();
    assert_eq!(app.doc.components.len(), comps);
    // By number: three slices of a third each.
    let three = crate::modeltools::SliceForm { by_count: true, count: 3, boundaries: false, ..form };
    assert!(app.slice_model(three));
    assert_eq!(app.doc.components.len(), comps + 3);
}

#[test]
fn an_offset_model_is_added_as_a_new_component() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    let comps = app.doc.components.len();
    app.open_offset_model();
    assert_eq!(app.offset_form.map(|f| f.distance), Some(crate::modeltools::OFFSET_DISTANCE_INCH), "inches at first");
    assert!(!app.offset_model(2.0), "more than 10% of 10 inches");
    assert!(app.message.take().unwrap().contains("10%"));
    let top = app.model_top().unwrap();
    assert!(app.offset_model(0.125));
    assert_eq!(app.doc.components.len(), comps + 1);
    let c = app.doc.components.last().unwrap();
    assert_eq!(c.name, "Model With 0.125 Offset");
    assert!((c.height - (top + 0.125)).abs() < 0.01, "{} {}", c.height, top);
    app.model_dirty = true;
    assert!((app.model_top().unwrap() - (top + 0.125)).abs() < 0.01, "it shows above the rest");
    app.do_undo();
    assert_eq!(app.doc.components.len(), comps);
    app.doc.components.clear();
    app.model_dirty = true;
    app.offset_form = None;
    app.open_offset_model();
    assert!(app.offset_form.is_none() && app.message.is_some(), "nothing to offset");
}

#[test]
fn a_component_is_split_in_two_by_a_vector() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.run();
    let app = h.state_mut();
    let comps = app.doc.components.len();
    let height = |app: &crate::app::App, i: usize, at: [f64; 2]| crate::relief::component_height(&app.doc.components[i], 10.0, 10.0, 40_000).sample(at[0], at[1]);
    let (left, right) = ([4.0, 6.0], [6.0, 6.0]);
    assert!(height(app, 1, left) > 0.0 && height(app, 1, right) > 0.0);
    // Without exactly one vector, it says how to choose.
    app.selected_comp = Some(1);
    app.selected = Vec::new();
    app.split_component();
    assert!(app.message.take().is_some());
    assert_eq!(app.doc.components.len(), comps);
    // A line that misses it makes nothing.
    let aside = app.doc.add_vector(vec![crate::geom::Path { pts: vec![[0.5, 0.5], [0.5, 9.5]], closed: false }], Prim::Free);
    app.selected = vec![aside];
    app.split_component();
    assert!(app.message.take().is_some(), "not two areas");
    // An open line straight across the disc: a left part and a right one.
    let line = app.doc.add_vector(vec![crate::geom::Path { pts: vec![[5.0, 2.0], [5.0, 9.9]], closed: false }], Prim::Free);
    app.selected = vec![line];
    let before = height(app, 1, left);
    app.split_component();
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_eq!(app.doc.components.len(), comps + 1);
    let (a, b) = (height(app, 1, left), height(app, 1, right));
    let (c, d) = (height(app, 2, left), height(app, 2, right));
    assert!((a > 0.0) != (b > 0.0) && (c > 0.0) != (d > 0.0) && (a > 0.0) != (c > 0.0), "{a} {b} {c} {d}");
    assert!((a.max(c) - before).abs() < 1e-3, "the heights stay");
    assert!(app.doc.components[2].name.ends_with("(2)"));
    // A closed square: what's inside, then the rest.
    let part = if a > 0.0 { 1 } else { 2 };
    let square = app.doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([4.0, 6.0], 0.8, 0.8, 0.0))], Prim::Free);
    app.selected = vec![square];
    app.selected_comp = Some(part);
    app.split_component();
    assert_eq!(app.doc.components.len(), comps + 2);
    assert!(height(app, part, [4.0, 6.0]) > 0.0 && height(app, part, [3.2, 6.0]) == 0.0, "inside first");
    assert!(height(app, part + 1, [3.2, 6.0]) > 0.0 && height(app, part + 1, [4.0, 6.0]) == 0.0, "then outside");
    // Undone in one step.
    app.do_undo();
    assert_eq!(app.doc.components.len(), comps + 1);
}

/// For looking into how far raster rows sit from the original's: BURIN_JOBS=<folder> cargo test raster_rows_against -- --nocapture
#[test]
fn raster_rows_against_saved_jobs() {
    let Ok(dir) = std::env::var("BURIN_JOBS") else { return };
    let mut files = Vec::new();
    let mut todo = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else if p.extension().is_some_and(|x| x == "crv3d") { files.push(p) }
        }
    }
    for file in files {
        let mut app = App::default();
        if app.open_project(&file).is_err() { continue }
        for t in &app.doc.toolpaths {
            if t.op.kind != crate::toolpath::OpKind::Pocket || !t.op.raster { continue }
            let Some(saved) = &t.kept_moves else { continue };
            let step = if t.op.stepover > 0.0 { t.op.stepover } else { t.op.tool.dia * t.op.tool.stepover };
            let (s, k) = t.op.raster_angle.to_radians().sin_cos();
            let normal = [-s, k];
            let dir = [k, s];
            // Rows: cutting moves that run along the row direction.
            let mut ns: Vec<f64> = saved.windows(2).filter(|w| !w[1].rapid && {
                let d = [w[1].p[0] - w[0].p[0], w[1].p[1] - w[0].p[1]];
                let len = d[0].hypot(d[1]);
                len > step * 0.5 && ((d[0] * dir[0] + d[1] * dir[1]).abs() / len) > 0.9999
            }).map(|w| w[1].p[0] * normal[0] + w[1].p[1] * normal[1]).collect();
            ns.sort_by(f64::total_cmp);
            ns.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
            if ns.len() < 2 { continue }
            let phase = |v: f64| v.rem_euclid(step);
            let (lo, hi) = crate::geom::bbox(&t.op.paths).unwrap();
            let radius = t.op.tool.dia / 2.0 + t.op.allowance;
            let extra = if t.op.profile_pass == crate::toolpath::ProfilePass::None { 0.0 } else { t.op.raster_allowance.abs() };
            let first_for = |lo: [f64; 2], hi: [f64; 2]| {
                let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                let reach = (hi[0] - c[0]).hypot(hi[1] - c[1]) * 1.05;
                c[0] * normal[0] + c[1] * normal[1] - reach
            };
            let inset = |d: f64| first_for([lo[0] + d, lo[1] + d], [hi[0] - d, hi[1] - d]);
            println!("{} | {} | angle {} step {:.4} dia {:.4} allow {:.4} rallow {:.4} pass {:?} rows {}", file.file_name().unwrap().to_string_lossy(), t.op.name, t.op.raster_angle, step, t.op.tool.dia, t.op.allowance, t.op.raster_allowance, t.op.profile_pass, ns.len());
            println!("   saved first {:.4} phase {:.4} | box lo {:?} hi {:?}", ns[0], phase(ns[0]), lo, hi);
            for (name, v) in [("raw", inset(0.0)), ("radius", inset(radius)), ("clear", inset(radius + extra)), ("dia", inset(t.op.tool.dia))] {
                println!("   {name:7} first {:.4} phase {:.4}", v, phase(v));
            }
            let shown: Vec<String> = ns.iter().take(12).map(|v| format!("{v:.4}({:.4})", phase(*v))).collect();
            println!("   rows {}", shown.join(" "));
            // The phase most rows share, in tenths of a millimetre.
            let mut counts = std::collections::BTreeMap::new();
            for v in &ns {
                *counts.entry((phase(*v) * 1000.0).round() as i64).or_insert(0) += 1;
            }
            let mut by: Vec<_> = counts.into_iter().collect();
            by.sort_by_key(|(_, n)| -(*n as i64));
            println!("   phases {:?}", &by[..by.len().min(4)]);
            let mine = crate::toolpath::generate(&t.op, app.doc.machine_on_job(), &crate::toolpath::JobSize { width: app.doc.job.width, height: app.doc.job.height, thickness: app.doc.job.thickness }, None).unwrap_or_default();
            let mut ms: Vec<f64> = mine.windows(2).filter(|w| !w[1].rapid && {
                let d = [w[1].p[0] - w[0].p[0], w[1].p[1] - w[0].p[1]];
                let len = d[0].hypot(d[1]);
                len > step * 2.0 && ((d[0] * dir[0] + d[1] * dir[1]).abs() / len) > 0.9999
            }).map(|w| w[1].p[0] * normal[0] + w[1].p[1] * normal[1]).collect();
            ms.sort_by(f64::total_cmp);
            ms.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
            let shown: Vec<String> = ms.iter().take(12).map(|v| format!("{v:.4}({:.4})", phase(*v))).collect();
            println!("   burin {}", shown.join(" "));
            let hops = |m: &[crate::toolpath::Move]| m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).count();
            println!("   hops saved {} burin {}", hops(saved), hops(&mine));
            let at = |m: &[crate::toolpath::Move]| m.windows(2).filter(|w| w[1].rapid && !w[0].rapid).map(|w| format!("{:.2},{:.2}", w[0].p[0], w[0].p[1])).take(14).collect::<Vec<_>>().join(" ");
            println!("   saved at {}", at(saved));
            println!("   burin at {}", at(&mine));
        }
    }
}

/// Per pocket: BURIN_JOBS=<folder> cargo test raster_rows_by_pocket -- --nocapture
#[test]
fn raster_rows_by_pocket() {
    let Ok(dir) = std::env::var("BURIN_JOBS") else { return };
    let mut files = Vec::new();
    let mut todo = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else if p.extension().is_some_and(|x| x == "crv3d") { files.push(p) }
        }
    }
    for file in files {
        let mut app = App::default();
        if app.open_project(&file).is_err() { continue }
        for t in &app.doc.toolpaths {
            if t.op.kind != crate::toolpath::OpKind::Pocket || !t.op.raster || t.op.raster_angle != 0.0 { continue }
            let Some(saved) = &t.kept_moves else { continue };
            let step = if t.op.stepover > 0.0 { t.op.stepover } else { t.op.tool.dia * t.op.tool.stepover };
            let radius = t.op.tool.dia / 2.0 + t.op.allowance;
            let extra = if t.op.profile_pass == crate::toolpath::ProfilePass::None { 0.0 } else { t.op.raster_allowance.abs() };
            println!("{} | {} | step {step} radius {radius} extra {extra}", file.file_name().unwrap().to_string_lossy(), t.op.name);
            let whole = crate::geom::bbox(&t.op.paths).unwrap();
            for (k, outer) in t.op.paths.iter().enumerate().filter(|(_, p)| p.closed) {
                let (lo, hi) = crate::geom::bbox(std::slice::from_ref(outer)).unwrap();
                // Rows whose middle is in this outline's box and inside it.
                let mut ys: Vec<f64> = saved.windows(2).filter(|w| !w[1].rapid && (w[1].p[1] - w[0].p[1]).abs() < 1e-6 && (w[1].p[0] - w[0].p[0]).abs() > step * 0.5).filter_map(|w| {
                    let m = [(w[0].p[0] + w[1].p[0]) / 2.0, w[0].p[1]];
                    (m[0] > lo[0] && m[0] < hi[0] && m[1] > lo[1] && m[1] < hi[1] && crate::geom::inside_even_odd(std::slice::from_ref(outer), m)).then_some(m[1])
                }).collect();
                ys.sort_by(f64::total_cmp);
                ys.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
                if ys.len() < 3 { continue }
                let ph = |v: f64| v.rem_euclid(step);
                let mut counts = std::collections::BTreeMap::new();
                for v in &ys { *counts.entry((ph(*v) * 1000.0).round() as i64).or_insert(0) += 1; }
                let best = counts.iter().max_by_key(|(_, n)| **n).map(|(p, _)| *p as f64 / 1000.0).unwrap();
                let pred = |lo: [f64; 2], hi: [f64; 2], d: f64, f: f64| {
                    let (lo, hi) = ([lo[0] + d, lo[1] + d], [hi[0] - d, hi[1] - d]);
                    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                    ph(c[1] - (hi[0] - c[0]).hypot(hi[1] - c[1]) * f)
                };
                let offset_box = |level: f64| {
                    let field = crate::geom::Field::build_to(std::slice::from_ref(outer), t.op.tool.dia, 900, t.op.tool.dia / 64.0)?;
                    let loops: Vec<crate::geom::Path> = field.contours(level).into_iter().map(crate::geom::Path::closed).collect();
                    crate::geom::bbox(&loops)
                };
                for t in [0.0, 0.01, 0.0254, 0.05] {
                    if let Some((a, b)) = offset_box(radius + extra - t) {
                        println!("    off-clear-{t} pred+half {:.4} (saved {best:.4})", (pred(a, b, 0.0, 1.05) + step / 2.0).rem_euclid(step));
                    }
                }
                println!("  path {k} box {:.3},{:.3} {:.3},{:.3} rows {} saved phase {best:.4} | own raw {:.4} rad {:.4} clear {:.4} | whole clear {:.4} | lo+rad {:.4} lo+clear {:.4} centre {:.4}",
                    lo[0], lo[1], hi[0], hi[1], ys.len(), pred(lo, hi, 0.0, 1.05), pred(lo, hi, radius, 1.05), pred(lo, hi, radius + extra, 1.05), pred(whole.0, whole.1, radius + extra, 1.05), ph(lo[1] + radius), ph(lo[1] + radius + extra), ph((lo[1] + hi[1]) / 2.0));
            }
        }
    }
}

/// Rapid travel of Shortest Path toolpaths against saved jobs: BURIN_JOBS=<folder> cargo test shortest_travel_against -- --nocapture
#[test]
fn shortest_travel_against_saved_jobs() {
    let Ok(dir) = std::env::var("BURIN_JOBS") else { return };
    let mut files = Vec::new();
    let mut todo = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else if p.extension().is_some_and(|x| x == "crv3d") { files.push(p) }
        }
    }
    files.sort();
    let travel = |m: &[crate::toolpath::Move]| m.windows(2).filter(|w| w[1].rapid).map(|w| (w[1].p[0] - w[0].p[0]).hypot(w[1].p[1] - w[0].p[1])).sum::<f64>();
    let (mut total_saved, mut total_mine, mut n, mut shorter) = (0.0, 0.0, 0, 0);
    let (mut plunge_total, mut plunge_same) = (0, 0);
    let started = std::time::Instant::now();
    let mut kinds = std::collections::BTreeMap::new();
    for file in files {
        let mut app = App::default();
        if app.open_project(&file).is_err() { continue }
        for t in &app.doc.toolpaths {
            *kinds.entry(format!("{:?} {:?}", t.op.order_choices.map(|c| (c.selection, c.left_to_right, c.bottom_to_top, c.grid, c.shortest)), t.op.order)).or_insert(0) += 1;
            let Some(c) = t.op.order_choices else { continue };
            // Only those whose shapes were all found: the others keep their saved moves.
            if c.selection || c.shortest || t.op.kind == crate::toolpath::OpKind::Pocket || t.op.source.is_empty() { continue }
            let Some(saved) = &t.kept_moves else { continue };
            let job = crate::toolpath::JobSize { width: app.doc.job.width, height: app.doc.job.height, thickness: app.doc.job.thickness };
            let Ok(mine) = crate::toolpath::generate(&t.op, app.doc.machine_on_job(), &job, None) else { continue };
            let (a, b) = (travel(saved), travel(&mine));
            total_saved += a;
            total_mine += b;
            n += 1;
            if b <= a + 1.0 { shorter += 1 }
            // Plunges in turn: the first point of each cut after a rapid.
            let plunges = |m: &[crate::toolpath::Move]| m.windows(2).filter(|w| w[0].rapid && !w[1].rapid).map(|w| [w[1].p[0], w[1].p[1]]).collect::<Vec<_>>();
            let (ps, pm) = (plunges(saved), plunges(&mine));
            let same = ps.iter().zip(&pm).filter(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1.0).count();
            plunge_total += ps.len();
            plunge_same += same;
            if ps.len() > 3 && same * 2 < ps.len() {
                println!("{} | {} | {:?} | plunges {} same {} | saved {a:.0} burin {b:.0}", file.file_name().unwrap().to_string_lossy(), t.op.name, t.op.kind, ps.len(), same);
                let show = |v: &[[f64; 2]]| v.iter().take(6).map(|p| format!("{:.0},{:.0}", p[0], p[1])).collect::<Vec<_>>().join(" ");
                println!("   saved {}\n   burin {}", show(&ps), show(&pm));
                println!("   first saved {:?} first burin {:?} home {:?} job {}x{} datum {:?}", saved.first().map(|m| m.p), mine.first().map(|m| m.p), app.doc.machine.home, app.doc.job.width, app.doc.job.height, app.doc.job.datum);
            }

        }
    }
    println!("plunges at the same place in turn: {plunge_same} of {plunge_total}");
    println!("{n} toolpaths: saved {total_saved:.0} mm, burin {total_mine:.0} mm ({:+.1}%), {shorter} as short or shorter, in {:?}", (total_mine / total_saved - 1.0) * 100.0, started.elapsed());
}

/// One toolpath's moves, for looking closely: BURIN_JOB=<file> BURIN_TP=<name> cargo test one_toolpath_moves -- --nocapture
#[test]
fn one_toolpath_moves() {
    let (Ok(file), Ok(name)) = (std::env::var("BURIN_JOB"), std::env::var("BURIN_TP")) else { return };
    let mut app = App::default();
    app.open_project(std::path::Path::new(&file)).unwrap();
    let t = app.doc.toolpaths.iter().find(|t| t.op.name == name).unwrap();
    let job = crate::toolpath::JobSize { width: app.doc.job.width, height: app.doc.job.height, thickness: app.doc.job.thickness };
    let mine = crate::toolpath::generate(&t.op, app.doc.machine_on_job(), &job, None).unwrap();
    println!("home on job {:?}", app.doc.machine_on_job().home);
    for (k, path) in t.op.paths.iter().enumerate() {
        let (lo, hi) = crate::geom::bbox(std::slice::from_ref(path)).unwrap();
        println!("  shape {k}: start {:?} closed {} box {:?} {:?}", path.pts[0].map(|v| v.round()), path.closed, lo.map(|v| v.round()), hi.map(|v| v.round()));
    }
    println!("shapes {} closed {} side {:?} order {:?} start_at {:?} cut {} pass {}", t.op.paths.len(), t.op.paths.iter().filter(|p| p.closed).count(), t.op.side, t.op.order_choices, t.op.start_at, t.op.cut_depth, t.op.tool.pass_depth);
    for (label, m) in [("saved", t.kept_moves.as_ref().unwrap().as_slice()), ("burin", mine.as_slice())] {
        let rapids = m.iter().filter(|x| x.rapid).count();
        let zs: Vec<f64> = m.iter().filter(|x| x.rapid).map(|x| x.p[2]).collect();
        let lowest = zs.iter().copied().fold(f64::MAX, f64::min);
        println!("{label}: {} moves, {rapids} rapid, lowest rapid z {lowest:.3}", m.len());
        for w in m.windows(2).filter(|w| w[0].rapid != w[1].rapid).take(16) {
            println!("   {:?} -> {:?} ({})", w[0].p.map(|v| (v * 100.0).round() / 100.0), w[1].p.map(|v| (v * 100.0).round() / 100.0), if w[1].rapid { "lift" } else { "down" });
        }
    }
}

/// Page Up puts the 2D and 3D views side by side, Page Down one above the other, the view in
/// use first; the same key again goes back to one view, and a click in the other view uses it.
#[test]
fn page_keys_show_both_views_at_once() {
    use crate::app::Tiling;
    let mut h = job_with_square([5.0, 5.0]);
    h.run();
    let whole = h.state().view_rect;
    h.key_press(egui::Key::PageUp);
    h.run();
    h.run();
    assert_eq!(h.state().tiled, Some(Tiling::SideBySide));
    let left = h.state().view_rect;
    assert!(left.width() < whole.width() * 0.6, "{left:?} {whole:?}");
    assert!((left.height() - whole.height()).abs() < 1.0);
    // The square is still drawn where the 2D view says it is, inside the left half.
    let at = h.state().to_screen([5.0, 5.0]);
    assert!(left.contains(at), "{at:?} {left:?}");
    // A click in the right half makes the 3D view the one in use.
    click_at(&mut h, egui::pos2(whole.right() - 40.0, whole.center().y));
    assert!(h.state().view3d);
    h.key_press(egui::Key::PageDown);
    h.run();
    assert_eq!(h.state().tiled, Some(Tiling::Stacked));
    let top = h.state().view_rect;
    assert!(top.height() < whole.height() * 0.6 && top.top() < whole.center().y, "the 3D view in use goes on top: {top:?}");
    h.key_press(egui::Key::PageDown);
    h.run();
    assert_eq!(h.state().tiled, None);
}

/// The view toolbar's last two buttons are the original's Tile Windows Vertical and Horizontal.
#[test]
fn toolbar_tiles_the_views() {
    use crate::app::Tiling;
    let mut h = job_with_square([5.0, 5.0]);
    h.run();
    click(&mut h, "Tile Windows Vertical");
    assert_eq!(h.state().tiled, Some(Tiling::SideBySide));
    click(&mut h, "Tile Windows Horizontal");
    assert_eq!(h.state().tiled, Some(Tiling::Stacked));
    click(&mut h, "Tile Windows Horizontal");
    assert_eq!(h.state().tiled, None);
}

/// With a component chosen and no shape, Up and Down move through the component list and
/// stop at its ends, and Delete takes the component away, as the original's component tree does.
#[test]
fn component_list_takes_the_arrow_and_delete_keys() {
    let mut h = harness();
    h.state_mut().load_sample();
    h.state_mut().left_tab = crate::app::LeftTab::Modeling;
    h.state_mut().selected.clear();
    h.state_mut().selected_comp = Some(0);
    h.run();
    let count = h.state().doc.components.len();
    assert!(count >= 2, "the sample has {count} components");
    h.key_press(egui::Key::ArrowDown);
    h.run();
    assert_eq!(h.state().selected_comp, Some(1));
    h.key_press(egui::Key::ArrowUp);
    h.run();
    h.key_press(egui::Key::ArrowUp);
    h.run();
    assert_eq!(h.state().selected_comp, Some(0), "it stops at the top");
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(h.state().doc.components.len(), count - 1);
    assert_eq!(h.state().selected_comp, None);
}

/// Add Zero Plane puts a flat component under the whole model, so a dish cut after it goes
/// below zero and a 3D finishing toolpath then carves it into the material.
#[test]
fn a_zero_plane_lets_a_dish_go_into_the_material() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().open_shape_form(None);
    let crate::app::LeftForm::Shape(mut f) = h.state().left.clone() else { panic!("the shape form") };
    f.combine = crate::relief::Combine::Subtract;
    f.profile = crate::relief::Profile::Round;
    f.height = 0.25;
    h.state_mut().add_component(f);
    h.state_mut().refresh_model();
    assert!(h.state().model.as_ref().map_or(true, |m| m.data.iter().all(|v| *v == 0.0)), "nothing to cut into");
    h.state_mut().left_tab = crate::app::LeftTab::Modeling;
    h.state_mut().left = LeftForm::None;
    h.run();
    click(&mut h, "Add Zero Plane");
    assert_eq!(h.state().doc.components[0].name, "Zero Plane");
    assert!(h.state().doc.components[0].whole);
    h.state_mut().refresh_model();
    let low = h.state().model.as_ref().unwrap().data.iter().cloned().fold(0.0f32, f32::min);
    assert!(low < -0.2, "the dish goes below zero: {low}");
    h.state_mut().do_undo();
    assert!(h.state().doc.components.iter().all(|c| c.name != "Zero Plane"), "one step to undo");
}

/// Space opens the last drawing form again, as the original's manual says.
#[test]
fn space_opens_the_last_drawing_form_again() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().selected.clear();
    h.run();
    h.key_press(egui::Key::Space);
    h.run();
    assert!(matches!(h.state().left, LeftForm::None), "nothing drawn yet: nothing to open");
    h.state_mut().start_draw(crate::app::DrawKind::Star);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(matches!(h.state().left, LeftForm::None));
    h.key_press(egui::Key::Space);
    h.run();
    assert!(matches!(&h.state().left, LeftForm::Draw(f) if f.kind == crate::app::DrawKind::Star));
}

/// Groups go inside groups: New Group with a group chosen makes one inside it, New makes a tool
/// in it, and a group dragged onto another goes inside with everything it holds.
#[test]
fn tool_groups_can_hold_groups() {
    use crate::panels::{group_paths, move_group};
    let mut tools = crate::toolpath::default_tools();
    tools[0].group = "Roughing/Big".into();
    let mut groups = vec!["Finishing".to_string()];
    assert_eq!(group_paths(&groups, &tools), ["Finishing", "Roughing", "Roughing/Big"]);
    // Finishing goes inside Roughing; Roughing can't go inside its own Big.
    assert_eq!(move_group(&mut groups, &mut tools, "Finishing", "Roughing").as_deref(), Some("Roughing/Finishing"));
    assert_eq!(move_group(&mut groups, &mut tools, "Roughing", "Roughing/Big"), None);
    // Big, with its tool, goes to the top.
    assert_eq!(move_group(&mut groups, &mut tools, "Roughing/Big", "").as_deref(), Some("Big"));
    assert_eq!(tools[0].group, "Big");
    assert_eq!(group_paths(&groups, &tools), ["Big", "Roughing", "Roughing/Finishing"]);

    let mut h = harness();
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.state_mut().open_tool_list(None);
    h.state_mut().tool_groups_edit = vec!["Roughing".into()];
    h.run();
    click(&mut h, "Roughing");
    assert_eq!(h.state().tool_group_picked.as_deref(), Some("Roughing"));
    click(&mut h, "New Group");
    h.get_all_by_value("New Group").next().unwrap().focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_all_by_value("New Group").next().unwrap().type_text("Big");
    h.run();
    click(&mut h, "Apply");
    assert!(h.state().tool_groups_edit.contains(&"Roughing/Big".to_string()), "{:?}", h.state().tool_groups_edit);
    assert!(h.query_by_label("Big").is_some(), "shown inside Roughing");
    click(&mut h, "Big");
    click(&mut h, "New ...");
    let tools = h.state().tool_edit.clone().unwrap();
    assert_eq!(tools.last().unwrap().group, "Roughing/Big", "the new tool goes into the chosen group");
}

/// Export as 3D Clipart writes the whole model, every component in order, as the original's
/// does; importing it or choosing it in the Clipart tab's folder brings them back together in the
/// middle of the job. With nothing in the model it refuses.
#[test]
fn the_model_goes_out_as_3d_clipart_and_comes_back() {
    let mut h = job_with_square([2.0, 2.0]);
    h.state_mut().export_3d_clipart();
    assert!(h.state_mut().message.take().is_some(), "nothing in the model: refused");
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().open_shape_form(None);
    let LeftForm::Shape(f) = h.state().left.clone() else { panic!("the shape form") };
    h.state_mut().add_component(f);
    h.state_mut().left = LeftForm::None;
    h.state_mut().add_zero_plane();
    let cs = h.state().doc.components.clone();
    assert_eq!(cs.len(), 2);
    let text = crate::modeltools::clip3d_text(&cs);
    let back = crate::modeltools::read_clip3d(&text).unwrap();
    assert_eq!(back.len(), 2, "every component");
    assert_eq!(back[0].name, "Zero Plane", "in their order");
    let all: Vec<crate::geom::Path> = back.iter().flat_map(|c| c.paths.clone()).collect();
    let (lo, _) = crate::geom::bbox(&all).unwrap();
    assert!(lo[0].abs() < 1e-9 && lo[1].abs() < 1e-9, "kept from their corner");
    // A file of the first version, with one component, still reads.
    let old = serde_json::json!({ "burin_3dclip": 1, "component": cs[1] }).to_string();
    assert_eq!(crate::modeltools::read_clip3d(&old).unwrap().len(), 1);
    assert!(crate::modeltools::read_clip3d("{}").is_err());
    let dir = std::env::temp_dir().join(format!("burin-clip3d-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dome.3dclip"), &text).unwrap();
    h.state_mut().import_3d_clipart(&dir.join("dome.3dclip")).unwrap();
    assert_eq!(h.state().doc.components.len(), 4);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.components[2].paths).unwrap();
    let j = h.state().doc.job.clone();
    assert!(((lo[0] + hi[0]) / 2.0 - j.width / 2.0).abs() < 1e-9, "together in the middle of the job");
    assert_eq!(h.state().selected_comp, Some(3));
    // In a library folder of the Clipart tab, a click brings it in.
    h.state_mut().clip_folders = vec![dir.clone()];
    h.state_mut().clip_folder = 1;
    h.state_mut().clip_tab = 0;
    h.state_mut().left_tab = crate::app::LeftTab::Clipart;
    h.run();
    h.run();
    click(&mut h, "dome");
    assert_eq!(h.state().doc.components.len(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Create Textured Area Component repeats the chosen component inside the chosen closed shape,
/// as a new component, and hides the tile.
#[test]
fn a_component_repeats_inside_a_shape_as_a_texture() {
    let mut h = job_with_square([2.0, 2.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().open_shape_form(None);
    let LeftForm::Shape(f) = h.state().left.clone() else { panic!("the shape form") };
    h.state_mut().add_component(f);
    h.state_mut().left = LeftForm::None;
    let bound = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(vec![[1.0, 4.0], [9.0, 4.0], [9.0, 9.0], [1.0, 9.0]])], Prim::Free);
    h.state_mut().selected = vec![bound];
    h.state_mut().selected_comp = Some(0);
    h.state_mut().open_texture_area();
    assert!(h.state().texture_form.is_some());
    h.run();
    click(&mut h, "Apply");
    let cs = &h.state().doc.components;
    assert_eq!(cs.len(), 2);
    assert!(!cs[0].visible, "the tile is hidden");
    assert!(cs[1].name.starts_with("Texture Area"));
    h.state_mut().refresh_model();
    let m = h.state().model.clone().unwrap();
    assert_eq!(m.sample(5.0, 2.0), 0.0, "nothing outside the boundary");
    assert!((4..9).any(|y| m.sample(5.0, y as f64 + 0.5) > 0.0), "tiles inside it");
    assert!(h.state().texture_form.is_none());
    h.state_mut().do_undo();
    assert_eq!(h.state().doc.components.len(), 1);
}

/// Joins in the original's saved jobs come through into the imported toolpaths, one for each
/// stored move of that kind (`BURIN_JOBS` points at a folder of .crv3d jobs).
#[test]
fn joins_come_in_from_saved_jobs() {
    let Ok(dir) = std::env::var("BURIN_JOBS") else { return };
    let mut files = Vec::new();
    let mut todo = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else if p.extension().is_some_and(|x| x == "crv3d") { files.push(p) }
        }
    }
    let (mut stored, mut kept) = (0, 0);
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        let pattern = [0x20u8, 0, 0, 0, crate::crv3d::JOIN_KIND as u8, 0, 0, 0];
        stored += bytes.windows(8).filter(|w| *w == pattern).count();
        let mut app = App::default();
        if app.open_project(&file).is_err() { continue }
        kept += app.doc.toolpaths.iter().filter_map(|t| t.kept_moves.as_ref()).map(|m| m.iter().filter(|m| m.join).count()).sum::<usize>();
    }
    eprintln!("joins stored {stored}, brought in {kept}");
    assert!(kept > 0 && kept * 10 >= stored * 9, "{kept} of {stored}");
}

/// A two-sided job: 1 and 2 choose the side, what's drawn goes on the side in use, only that
/// side's shapes and toolpaths show and can be chosen, and the rulers turn orange underneath.
#[test]
fn a_two_sided_job_keeps_each_side_apart() {
    let mut h = job_with_square([5.0, 5.0]);
    let top = h.state().doc.vectors[0].id;
    h.state_mut().doc.job.two_sided = true;
    h.run();
    h.key_press(egui::Key::Num2);
    h.run();
    assert_eq!(h.state().doc.active_face, 1);
    assert!(h.query_by_label("Toggle Top / Bottom Side").is_some(), "the side button shows");
    let bottom = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::circle([3.0, 3.0], 1.0))], Prim::Free);
    assert_eq!(h.state().doc.vectors.iter().find(|v| v.id == bottom).unwrap().face, 1);
    // Select all takes only the bottom side's shape.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert_eq!(h.state().selected, vec![bottom]);
    // A toolpath made now is the bottom side's, and is listed only there.
    h.state_mut().right_open = true;
    h.state_mut().start_op(crate::toolpath::OpKind::Profile);
    h.state_mut().calculate_op();
    h.state_mut().right = RightMode::List;
    h.run();
    assert_eq!(h.state().doc.toolpaths[0].face, 1);
    assert!(h.query_by_label("Toolpaths (Bottom)").is_some());
    h.key_press(egui::Key::Num1);
    h.run();
    assert_eq!(h.state().doc.active_face, 0);
    assert!(h.state().selected.is_empty(), "changing side drops the selection");
    assert!(h.query_by_label("Toolpaths (Top)").is_some());
    assert!(!h.state().doc.toolpaths.iter().any(|t| h.state().doc.toolpath_on_side(t)), "the top side has none");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    assert_eq!(h.state().selected, vec![top]);
    // Turning the block the other way puts the bottom side's shapes half a turn round the middle.
    h.state_mut().change_flip_direction();
    assert!(h.state().doc.job.flip_around_y);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors.iter().find(|v| v.id == bottom).unwrap().paths).unwrap();
    assert!(((lo[0] + hi[0]) / 2.0 - 7.0).abs() < 1e-6 && ((lo[1] + hi[1]) / 2.0 - 7.0).abs() < 1e-6);
    // One step undoes it, as the original's batch does (`0x1402a75d0`).
    h.state_mut().do_undo();
    assert!(!h.state().doc.job.flip_around_y);
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors.iter().find(|v| v.id == bottom).unwrap().paths).unwrap();
    assert!(((lo[0] + hi[0]) / 2.0 - 3.0).abs() < 1e-6, "back where it was");
    // A single-sided job shows everything again.
    h.state_mut().doc.job.two_sided = false;
    assert!(h.state().doc.vectors.iter().all(|v| h.state().doc.shows(v)));
}

/// Preview All Sides: the bottom side's toolpaths cut their side of the same block, turned over
/// the way the job flips, and where the two sides' cuts meet the material is gone.
#[test]
fn both_sides_preview_as_one_block() {
    use crate::toolpath::{Move, OpKind};
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.job.two_sided = true;
    h.state_mut().right_open = true;
    let t = h.state().doc.job.thickness;
    // A slot 0.3 deep from the top along y = 3, and one 0.3 deep from underneath at the same spot
    // (around X, y on the bottom is H - y).
    let slot = |y: f64| -> Vec<Move> {
        let mut m = vec![Move { p: [1.0, y, 0.5], rapid: true, join: false }];
        m.push(Move { p: [1.0, y, -0.3], rapid: false, join: false });
        m.push(Move { p: [9.0, y, -0.3], rapid: false, join: false });
        m.push(Move { p: [9.0, y, 0.5], rapid: true, join: false });
        m
    };
    let mut op = crate::toolpath::blank_op(OpKind::Profile, "p".into(), crate::toolpath::default_tools()[0].clone(), 0.5, Vec::new());
    op.tool.dia = 0.5;
    for (face, y) in [(0u8, 3.0), (1u8, 10.0 - 3.0)] {
        let moves = std::sync::Arc::new(slot(y));
        h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: op.clone(), visible: true, color: [0, 0, 0], moves, kept_moves: None, kept_steps: None, face, sheet: 0 });
    }
    // A bottom-only slot elsewhere, which doesn't meet anything from the top.
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: op.clone(), visible: true, color: [0, 0, 0], moves: std::sync::Arc::new(slot(10.0 - 7.0)), kept_moves: None, kept_steps: None, face: 1, sheet: 0 });
    assert!(t < 0.6 + 1e-9, "0.3 from each side reaches through {t}");
    h.state_mut().preview_all_sides();
    h.state_mut().finish_preview();
    let sim = h.state().sim.clone().unwrap();
    let at = |x: f64, y: f64| {
        let g = &sim.grid;
        sim.is_through((x / g.cell).round() as usize, (y / g.cell).round() as usize)
    };
    assert!(at(5.0, 3.0), "the two slots meet: through");
    assert!(!at(5.0, 7.0), "the bottom slot alone stays hidden underneath");
    assert!(!at(5.0, 5.0), "uncut");
    // Previewing one side again starts afresh, without the other side.
    h.state_mut().sim_floor = None;
    h.state_mut().sim = None;
    h.state_mut().preview(vec![0]);
    h.state_mut().finish_preview();
    let sim = h.state().sim.clone().unwrap();
    let g = &sim.grid;
    assert!(!sim.is_through((5.0 / g.cell).round() as usize, (3.0 / g.cell).round() as usize));
}

/// With the bottom side active, Preview All Sides carves the bottom as the side looked at, and the
/// top side's cuts meet it from the other side, as the original does with the active side first.
#[test]
fn both_sides_preview_from_the_active_side() {
    use crate::toolpath::{Move, OpKind};
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().doc.job.two_sided = true;
    let slot = |y: f64| -> std::sync::Arc<Vec<Move>> {
        std::sync::Arc::new(vec![
            Move { p: [1.0, y, 0.5], rapid: true, join: false },
            Move { p: [1.0, y, -0.3], rapid: false, join: false },
            Move { p: [9.0, y, -0.3], rapid: false, join: false },
            Move { p: [9.0, y, 0.5], rapid: true, join: false },
        ])
    };
    let mut op = crate::toolpath::blank_op(OpKind::Profile, "p".into(), crate::toolpath::default_tools()[0].clone(), 0.5, Vec::new());
    op.tool.dia = 0.5;
    // From the top at y = 3 (on the bottom that's y = 7), and a top-only slot at y = 8.
    for (face, y) in [(0u8, 3.0), (1u8, 7.0), (0u8, 8.0)] {
        h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: op.clone(), visible: false, color: [0, 0, 0], moves: slot(y), kept_moves: None, kept_steps: None, face, sheet: 0 });
    }
    h.state_mut().set_active_face(1);
    h.state_mut().preview_all_sides();
    h.state_mut().finish_preview();
    let sim = h.state().sim.clone().unwrap();
    let at = |x: f64, y: f64| sim.is_through((x / sim.grid.cell).round() as usize, (y / sim.grid.cell).round() as usize);
    assert!(at(5.0, 7.0), "seen from the bottom, the slots meet at its y = 7");
    assert!(!at(5.0, 2.0), "the top-only slot (bottom y = 2) doesn't go through");
    assert_eq!(h.state().doc.active_face, 1, "the active side stays active");
}

/// In a narrow window the two-sided buttons stay inside the view's toolbar.
#[test]
fn two_sided_buttons_fit_a_narrow_window() {
    let mut h = Harness::builder().with_size(vec2(943.0, 700.0)).with_accessibility_check(false).build_ui_state(|ui, app: &mut App| app.frame(ui), App::default());
    crate::ui::setup_style(&h.ctx);
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.state_mut().right_open = true;
    h.state_mut().doc.job.two_sided = true;
    h.run();
    h.run();
    for name in ["Change flip direction", "Toggle Top / Bottom Side"] {
        let r = h.get_by_label(name).rect();
        assert!(r.right() <= 943.0 - 282.0, "{name} ends at {}", r.right());
    }
    click(&mut h, "Toggle Top / Bottom Side");
    assert_eq!(h.state().doc.active_face, 1);
}

/// `=` turns the Multi-Sided View on and off, only in a two-sided job, and Preview All Sides
/// leaves it on, as the original does.
#[test]
fn equals_toggles_the_multi_sided_view() {
    let mut h = job_with_square([5.0, 5.0]);
    h.key_press(egui::Key::Equals);
    h.run();
    assert!(!h.state().multi_sided, "a one-sided job has no other side");
    h.state_mut().doc.job.two_sided = true;
    h.key_press(egui::Key::Equals);
    h.run();
    assert!(h.state().multi_sided);
    h.key_press(egui::Key::Equals);
    h.run();
    assert!(!h.state().multi_sided);
    let mut op = crate::toolpath::blank_op(crate::toolpath::OpKind::Profile, "p".into(), crate::toolpath::default_tools()[0].clone(), 0.5, Vec::new());
    op.tool.dia = 0.5;
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op, visible: true, color: [0, 0, 0], moves: Default::default(), kept_moves: None, kept_steps: None, face: 1, sheet: 0 });
    h.state_mut().preview_all_sides();
    assert!(h.state().multi_sided);
}

/// With a text chosen, a click between two of its letters brings them closer, and Shift moves
/// them apart, as the original's text spacing clicks do.
#[test]
fn a_click_between_letters_moves_them() {
    let mut h = job_with_square([5.0, 5.0]);
    let prim = Prim::Text { at: [2.0, 5.0], height: 1.0, text: "WAV".into(), style: crate::fonts::TextStyle::default() };
    let id = h.state_mut().doc.add_vector(prim.paths(), prim);
    h.state_mut().selected = vec![id];
    h.run();
    let carets = crate::fonts::letter_carets("WAV", 1.0, [2.0, 5.0], &crate::fonts::TextStyle::default());
    let a = carets[1];
    let gap = h.state().to_screen([a.x, a.base + 0.5]);
    assert!(h.state().text_gap_under(gap).is_some());
    click_at(&mut h, gap);
    let gaps = |h: &Harness<'static, App>| match &h.state().doc.vectors.iter().find(|v| v.id == id).unwrap().prim {
        Prim::Text { style, .. } => style.letter_gaps.clone(),
        _ => Vec::new(),
    };
    assert!(gaps(&h).get(1).is_some_and(|g| *g < 0.0), "closer: {:?}", gaps(&h));
    assert_eq!(h.state().selected, vec![id], "the text stays chosen");
    h.state_mut().do_undo();
    assert!(gaps(&h).is_empty(), "one undo step");
}

/// Sculpt Visible Model: the chosen component becomes heights, a stroke of Deposit raises it in
/// the model, each stroke is one undo step, and Close finishes.
#[test]
fn sculpting_raises_the_model_under_the_brush() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().open_shape_form(None);
    let LeftForm::Shape(f) = h.state().left.clone() else { panic!("the shape form") };
    h.state_mut().add_component(f);
    h.state_mut().left = LeftForm::None;
    h.state_mut().selected_comp = Some(0);
    h.state_mut().refresh_model();
    let before = h.state().model.as_ref().unwrap().sample(5.0, 5.0);
    h.state_mut().open_sculpt();
    assert!(h.state().sculpt.is_some() && h.state().view3d);
    h.state_mut().sculpt.as_mut().unwrap().tool = crate::sculpt::SculptTool::Deposit;
    h.state_mut().sculpt_at([5.0, 5.0], true, false);
    h.state_mut().sculpt_at([5.1, 5.0], false, false);
    h.state_mut().refresh_model();
    let after = h.state().model.as_ref().unwrap().sample(5.0, 5.0);
    assert!(after > before + 1e-3, "{before} -> {after}");
    assert!(h.state().doc.components[0].surface.is_some(), "now heights of its own");
    h.state_mut().do_undo();
    h.state_mut().refresh_model();
    assert!((h.state().model.as_ref().unwrap().sample(5.0, 5.0) - before).abs() < 1e-3, "one undo step for the stroke");
    h.run();
    click(&mut h, "Close");
    assert!(h.state().sculpt.is_none());
}

/// Emboss Component: the chosen component's heights become the shape of its slopes, at 10% of
/// its height at first, in one undo step.
#[test]
fn emboss_flattens_a_component() {
    let mut h = job_with_square([5.0, 5.0]);
    let id = h.state().doc.vectors[0].id;
    h.state_mut().selected = vec![id];
    h.state_mut().open_shape_form(None);
    let LeftForm::Shape(mut f) = h.state().left.clone() else { panic!("the shape form") };
    f.profile = crate::relief::Profile::Round;
    h.state_mut().add_component(f);
    h.state_mut().left = LeftForm::None;
    h.state_mut().selected_comp = Some(0);
    h.state_mut().refresh_model();
    let tall = h.state().model.as_ref().unwrap().max();
    h.state_mut().open_emboss();
    h.run();
    click(&mut h, "Apply");
    assert!(h.state().emboss_form.is_none());
    h.state_mut().refresh_model();
    let flat = h.state().model.as_ref().unwrap().max();
    assert!(flat < tall * 0.11 && flat > 0.0, "{tall} -> {flat}");
    h.state_mut().do_undo();
    h.state_mut().refresh_model();
    assert!((h.state().model.as_ref().unwrap().max() - tall).abs() < 1e-4);
}

/// Two Rail Sweep: the first two chosen shapes are the rails, the open ones after them the cross
/// sections, and Apply makes a component between the rails.
#[test]
fn two_rails_and_a_section_make_a_component() {
    let mut h = job_with_square([5.0, 5.0]);
    let a = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 2.0], [9.0, 2.0]])], Prim::Free);
    let b = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 4.0], [9.0, 4.0]])], Prim::Free);
    let arch = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[0.0, 0.0], [0.5, 0.3], [1.0, 0.0]])], Prim::Free);
    h.state_mut().selected = vec![a, b, arch];
    h.state_mut().open_two_rail_sweep();
    let form = h.state().sweep_form.clone().unwrap();
    assert_eq!(form.rails, vec![a, b]);
    assert_eq!(form.sections, vec![arch]);
    assert!(form.scale_with_width && form.between_spans && !form.fill_centre && !form.exact_height, "the original's starting choices");
    h.run();
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.components.len(), 1);
    h.state_mut().refresh_model();
    let m = h.state().model.clone().unwrap();
    assert!(m.sample(5.0, 3.0) > 0.4, "the arch scaled to the 2 between the rails: {}", m.sample(5.0, 3.0));
    assert_eq!(m.sample(5.0, 6.0), 0.0);
}

/// Extrude and Weave: the rails chosen first, the open cross section last, and Apply makes a
/// component along the rails, with the original's starting choices.
#[test]
fn a_section_extruded_along_a_rail_makes_a_component() {
    let mut h = job_with_square([5.0, 5.0]);
    let rail = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 2.0], [9.0, 2.0]])], Prim::Free);
    let bump = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[0.0, 0.0], [0.5, 0.3], [1.0, 0.0]])], Prim::Free);
    h.state_mut().selected = vec![rail, bump];
    h.state_mut().open_extrude();
    let form = h.state().extrude_form.clone().unwrap();
    assert_eq!((form.rails.clone(), form.sections.clone()), (vec![rail], vec![bump]));
    assert!(form.sharp_corners && !form.weave && (form.under - 0.7).abs() < 1e-9 && (form.over - 1.3).abs() < 1e-9);
    h.run();
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.components.len(), 1);
    h.state_mut().refresh_model();
    assert!(h.state().model.as_ref().unwrap().sample(5.0, 2.0) > 0.25);
}

/// Extrude and Weave checks the rails first, then the cross section, with the original's
/// messages (`0x14074a880`, `0x14074b4d0`), and makes one rail of every path of a chosen vector.
#[test]
fn extrude_checks_rails_then_section_and_uses_every_path() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().extrude(&crate::sweep::ExtrudeForm::default());
    assert!(h.state().message.as_deref().unwrap().contains("rails"));
    let two = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 2.0], [9.0, 2.0]]), crate::geom::Path::open(vec![[1.0, 8.0], [9.0, 8.0]])], Prim::Free);
    let form = crate::sweep::ExtrudeForm { rails: vec![two], ..Default::default() };
    assert!(!h.state_mut().extrude(&form));
    assert!(h.state().message.as_deref().unwrap().contains("cross section"));
    let bump = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[0.0, 0.0], [0.5, 0.3], [1.0, 0.0]])], Prim::Free);
    assert!(h.state_mut().extrude(&crate::sweep::ExtrudeForm { sections: vec![bump], ..form }));
    h.state_mut().refresh_model();
    let m = h.state().model.as_ref().unwrap();
    assert!(m.sample(5.0, 2.0) > 0.25 && m.sample(5.0, 8.0) > 0.25, "both paths carry the section");
}

/// Extrude and Weave with no cross section uses the chosen component, repeated along the rail.
#[test]
fn the_chosen_component_is_extruded_along_a_rail() {
    let mut h = job_with_square([5.0, 5.0]);
    let bead = h.state().doc.components.len();
    h.state_mut().doc.components.push(crate::relief::Component { name: "Bead".into(), paths: vec![crate::geom::Path::closed(crate::geom::circle([1.0, 9.0], 0.4))], profile: crate::relief::Profile::Round, height: 0.3, base: 0.0, combine: crate::relief::Combine::Add, visible: false, surface: None, whole: false });
    let rail = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[1.0, 2.0], [9.0, 2.0]])], Prim::Free);
    h.state_mut().selected = vec![rail];
    h.state_mut().open_extrude();
    h.state_mut().selected_comp = Some(bead);
    h.run();
    click(&mut h, "Apply");
    assert_eq!(h.state().doc.components.len(), bead + 2);
    h.state_mut().refresh_model();
    let m = h.state().model.as_ref().unwrap();
    let top = (0..80).map(|k| m.sample(1.0 + k as f64 * 0.1, 2.0)).fold(0.0f32, f32::max);
    assert!(top > 0.2, "{top}");
}

/// Two Rail Sweep takes rails in left and right pairs, as the original does, and sweeps each pair
/// with the same cross section; an odd number of rails is refused.
#[test]
fn two_rail_sweep_sweeps_every_pair_of_rails() {
    let mut h = job_with_square([5.0, 5.0]);
    let line = |h: &mut Harness<'static, App>, a: [f64; 2], b: [f64; 2]| h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![a, b])], Prim::Free);
    let (l1, r1) = (line(&mut h, [1.0, 1.0], [9.0, 1.0]), line(&mut h, [1.0, 2.0], [9.0, 2.0]));
    let (l2, r2) = (line(&mut h, [1.0, 7.0], [9.0, 7.0]), line(&mut h, [1.0, 8.0], [9.0, 8.0]));
    let section = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[0.0, 0.0], [0.5, 0.3], [1.0, 0.0]])], Prim::Free);
    let mut form = crate::sweep::SweepForm { rails: vec![l1, r1, l2], sections: vec![section], ..Default::default() };
    assert!(!h.state_mut().two_rail_sweep(&form));
    assert!(h.state().message.as_deref().unwrap().contains("pairs"));
    form.rails.push(r2);
    assert!(h.state_mut().two_rail_sweep(&form));
    h.state_mut().refresh_model();
    let m = h.state().model.as_ref().unwrap();
    assert!(m.sample(5.0, 1.5) > 0.1 && m.sample(5.0, 7.5) > 0.1, "both pairs swept");
    assert!(m.sample(5.0, 4.5) < 0.05, "nothing between the pairs");
}

/// A set grid spacing is what pointer positions snap to, whatever the zoom; 0 goes back to
/// following the zoom.
#[test]
fn a_set_grid_spacing_is_snapped_to() {
    let mut h = job_with_square([5.0, 5.0]);
    h.state_mut().snap = true;
    h.state_mut().snap_shapes = false;
    h.state_mut().show_guides = false;
    h.state_mut().grid_spacing = 0.75;
    let p = h.state().snapped([1.1, 2.0]);
    let zero = h.state().doc.job.origin();
    let on = |v: f64, z: f64| (((v - z) / 0.75).round() * 0.75 + z - v).abs() < 1e-9;
    assert!(on(p[0], zero[0]) && on(p[1], zero[1]), "{p:?}");
    h.state_mut().grid_spacing = 0.0;
    assert!(h.state().grid_steps().0 > 0.0);
}

/// Several sheets: each has its own shapes and toolpaths, only the one showing is seen and cut,
/// shapes can be sent to another, and deleting a sheet takes its shapes with it.
#[test]
fn sheets_keep_their_own_shapes_and_toolpaths() {
    let mut h = job_with_square([5.0, 5.0]);
    let first = h.state().doc.vectors[0].id;
    assert_eq!(h.state().doc.sheet_count(), 1);
    h.state_mut().add_sheet();
    assert_eq!((h.state().doc.sheet_count(), h.state().doc.active_sheet), (2, 1));
    let doc = &h.state().doc;
    assert!(!doc.shows(&doc.vectors[0]), "the first sheet's square is not on the second");
    let second = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([3.0, 3.0], 1.0, 1.0, 0.0))], Prim::Free);
    let on = |h: &Harness<'static, App>, id: u64| h.state().doc.vectors.iter().find(|v| v.id == id).map(|v| v.sheet);
    assert_eq!(on(&h, second), Some(1));
    // A toolpath made now belongs to this sheet.
    h.state_mut().doc.toolpaths.push(crate::app::Toolpath { op: crate::toolpath::blank_op(crate::toolpath::OpKind::Profile, "p".into(), crate::toolpath::default_tools()[0].clone(), 0.1, Vec::new()), visible: true, color: [0, 0, 0], moves: Default::default(), kept_moves: None, kept_steps: None, face: 0, sheet: 1 });
    assert!(h.state().doc.toolpath_on_side(&h.state().doc.toolpaths[0]));
    h.state_mut().show_sheet(0);
    assert!(!h.state().doc.toolpath_on_side(&h.state().doc.toolpaths[0]));
    // Send the square to sheet 2, then delete sheet 2: both shapes and the toolpath go.
    h.state_mut().selected = vec![first];
    h.state_mut().move_selected_to_sheet(1);
    assert_eq!(on(&h, first), Some(1));
    h.state_mut().show_sheet(1);
    h.state_mut().delete_sheet();
    assert_eq!(h.state().doc.sheet_count(), 1);
    assert!(h.state().doc.vectors.is_empty() && h.state().doc.toolpaths.is_empty());
    h.state_mut().do_undo();
    assert_eq!((h.state().doc.sheet_count(), h.state().doc.vectors.len()), (2, 2));
}

/// Nesting more parts than fit on the sheet puts the rest on new sheets after the others, laid
/// out the same way, as the original does.
#[test]
fn nesting_more_than_fits_adds_sheets() {
    let mut h = harness();
    h.state_mut().new_file();
    let mut ids = Vec::new();
    // Twelve 3 inch squares on a 10 inch sheet: nine fit at most.
    for k in 0..12 {
        let c = [1.0 + (k % 4) as f64 * 0.5, 1.0 + (k / 4) as f64 * 0.5];
        ids.push(h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect(c, 3.0, 3.0, 0.0))], Prim::Free));
    }
    h.state_mut().selected = ids.clone();
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    h.run();
    let doc = &h.state().doc;
    assert!(doc.sheet_count() >= 2, "{}", h.state().status);
    let on_second = doc.vectors.iter().filter(|v| v.sheet == 1).count();
    assert!(on_second > 0 && on_second < 12);
    assert!(h.state().status.contains("sheets"), "{}", h.state().status);
    // Every part sits inside the sheet, whichever it is on.
    for v in &doc.vectors {
        let (lo, hi) = crate::geom::bbox(&v.paths).unwrap();
        assert!(lo[0] >= -1e-9 && lo[1] >= -1e-9 && hi[0] <= 10.0 + 1e-9 && hi[1] <= 10.0 + 1e-9, "{lo:?} {hi:?}");
    }
}

/// With rotation allowed, nesting turns parts too tall for the sheet to lie along the rows, so
/// they fit; without it they run past the sheet.
#[test]
fn nesting_may_turn_parts() {
    let make = |h: &mut Harness<'static, App>| {
        h.state_mut().new_file();
        h.state_mut().doc.job.height = 5.0;
        let ids: Vec<u64> = (0..3).map(|k| h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([2.0 + k as f64 * 1.5, 2.5], 1.0, 6.0, 0.0))], Prim::Free)).collect();
        h.state_mut().selected = ids;
        h.state_mut().start_xform(Xform::Nest);
    };
    let inside = |h: &Harness<'static, App>| h.state().doc.vectors.iter().all(|v| {
        let (lo, hi) = crate::geom::bbox(&v.paths).unwrap();
        lo[0] >= -1e-9 && hi[0] <= 10.0 + 1e-9 && lo[1] >= -1e-9 && hi[1] <= 5.0 + 1e-9
    });
    let mut h = harness();
    make(&mut h);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        assert!(f.nest_turn && f.part_in_part && f.nest_step == 45.0, "the original's starting values");
        f.nest_turn = false;
    }
    h.state_mut().apply_xform();
    assert!(!inside(&h), "upright they run past the sheet");
    make(&mut h);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.nest_turn = true;
    }
    h.state_mut().apply_xform();
    h.run();
    assert!(inside(&h), "{}", h.state().status);
    assert_eq!(h.state().doc.sheet_count(), 1);
    for v in &h.state().doc.vectors {
        let (lo, hi) = crate::geom::bbox(&v.paths).unwrap();
        assert!(hi[0] - lo[0] > hi[1] - lo[1], "lying along X: {lo:?} {hi:?}");
    }
}

/// A negative border, clearance or tool size counts as 0 when nesting, as in the original's form.
#[test]
fn nesting_settings_below_zero_count_as_zero() {
    assert_eq!(crate::edit::nest_value(-2.0), 0.0);
    assert_eq!(crate::edit::nest_value(0.3), 0.3);
    let mut h = harness();
    h.state_mut().new_file();
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 2.0, 2.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.state_mut().start_xform(Xform::Nest);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.border = -1.0;
    }
    h.state_mut().apply_xform();
    let (lo, _) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!(lo[0].abs() < 1e-9 && lo[1].abs() < 1e-9, "from the corner itself: {lo:?}");
}

/// Part in part: small parts land in the hole of a frame and move with it.
#[test]
fn nesting_puts_small_parts_in_holes() {
    let mut h = harness();
    h.state_mut().new_file();
    let frame = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 6.0, 6.0, 0.0)), crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 4.0, 4.0, 0.0))], Prim::Free);
    let smalls: Vec<u64> = (0..3).map(|k| h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([1.0 + k as f64 * 1.5, 9.0], 1.0, 1.0, 0.0))], Prim::Free)).collect();
    let mut all = vec![frame];
    all.extend(&smalls);
    h.state_mut().selected = all;
    h.state_mut().start_xform(Xform::Nest);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.part_in_part = true;
    }
    h.state_mut().apply_xform();
    h.run();
    let doc = &h.state().doc;
    let fr = doc.vectors.iter().find(|v| v.id == frame).unwrap();
    let (flo, fhi) = crate::geom::bbox(&fr.paths[1..]).unwrap();
    for id in &smalls {
        let v = doc.vectors.iter().find(|v| v.id == *id).unwrap();
        let (lo, hi) = crate::geom::bbox(&v.paths).unwrap();
        assert!(lo[0] >= flo[0] && lo[1] >= flo[1] && hi[0] <= fhi[0] && hi[1] <= fhi[1], "in the hole: {lo:?} {hi:?} vs {flo:?} {fhi:?}");
    }
    // Apart from each other.
    let boxes: Vec<_> = smalls.iter().map(|id| crate::geom::bbox(&doc.vectors.iter().find(|v| v.id == *id).unwrap().paths).unwrap()).collect();
    for i in 0..3 {
        for j in i + 1..3 {
            let (a, b) = (boxes[i], boxes[j]);
            assert!(a.1[0] <= b.0[0] || b.1[0] <= a.0[0] || a.1[1] <= b.0[1] || b.1[1] <= a.0[1], "overlap");
        }
    }
    assert!(h.state().status.contains('4'), "{}", h.state().status);
}

/// "Last vector is nest boundary": the parts go inside the last shape chosen, which stays put.
#[test]
fn nesting_fills_a_chosen_outline() {
    let mut h = harness();
    h.state_mut().new_file();
    let parts: Vec<u64> = (0..6).map(|k| h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([1.0 + k as f64 * 1.4, 9.3], 1.0, 1.0, 0.0))], Prim::Free)).collect();
    let circle = crate::geom::circle([5.0, 4.5], 3.5);
    let ring = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(circle.clone())], Prim::Free);
    let mut chosen = parts.clone();
    chosen.push(ring);
    h.state_mut().selected = chosen;
    h.state_mut().start_xform(Xform::Nest);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.into_boundary = true;
    }
    h.state_mut().apply_xform();
    h.run();
    let doc = &h.state().doc;
    assert_eq!(doc.vectors.iter().find(|v| v.id == ring).unwrap().paths[0].pts, circle, "the outline stays put");
    for id in &parts {
        let (lo, hi) = crate::geom::bbox(&doc.vectors.iter().find(|v| v.id == *id).unwrap().paths).unwrap();
        for c in [lo, hi, [lo[0], hi[1]], [hi[0], lo[1]]] {
            assert!((c[0] - 5.0).hypot(c[1] - 4.5) <= 3.5, "inside the circle: {c:?}");
        }
    }
    assert!(h.state().status.contains("outline"), "{}", h.state().status);
}

/// A turn step of 0 means no turning, as the original gives such a part a full turn.
#[test]
fn a_zero_turn_step_means_no_turning() {
    let mut h = harness();
    h.state_mut().new_file();
    h.state_mut().doc.job.height = 5.0;
    let id = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([3.0, 2.5], 1.0, 6.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![id];
    h.state_mut().start_xform(Xform::Nest);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.nest_turn = true;
        f.nest_step = 0.0;
    }
    h.state_mut().apply_xform();
    let (lo, hi) = crate::geom::bbox(&h.state().doc.vectors[0].paths).unwrap();
    assert!(hi[1] - lo[1] > hi[0] - lo[0], "still upright: {lo:?} {hi:?}");
}

/// Nesting by true shape: right triangles turn to fit into each other, so six fit on one sheet
/// where their boxes need two, and no two come closer than the tool's gap.
#[test]
fn nesting_by_shape_fits_triangles_together() {
    let make = |h: &mut Harness<'static, App>, by_shape: bool| {
        h.state_mut().new_file();
        let ids: Vec<u64> = (0..6).map(|k| {
            let (x, y) = (0.5 + k as f64 * 1.5, 9.0);
            h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(vec![[x, y - 4.0], [x + 4.0, y - 4.0], [x, y]])], Prim::Free)
        }).collect();
        h.state_mut().selected = ids;
        h.state_mut().start_xform(Xform::Nest);
        if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
            f.by_shape = by_shape;
            f.nest_turn = true;
        }
        h.state_mut().apply_xform();
    };
    let mut h = harness();
    make(&mut h, false);
    assert!(h.state().doc.sheet_count() > 1, "by their boxes they need two sheets");
    make(&mut h, true);
    let doc = &h.state().doc;
    assert_eq!(doc.sheet_count(), 1, "{}", h.state().status);
    let shapes: Vec<&crate::geom::Path> = doc.vectors.iter().map(|v| &v.paths[0]).collect();
    let near = |p: [f64; 2], path: &crate::geom::Path| path.segments().map(|(a, b)| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
    }).fold(f64::MAX, f64::min);
    for i in 0..shapes.len() {
        let (lo, hi) = crate::geom::bbox(std::slice::from_ref(shapes[i])).unwrap();
        assert!(lo[0] >= 0.25 - 1e-6 && lo[1] >= 0.25 - 1e-6 && hi[0] <= 9.75 + 1e-6 && hi[1] <= 9.75 + 1e-6, "inside the border: {lo:?} {hi:?}");
        for j in 0..shapes.len() {
            if i != j {
                let d = shapes[i].pts.iter().map(|p| near(*p, shapes[j])).fold(f64::MAX, f64::min);
                assert!(d >= 0.25 - 1e-6, "parts {i} and {j} are {d} apart");
                assert!(!crate::geom::inside_even_odd(std::iter::once(shapes[j]), shapes[i].pts[0]));
            }
        }
    }
}

/// Only closed outlines are nested; an open line stays where it is, and with nothing closed the
/// original's message says so.
#[test]
fn nesting_leaves_open_lines_alone() {
    let mut h = harness();
    h.state_mut().new_file();
    let line = h.state_mut().doc.add_vector(vec![crate::geom::Path::open(vec![[6.0, 6.0], [8.0, 7.0]])], Prim::Free);
    h.state_mut().selected = vec![line];
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    assert!(h.state().status.contains("No suitable parts"), "{}", h.state().status);
    let square = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 1.0, 1.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![line, square];
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    let doc = &h.state().doc;
    assert_eq!(doc.vectors.iter().find(|v| v.id == line).unwrap().paths[0].pts[0], [6.0, 6.0], "the line didn't move");
    let (lo, _) = crate::geom::bbox(&doc.vectors.iter().find(|v| v.id == square).unwrap().paths).unwrap();
    assert!(lo[0] < 1.0 && lo[1] < 1.0, "the square was nested: {lo:?}");
}

/// Copies per part: a square asked for three times is nested three times; nesting from another
/// sheet than the first, or into an outline with an open line, is refused.
#[test]
fn nesting_makes_the_copies_each_part_asks_for() {
    let mut h = harness();
    h.state_mut().new_file();
    let square = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 2.0, 2.0, 0.0))], Prim::Free);
    h.state_mut().doc.nest_copies.insert(square, 3);
    h.state_mut().selected = vec![square];
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    let doc = &h.state().doc;
    assert_eq!(doc.vectors.len(), 3, "{}", h.state().status);
    assert!(doc.nest_copies.is_empty(), "the count goes back to one");
    let boxes: Vec<_> = doc.vectors.iter().map(|v| crate::geom::bbox(&v.paths).unwrap()).collect();
    for i in 0..3 {
        for j in i + 1..3 {
            let (a, b) = (boxes[i], boxes[j]);
            assert!(a.1[0] <= b.0[0] || b.1[0] <= a.0[0] || a.1[1] <= b.0[1] || b.1[1] <= a.0[1], "copies overlap");
        }
    }
    // From the second sheet: refused.
    h.state_mut().add_sheet();
    let other = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 1.0, 1.0, 0.0))], Prim::Free);
    h.state_mut().selected = vec![other];
    h.state_mut().start_xform(Xform::Nest);
    h.state_mut().apply_xform();
    assert!(h.state().status.contains("first sheet"), "{}", h.state().status);
    // An outline with an open line in it: refused.
    h.state_mut().show_sheet(0);
    let open_outline = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([5.0, 5.0], 8.0, 8.0, 0.0)), crate::geom::Path::open(vec![[1.0, 1.0], [2.0, 2.0]])], Prim::Free);
    h.state_mut().selected = vec![square, open_outline];
    h.state_mut().start_xform(Xform::Nest);
    if let crate::app::LeftForm::Xform(f) = &mut h.state_mut().left {
        f.into_boundary = true;
    }
    h.state_mut().apply_xform();
    assert!(h.state().status.contains("open lines"), "{}", h.state().status);
}

/// The copies box shows the count the chosen shapes share, and stays empty when they differ.
#[test]
fn the_copies_box_shows_a_shared_count_or_nothing() {
    let mut h = harness();
    h.state_mut().new_file();
    let a = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([3.0, 3.0], 1.0, 1.0, 0.0))], Prim::Free);
    let b = h.state_mut().doc.add_vector(vec![crate::geom::Path::closed(crate::geom::rect([6.0, 6.0], 1.0, 1.0, 0.0))], Prim::Free);
    h.state_mut().doc.nest_copies.insert(a, 7);
    h.state_mut().doc.nest_copies.insert(b, 9);
    h.state_mut().selected = vec![a, b];
    h.state_mut().start_xform(Xform::Nest);
    h.run();
    let values = |h: &Harness<'static, App>| h.get_all_by_role(egui::accesskit::Role::TextInput).filter_map(|n| n.value()).collect::<Vec<String>>();
    let shown = values(&h);
    assert!(!shown.iter().any(|v| v == "7" || v == "9"), "{shown:?}");
    h.state_mut().doc.nest_copies.insert(b, 7);
    h.run();
    assert!(values(&h).iter().any(|v| v == "7"), "{:?}", values(&h));
}

/// Two workspaces, as Affinity's personas: Design shows the drawing panel alone, Machining the
/// toolpath panel alone. The buttons switch, and opening something of the other side goes there.
#[test]
fn design_and_machining_workspaces_switch() {
    let mut h = harness();
    h.state_mut().persona = crate::app::Persona::Design;
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    let shown = |h: &Harness<'static, App>, label: &str| h.query_by_label(label).is_some();
    assert!(shown(&h, "Draw circle") || shown(&h, "Shapes"), "the drawing panel");
    assert!(!shown(&h, "Pocket toolpath"), "no toolpath panel in Design");
    click(&mut h, "Machining");
    assert_eq!(h.state().persona, crate::app::Persona::Machining);
    assert!(shown(&h, "Pocket toolpath"), "the toolpath panel");
    assert!(!shown(&h, "Shapes"), "no drawing panel in Machining");
    // A toolpath form stays in Machining; back to Design with its button.
    click(&mut h, "Pocket toolpath");
    assert!(matches!(h.state().right, RightMode::Op { .. }));
    assert_eq!(h.state().persona, crate::app::Persona::Machining);
    click(&mut h, "Design");
    assert_eq!(h.state().persona, crate::app::Persona::Design, "the button wins over the open form");
    h.run();
    assert_eq!(h.state().persona, crate::app::Persona::Design, "and stays");
    // Opening the preview goes to Machining by itself.
    h.state_mut().right = RightMode::Preview;
    h.run();
    assert_eq!(h.state().persona, crate::app::Persona::Machining);
    // Opening the job's setup goes back to Design.
    h.state_mut().open_job_setup();
    h.run();
    assert_eq!(h.state().persona, crate::app::Persona::Design);
}

#[test]
fn the_version_shows_on_the_start_screen_and_in_about() {
    let version = env!("CARGO_PKG_VERSION");
    let mut h = harness();
    h.run();
    h.get_by_label_contains(&format!("Version {version}"));
    h.state_mut().about_open = true;
    h.run();
    h.get_by_label_contains(&format!("Burin {version} (OpenGL)"));
    h.get_by_label_contains("GNU GPL");
}

#[test]
fn another_job_opened_in_the_same_window_is_fitted_to_it() {
    let mut h = harness();
    h.state_mut().new_file();
    h.state_mut().left = LeftForm::None;
    h.run();
    let small = h.state().scale;
    // A job ten times as wide, as another project opened over this one.
    let mut doc = h.state().doc.clone();
    doc.job.width *= 10.0;
    doc.job.height *= 10.0;
    h.state_mut().doc = doc;
    h.state_mut().reset_state();
    h.state_mut().screen = Screen::Work;
    h.run();
    let big = h.state().scale;
    assert!(big < small * 0.2, "zoomed out to the new job: {small} then {big}");
}

#[test]
fn the_command_search_finds_and_does_a_command() {
    let mut h = harness();
    click(&mut h, "Start a new project");
    click(&mut h, "OK");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
    h.run();
    assert!(h.state().palette_open, "Ctrl+K opens it");
    h.state_mut().palette_query = "circle".into();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.run();
    assert!(!h.state().palette_open, "closed once done");
    assert!(matches!(h.state().left, LeftForm::Draw(_)), "the circle form is open: {:?}", std::mem::discriminant(&h.state().left));
}
