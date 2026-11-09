//! The command search: Ctrl+K, a few letters of what's wanted, Enter. It finds the actions that
//! have keys, the kinds of toolpath and the two workspaces by name, in the language shown, and does
//! them as their key or button would. Burin's own; the original has nothing like it.

use crate::app::*;
use crate::i18n::tr;
use crate::keys::Action;
use crate::theme;
use crate::toolpath::OpKind;
use egui::{Align2, Key, RichText};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Act(Action),
    Toolpath(OpKind),
    Workspace(Persona),
    Preview,
    SaveToolpaths,
    RecalculateAll,
}

const KINDS: [OpKind; 12] = [
    OpKind::Profile,
    OpKind::Pocket,
    OpKind::Drill,
    OpKind::Engrave,
    OpKind::VCarve,
    OpKind::Rough3D,
    OpKind::Finish3D,
    OpKind::Prism,
    OpKind::Texture,
    OpKind::Moulding,
    OpKind::Flute,
    OpKind::Chamfer,
];

/// Every command with what it's called.
pub fn commands() -> Vec<(Command, String)> {
    let mut out: Vec<(Command, String)> = crate::keys::all_actions().map(|a| (Command::Act(a), crate::keys::action_name(a).to_string())).collect();
    out.extend(KINDS.iter().map(|k| (Command::Toolpath(*k), k.title().to_string())));
    out.push((Command::Workspace(Persona::Design), tr("Design").to_string()));
    out.push((Command::Workspace(Persona::Machining), tr("Machining").to_string()));
    out.push((Command::Preview, tr("Preview toolpaths").to_string()));
    out.push((Command::SaveToolpaths, tr("Save toolpaths").to_string()));
    out.push((Command::RecalculateAll, tr("Recalculate all toolpaths").to_string()));
    out
}

/// Letters without their accents and in small case, so "decaler" finds "Décaler".
fn plain(text: &str) -> String {
    text.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// The commands whose name holds every word typed, those starting with the first word first.
pub fn matching(query: &str) -> Vec<(Command, String)> {
    let words: Vec<String> = plain(query).split_whitespace().map(String::from).collect();
    let mut found: Vec<(bool, Command, String)> = commands()
        .into_iter()
        .filter(|(_, name)| {
            let name = plain(name);
            words.iter().all(|w| name.contains(w.as_str()))
        })
        .map(|(c, name)| (words.first().is_some_and(|w| plain(&name).starts_with(w.as_str())), c, name))
        .collect();
    found.sort_by_key(|f| !f.0);
    found.into_iter().map(|(_, c, n)| (c, n)).collect()
}

/// The most lines shown at once.
const SHOWN: usize = 10;

impl App {
    pub(crate) fn run_command(&mut self, command: Command) {
        match command {
            Command::Act(action) => self.forced_action = Some(action),
            Command::Toolpath(kind) => {
                self.set_persona(Persona::Machining);
                self.start_op(kind);
            }
            Command::Workspace(p) => self.set_persona(p),
            Command::Preview => {
                self.right = RightMode::Preview;
                self.right_open = true;
                self.view3d = true;
            }
            Command::SaveToolpaths => {
                self.right = RightMode::Save;
                self.right_open = true;
            }
            Command::RecalculateAll => self.recalculate_all(),
        }
    }

    pub(crate) fn palette_window(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Work && !self.palette_open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::K)) {
            self.palette_open = true;
            self.palette_query.clear();
            self.palette_pick = 0;
        }
        if !self.palette_open {
            return;
        }
        let found = matching(&self.palette_query);
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (i.consume_key(egui::Modifiers::NONE, Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape))
        });
        let last = found.len().min(SHOWN).saturating_sub(1);
        if down {
            self.palette_pick = (self.palette_pick + 1).min(last);
        }
        if up {
            self.palette_pick = self.palette_pick.saturating_sub(1);
        }
        self.palette_pick = self.palette_pick.min(last);
        let mut chosen = if enter { found.get(self.palette_pick).map(|f| f.0) } else { None };
        let keys = self.keys;
        let top = ctx.content_rect().top() + 70.0;
        egui::Window::new(tr("Find a command"))
            .id(egui::Id::unique("command-search"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size([460.0, 0.0])
            .anchor(Align2::CENTER_TOP, [0.0, top])
            .show(ctx, |ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut self.palette_query).hint_text(tr("Find a command ...")).desired_width(f32::INFINITY));
                field.request_focus();
                if field.changed() {
                    self.palette_pick = 0;
                }
                ui.add_space(4.0);
                if found.is_empty() {
                    ui.label(RichText::new(tr("Nothing has that name.")).color(theme::MUTED));
                }
                for (i, (command, name)) in found.iter().take(SHOWN).enumerate() {
                    let picked = i == self.palette_pick;
                    let line = ui.horizontal(|ui| {
                        ui.set_width(ui.available_width());
                        let text = if picked { RichText::new(name).color(theme::TEXT).strong() } else { RichText::new(name).color(theme::MUTED) };
                        let r = ui.selectable_label(picked, text);
                        if let Command::Act(a) = command {
                            let key = keys.text(*a);
                            if !key.is_empty() {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(key).size(11.5).color(theme::FAINT)));
                            }
                        }
                        r
                    });
                    if line.inner.clicked() {
                        chosen = Some(*command);
                    }
                }
                ui.add_space(2.0);
                ui.label(RichText::new(tr("Enter to do it, Escape to close.")).size(11.0).color(theme::FAINT));
            });
        if escape {
            self.palette_open = false;
        }
        if let Some(command) = chosen {
            self.palette_open = false;
            self.run_command(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_find_commands_whatever_their_accents() {
        let names: Vec<String> = matching("pocket").into_iter().map(|f| f.1).collect();
        assert!(names.iter().any(|n| n == "Pocket Toolpath"), "{names:?}");
        assert!(matching("draw circ").iter().any(|f| f.0 == Command::Act(Action::DrawCircle)));
        assert!(matching("zzqx").is_empty());
        assert_eq!(plain("Décaler Ç"), "decaler c");
        // What starts with the word comes first.
        assert_eq!(matching("save").first().map(|f| f.1.starts_with("Save")), Some(true));
    }
}
