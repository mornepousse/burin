//! The monkeys: tests that do random things to Burin and check that it never falls over.
//!
//! - The file monkey takes good files of every kind Burin opens (projects, drawings, circuit boards,
//!   G-code, 3D models, post processors), damages them at random (bytes changed, cut, repeated,
//!   odd numbers put in) and opens them the way a dropped file is opened. Burin may refuse a file,
//!   but must not crash, hang, or keep shapes with impossible coordinates.
//! - The window monkey drives the whole program as a person would, at random: buttons, menus, boxes,
//!   keys, clicks and drags on the drawing, the wheel. After each action nothing may have crashed, the
//!   shapes must still be sound, and undoing then redoing must bring the same project back.
//!
//! Each run starts from a seed: the same seed does the very same things, so a failure printed with its
//! seed and its last actions can be played again. A short run is part of the tests; for a long one:
//!   BURIN_MONKEY_ROUNDS=2000 BURIN_MONKEY_STEPS=5000 BURIN_MONKEY_SEED=7 cargo test --release monkey -- --nocapture
//!
//! Tests never open the computer's file windows, never start other programs and keep their settings
//! in a folder of their own, so the monkeys can press anything.

use crate::app::App;
use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use std::time::{Duration, Instant};

/// A small, fast random source (xorshift*), the same everywhere for the same seed.
struct Dice(u64);

impl Dice {
    fn new(seed: u64) -> Self {
        Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn setting(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Every point of every shape is a real number of a sane size.
fn shapes_are_sound(app: &App) -> Result<(), String> {
    for v in &app.doc.vectors {
        for p in v.paths.iter().flat_map(|p| p.pts.iter()) {
            if !p[0].is_finite() || !p[1].is_finite() || p[0].abs() > 1e12 || p[1].abs() > 1e12 {
                return Err(format!("shape {} has the point {p:?}", v.id));
            }
        }
    }
    Ok(())
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "a panic".into())
}

// ---------------------------------------------------------------------------------------------
// The file monkey

/// Odd things to put in a file.
const ODD: &[&[u8]] = &[
    b"-", b"0", b"-0", b"1e308", b"-1e308", b"1e-308", b"NaN", b"inf", b"99999999999999999999", b"4294967295", b"-2147483648",
    b"\n", b"\r\n", b"\0", b" ", b",", b".", b"..", b"(", b")", b"<", b">", b"/>", b"%", b"*", b"\"", b"'", b"{", b"}", b"[", b"]",
    b"0\n", b"\n0\n", b"G2", b"G3", b"G36*", b"G37*", b"D03*", b"M48\n", b"T1C0\n", b"stream", b"endstream", b"obj", b"endobj",
    b"\xff\xff\xff\xff", b"\x00\x00\x00\x00", b"\xff\xfe", b"\x80", b"\xe2\x82",
];

fn damaged(good: &[u8], dice: &mut Dice) -> Vec<u8> {
    let mut b = good.to_vec();
    for _ in 0..1 + dice.below(4) {
        let n = b.len();
        match dice.below(7) {
            0 if n > 0 => {
                let i = dice.below(n);
                b[i] = dice.next() as u8;
            }
            1 if n > 0 => {
                let i = dice.below(n);
                let len = 1 + dice.below(64.min(n - i));
                b.drain(i..i + len);
            }
            2 if n > 0 => {
                let i = dice.below(n);
                let len = 1 + dice.below(256.min(n - i));
                let part: Vec<u8> = b[i..i + len].to_vec();
                let at = dice.below(n + 1);
                b.splice(at..at, part.iter().copied().cycle().take(len * (1 + dice.below(4))));
            }
            3 => {
                let at = dice.below(n + 1);
                b.splice(at..at, dice.pick(ODD).iter().copied());
            }
            4 if n > 0 => b.truncate(dice.below(n)),
            5 if n > 0 => {
                // A digit made another: numbers that still look like numbers.
                let digits: Vec<usize> = (0..n).filter(|&i| b[i].is_ascii_digit()).collect();
                if !digits.is_empty() {
                    let i = *dice.pick(&digits);
                    b[i] = b'0' + dice.below(10) as u8;
                }
            }
            _ if n > 0 => {
                // A sign flipped or a minus put before a number.
                let i = dice.below(n);
                if b[i] == b'-' {
                    b.remove(i);
                } else {
                    b.insert(i, b'-');
                }
            }
            _ => {}
        }
    }
    b
}

/// Good files of every kind, by their file ending.
fn good_files() -> Vec<(&'static str, Vec<u8>)> {
    use crate::crv3d::tests::{material, sample, sample_compound};
    let mut app = App::default();
    app.load_sample();
    let paths: Vec<crate::geom::Path> = app.doc.vectors.iter().flat_map(|v| v.paths.iter().cloned()).collect();
    let (w, h) = (app.doc.job.width, app.doc.job.height);
    let mut files: Vec<(&'static str, Vec<u8>)> = vec![
        ("crv3d", sample_compound(&material([0.0, -200.0, -13.0], [300.0, 0.0, 0.0], 3), &sample())),
        ("burin", serde_json::to_vec(&app.doc).unwrap()),
        ("dxf", crate::geom::dxf_write(&paths).into_bytes()),
        ("svg", crate::menus::svg_text(&paths, w, h, "in").into_bytes()),
        ("pdf", crate::menus::pdf_bytes(&paths, w, h, 72.0)),
        ("eps", crate::menus::eps_text(&paths, w, h, 72.0).into_bytes()),
        ("ai", crate::menus::ai_text(&paths, 72.0).into_bytes()),
        ("drl", b"M48\nMETRIC,TZ\nT1C0.800\nT2C3.0\n%\nG90\nG05\nT1\nX010000Y010000\nX020000Y010000\nT2\nX5.0Y5.0\nX1.0Y1.0G85X6.0Y1.0\nG00X0Y0\nM15\nG01X5Y5\nM16\nM30\n".to_vec()),
        ("gbr", b"%FSLAX46Y46*%\n%MOMM*%\n%AMRoundRect*\n0 comment*\n4,1,4,$2,$3,$4,$5,$6,$7,$8,$9,$2,$3,0*\n1,1,$1+$1,$2,$3*\n20,1,$1+$1,$2,$3,$4,$5,0*\n21,1,1,2,0,0,45*\n5,1,6,0,0,2,0*\n7,0,0,2,1,0.2,0*%\n%ADD10C,0.5*%\n%ADD11R,2X1*%\n%ADD12O,2X1*%\n%ADD13P,2X5X0*%\n%ADD14RoundRect,0.25X-0.75X-0.5X0.75X-0.5X0.75X0.5X-0.75X0.5X0*%\nD10*\nX0Y0D02*\nX10000000Y0D01*\nG75*\nG03*\nX0Y10000000I-5000000J5000000D01*\nG01*\nD11*\nX3000000Y3000000D03*\nD12*\nX5000000Y3000000D03*\nD13*\nX7000000Y3000000D03*\nD14*\nX9000000Y3000000D03*\nG36*\nX0Y0D02*\nX2000000Y0D01*\nX2000000Y2000000D01*\nX0Y0D01*\nG37*\n%LPC*%\nX1000000Y1000000D03*\n%LPD*%\nM02*\n".to_vec()),
        ("gko", b"%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.1*%\nD10*\nX0Y0D02*\nX50000000Y0D01*\nX50000000Y30000000D01*\nG74*\nG02*\nX0Y30000000I25000000J0D01*\nX0Y0D01*\nM02*\n".to_vec()),
        ("nc", b"%\n(test)\nG21 G90 G17\nG0 Z5\nG0 X0 Y0\nM3 S12000\nG1 Z-1 F100\nG1 X10 Y0 F800\nG2 X20 Y10 I0 J10\nG3 X10 Y20 R10\nG1 X0 Y0 Z-2\nG0 Z5\nM5\nM30\n%\n".to_vec()),
        ("stl", b"solid c\nfacet normal 0 0 1\nouter loop\nvertex 0 0 1\nvertex 10 0 1\nvertex 0 10 1\nendloop\nendfacet\nfacet normal 0 0 1\nouter loop\nvertex 10 0 2\nvertex 10 10 2\nvertex 0 10 1\nendloop\nendfacet\nendsolid c\n".to_vec()),
        ("pp", b"POST_NAME = \"Router (mm) (*.tap)\"\nFILE_EXTENSION = \"tap\"\nUNITS = \"MM\"\nVAR X_POSITION = [X|C|X|1.1]\nVAR Y_POSITION = [Y|C|Y|1.1]\nVAR Z_POSITION = [Z|C|Z|1.1]\nbegin HEADER\n\"(file [TP_FILENAME])\"\nbegin RAPID_MOVE\n\"G0 [X][Y][Z]\"\nbegin FEED_MOVE\n\"G1 [X][Y][Z]\"\nbegin FOOTER\n\"END\"\n".to_vec()),
    ];
    // A binary STL, two triangles.
    let mut stl = vec![0u8; 80];
    stl.extend_from_slice(&2u32.to_le_bytes());
    for tri in [[[0.0f32, 0.0, 0.0], [5.0, 0.0, 1.0], [0.0, 5.0, 2.0]], [[5.0, 0.0, 1.0], [5.0, 5.0, 3.0], [0.0, 5.0, 2.0]]] {
        stl.extend_from_slice(&[0u8; 12]);
        for v in tri {
            for c in v {
                stl.extend_from_slice(&c.to_le_bytes());
            }
        }
        stl.extend_from_slice(&[0u8; 2]);
    }
    files.push(("stl", stl));
    // The test files kept on this computer, when they are there.
    let local = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("essais");
    for dir in [local.clone(), local.join("pcb")] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut found: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        found.sort();
        for path in found {
            let ending = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let known = ["crv3d", "dxf", "svg", "drl", "gbr", "gko", "gtl"].iter().find(|k| **k == ending);
            if let (Some(k), Ok(bytes)) = (known, std::fs::read(&path)) {
                if bytes.len() <= 2_000_000 {
                    files.push((k, bytes));
                }
            }
        }
    }
    files
}

/// Opens a file as a dropped file is opened, in a program of its own, and says what went wrong.
fn open_damaged(ending: &str, bytes: Vec<u8>, dir: &std::path::Path, n: usize) -> Result<(), String> {
    let path = dir.join(format!("case-{n}.{ending}"));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let ending = ending.to_string();
    let (send, receive) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut app = App::default();
            match ending.as_str() {
                "crv3d" => {
                    let _ = app.open_project(&path);
                }
                "burin" => {
                    let _ = app.open_path(path.clone());
                }
                _ => app.open_picked(path.clone()),
            }
            shapes_are_sound(&app)
        }));
        let _ = send.send(match outcome {
            Ok(sound) => sound,
            Err(e) => Err(format!("crashed: {}", panic_text(e.as_ref()))),
        });
    });
    match receive.recv_timeout(Duration::from_secs(20)) {
        Ok(result) => {
            let _ = thread.join();
            result
        }
        Err(_) => Err("still busy after 20 s (hangs?)".into()),
    }
}

#[test]
fn monkey_damaged_files_never_crash_burin() {
    let rounds = setting("BURIN_MONKEY_ROUNDS", 12) as usize;
    let seed = setting("BURIN_MONKEY_SEED", 1);
    let dir = std::env::temp_dir().join(format!("burin-monkey-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The panics are caught and reported below; their messages would only clutter the output.
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut failures = Vec::new();
    let mut n = 0;
    for (k, (ending, good)) in good_files().into_iter().enumerate() {
        let mut dice = Dice::new(seed * 1000 + k as u64);
        // The good file itself first: it must open.
        for round in 0..=rounds {
            let bytes = if round == 0 { good.clone() } else { damaged(&good, &mut dice) };
            n += 1;
            if let Err(why) = open_damaged(ending, bytes.clone(), &dir, n) {
                let kept = dir.join(format!("failed-{n}.{ending}"));
                let _ = std::fs::write(&kept, &bytes);
                failures.push(format!("{ending} (seed {seed}, file {k}, round {round}): {why} — kept as {}", kept.display()));
                if failures.len() >= 20 {
                    break;
                }
            }
        }
    }
    std::panic::set_hook(quiet);
    if failures.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(failures.is_empty(), "{} of {n} damaged files went wrong:\n{}", failures.len(), failures.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// The window monkey

/// What the monkey must not press: it would leave the test (other programs) or only wait.
const NOT_PRESSED: &[&str] = &["Open in SolidCraft...", "Add to Applications Menu", "Remove from Applications Menu"];

const PRESSED_ROLES: &[Role] = &[Role::Button, Role::CheckBox, Role::RadioButton, Role::ComboBox, Role::MenuItem, Role::MenuItemCheckBox, Role::MenuItemRadio, Role::Tab, Role::Slider];

const KEYS: &[Key] = &[
    Key::Escape, Key::Enter, Key::Delete, Key::Backspace, Key::Tab, Key::Space, Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown,
    Key::Z, Key::Y, Key::C, Key::V, Key::X, Key::A, Key::G, Key::U, Key::D, Key::H, Key::M, Key::R, Key::S, Key::N, Key::Num0, Key::Num1, Key::Num5,
    Key::Plus, Key::Minus, Key::Equals, Key::F1, Key::F2, Key::F3, Key::F5, Key::F9, Key::F11, Key::Home, Key::End, Key::PageUp, Key::PageDown,
];

const TYPED: &[&str] = &["0", "1", "2.5", "-3", "8*3", "100", "1e9", "-0", "abc", "", "1/0", "(2+3)*4", "999999999", "0.0001", "12,5", "3'", "π", "  7  ", "x", "-1e-9"];

fn window_harness() -> Harness<'static, App> {
    let h = Harness::builder().with_size(vec2(1600.0, 960.0)).with_accessibility_check(false).build_ui_state(|ui, app: &mut App| app.frame(ui), App::default());
    crate::ui::setup_style(&h.ctx);
    h
}

fn pointer_click(h: &Harness<'static, App>, at: Pos2, button: PointerButton, modifiers: Modifiers) {
    h.event(Event::PointerMoved(at));
    h.event(Event::PointerButton { pos: at, button, pressed: true, modifiers });
    h.event(Event::PointerButton { pos: at, button, pressed: false, modifiers });
}

/// The controls on screen now that the monkey may press: their role, name and place.
fn controls(h: &Harness<'static, App>) -> Vec<(Role, String, egui::Rect)> {
    let mut out = Vec::new();
    for node in h.query_all_by(|n| PRESSED_ROLES.contains(&n.role()) && !n.is_disabled() && n.bounding_box().is_some()) {
        let label = node.accesskit_node().label().unwrap_or_default();
        if NOT_PRESSED.contains(&label.as_str()) {
            continue;
        }
        let rect = node.rect();
        if rect.width() > 0.0 && rect.height() > 0.0 && rect.min.x < 1600.0 && rect.min.y < 960.0 {
            out.push((node.accesskit_node().role(), label, rect));
        }
    }
    out
}

/// One random action, and what it was (for the report).
fn act(h: &mut Harness<'static, App>, dice: &mut Dice) -> String {
    let drawing = h.state().last_rect;
    let drawing = if drawing.is_positive() && drawing.is_finite() { drawing } else { egui::Rect::from_min_size(Pos2::new(400.0, 100.0), vec2(900.0, 700.0)) };
    let spot = |dice: &mut Dice| Pos2::new(drawing.min.x + dice.unit() * drawing.width(), drawing.min.y + dice.unit() * drawing.height());
    let roll = dice.below(100);
    if roll < 42 {
        let found = controls(h);
        if found.is_empty() {
            return "nothing to press".into();
        }
        let (role, label, rect) = dice.pick(&found).clone();
        pointer_click(h, rect.center(), PointerButton::Primary, Modifiers::NONE);
        return format!("press {role:?} \"{label}\"");
    }
    if roll < 56 {
        let (from, to) = (spot(dice), spot(dice));
        let modifiers = *dice.pick(&[Modifiers::NONE, Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND, Modifiers::ALT]);
        if dice.below(2) == 0 {
            pointer_click(h, from, PointerButton::Primary, modifiers);
            return format!("click the drawing at {from:?} {modifiers:?}");
        }
        h.event(Event::PointerMoved(from));
        h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers });
        h.step();
        for k in 1..=4 {
            h.event(Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
            h.step();
        }
        h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers });
        return format!("drag {from:?} to {to:?} {modifiers:?}");
    }
    if roll < 68 {
        let key = *dice.pick(KEYS);
        let modifiers = *dice.pick(&[Modifiers::NONE, Modifiers::NONE, Modifiers::COMMAND, Modifiers::SHIFT, Modifiers::ALT, Modifiers::COMMAND | Modifiers::SHIFT]);
        h.key_press_modifiers(modifiers, key);
        return format!("key {modifiers:?} {key:?}");
    }
    if roll < 82 {
        let boxes: Vec<egui::Rect> = h.query_all_by_role(Role::TextInput).map(|n| n.rect()).filter(|r| r.min.x < 1600.0 && r.min.y < 960.0).collect();
        if boxes.is_empty() {
            return "no box to type in".into();
        }
        let rect = *dice.pick(&boxes);
        let text = *dice.pick(TYPED);
        pointer_click(h, rect.center(), PointerButton::Primary, Modifiers::NONE);
        h.step();
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.event(Event::Text(text.to_string()));
        if dice.below(2) == 0 {
            h.key_press(Key::Enter);
        }
        return format!("type \"{text}\" at {:?}", rect.center());
    }
    if roll < 90 {
        let at = spot(dice);
        let delta = vec2(0.0, if dice.below(2) == 0 { 120.0 } else { -120.0 });
        h.event(Event::PointerMoved(at));
        h.event(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, modifiers: *dice.pick(&[Modifiers::NONE, Modifiers::COMMAND, Modifiers::SHIFT]), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Unknown });
        return format!("wheel {delta:?} at {at:?}");
    }
    if roll < 95 {
        let at = spot(dice);
        pointer_click(h, at, PointerButton::Secondary, Modifiers::NONE);
        return format!("right click at {at:?}");
    }
    // Undo then redo must give the same project back.
    let before = serde_json::to_string(&h.state().doc).unwrap();
    h.state_mut().do_undo();
    h.state_mut().do_redo();
    let after = serde_json::to_string(&h.state().doc).unwrap();
    assert!(before == after, "undo then redo changed the project ({} bytes before, {} after)", before.len(), after.len());
    "undo, redo".into()
}

fn window_run(seed: u64, steps: usize) {
    let mut dice = Dice::new(seed);
    let mut h = window_harness();
    // Half the runs on the sample project with its toolpaths, half on a new empty job.
    if seed % 2 == 0 {
        h.state_mut().load_sample();
    } else {
        h.get_by_label("Start a new project").click();
        h.run();
        h.get_by_label("OK").click();
    }
    h.run_steps(3);
    let mut log: Vec<String> = Vec::new();
    // BURIN_MONKEY_TRACE=1 says each action as it is done, to find one that never ends.
    let trace = std::env::var_os("BURIN_MONKEY_TRACE").is_some();
    for step in 0..steps {
        let started = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let did = act(&mut h, &mut dice);
            if trace {
                eprintln!("{step}: {did}");
            }
            log.push(did);
            h.run_steps(3);
            shapes_are_sound(h.state())
        }));
        let problem = match outcome {
            Ok(Ok(())) if started.elapsed() < Duration::from_secs(60) => None,
            Ok(Ok(())) => Some(format!("one action took {:?}", started.elapsed())),
            Ok(Err(why)) => Some(why),
            Err(e) => Some(format!("crashed: {}", panic_text(e.as_ref()))),
        };
        if let Some(why) = problem {
            let last: Vec<String> = log.iter().rev().take(25).rev().cloned().collect();
            panic!("window monkey, seed {seed}, step {step}: {why}\nlast actions:\n  {}", last.join("\n  "));
        }
    }
}

#[test]
fn monkey_random_use_of_the_window_never_crashes_burin() {
    let steps = setting("BURIN_MONKEY_STEPS", 250) as usize;
    let first = setting("BURIN_MONKEY_SEED", 1);
    for seed in first..first + 2 {
        window_run(seed, steps);
    }
}

/// Opens one file the way the file monkey does, to look into a failure it kept:
/// BURIN_PROBE=/tmp/x/failed-12.svg cargo test --release probe_a_kept_file -- --ignored --nocapture
#[test]
#[ignore]
fn probe_a_kept_file() {
    let Ok(file) = std::env::var("BURIN_PROBE") else { return };
    let path = std::path::PathBuf::from(&file);
    let mut app = App::default();
    let started = Instant::now();
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "crv3d" => println!("{:?}", app.open_project(&path).err()),
        "burin" => println!("{:?}", app.open_path(path.clone()).err()),
        _ => app.open_picked(path.clone()),
    }
    println!("{} shapes, {:?}, {:?} {:?}", app.doc.vectors.len(), started.elapsed(), app.message, app.status);
}
