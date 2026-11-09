//! Working toolpaths out away from the window, with a progress bar and a way to stop.
//!
//! The window stays alive while this runs, however slow the computer. Nothing can be changed
//! meanwhile, since the bar covers the program, so results always fit the drawing they came from.

use crate::app::*;
use crate::toolpath::{self, JobSize, Move, Op};
use egui::{vec2, Color32, Id, RichText};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use crate::platform::Instant;
use std::time::Duration;

/// One toolpath to work out: its place in the list, and its settings.
pub type Task = (usize, Op);
type Outcome = (usize, Op, Result<Vec<Move>, String>);

/// What the results are for.
pub enum Finish {
    /// The Recalculate button, and how many toolpaths it left as they were.
    All { unchanged: usize, no_shapes: usize, warn: bool },
    /// The Calculate button of a toolpath form.
    Calculate { form: Box<Op>, editing: Option<usize>, tab: usize },
}

pub enum Message {
    Starting(String),
    Done(Box<Outcome>),
}

pub struct Running {
    pub total: usize,
    pub results: Vec<Outcome>,
    pub current: String,
    pub messages: Receiver<Message>,
    pub stop: Arc<AtomicBool>,
    pub started: Instant,
    pub finish: Option<Finish>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The bar only appears for work long enough to need it, so short jobs don't flash.
const SHOW_AFTER: Duration = Duration::from_millis(250);

impl App {
    /// Starts the work, here and now or away from the window. Returns how many were finished now.
    pub(crate) fn run_tasks(&mut self, tasks: Vec<Task>, finish: Finish, background: bool) -> usize {
        let (machine, job) = (self.doc.machine_on_job(), JobSize { width: self.doc.job.width, height: self.doc.job.height, thickness: self.doc.job.thickness });
        let needs_model = tasks.iter().any(|(_, op)| op.kind.uses_model() || op.project);
        if !background || tasks.is_empty() {
            let mut results = Vec::with_capacity(tasks.len());
            for (at, op) in tasks {
                let made = toolpath::generate(&op, machine, &job, self.model.as_ref());
                results.push((at, op, made));
            }
            return self.finish_tasks(results, finish, false);
        }
        let (send, messages) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let total = tasks.len();
        let model = if needs_model { self.model.clone() } else { None };
        let stopped = stop.clone();
        let names = tasks.first().map_or(String::new(), |(_, op)| op.name.clone());
        crate::platform::spawn(move || {
            for (at, op) in tasks {
                if stopped.load(Ordering::Relaxed) || send.send(Message::Starting(op.name.clone())).is_err() {
                    return;
                }
                let made = toolpath::generate(&op, machine, &job, model.as_ref());
                if send.send(Message::Done(Box::new((at, op, made)))).is_err() {
                    return;
                }
            }
        });
        self.recalc = Some(Running { total, results: Vec::with_capacity(total), current: names, messages, stop, started: Instant::now(), finish: Some(finish) });
        0
    }

    /// Takes in what has been finished since the last frame.
    pub(crate) fn poll_recalc(&mut self) {
        let Some(running) = &mut self.recalc else { return };
        loop {
            match running.messages.try_recv() {
                Ok(Message::Starting(name)) => running.current = name,
                Ok(Message::Done(outcome)) => running.results.push(*outcome),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let results = std::mem::take(&mut running.results);
                    let finish = running.finish.take();
                    let cut_short = results.len() < running.total;
                    self.recalc = None;
                    if let Some(finish) = finish {
                        self.finish_tasks(results, finish, cut_short);
                    }
                    return;
                }
            }
        }
    }

    pub(crate) fn stop_recalc(&mut self) {
        let Some(mut running) = self.recalc.take() else { return };
        let results = std::mem::take(&mut running.results);
        if let Some(finish) = running.finish.take() {
            self.finish_tasks(results, finish, true);
        }
    }

    fn finish_tasks(&mut self, results: Vec<Outcome>, finish: Finish, stopped: bool) -> usize {
        match finish {
            Finish::All { unchanged, no_shapes, warn } => {
                let tried = results.len();
                let done = self.keep_results(results);
                let mut told = tf!("Recalculated {done} toolpath(s).");
                if unchanged > 0 {
                    told += &tf!(" {unchanged} from the project file kept as saved: their shapes have not changed.");
                }
                if no_shapes > 0 {
                    told += &tf!(" {no_shapes} can't follow the drawing: a shape it was made from has gone, or it is a V-carve. Edit it to choose its shapes.");
                }
                if tried > done {
                    told += &tf!(" {} could not be recalculated.", tried - done);
                }
                if stopped {
                    told = tf!("Stopped after {done} toolpath(s). The others still have their old moves: Recalculate finishes them.");
                }
                self.status = told;
                if !self.doc.toolpaths.is_empty() {
                    self.reset_preview();
                }
                // Even those kept as saved: this is the moment the depths get looked at again.
                if warn {
                    let all: Vec<usize> = (0..self.doc.toolpaths.len()).collect();
                    self.warn_if_too_deep(&all);
                }
                done
            }
            Finish::Calculate { form, editing, tab } => {
                if stopped {
                    self.status = crate::i18n::tr("Stopped. The toolpath wasn't changed.").into();
                    return 0;
                }
                let mut made = Vec::with_capacity(results.len());
                for (_, op, result) in results {
                    match result {
                        Ok(moves) => made.push((op, Arc::new(moves))),
                        Err(error) => {
                            self.right = RightMode::Op { op: form, editing, tab, error: Some(error) };
                            return 0;
                        }
                    }
                }
                // Asked before calculating when the settings gave the depth; else its moves tell only now.
                let asked = toolpath::deepest_cut(&form) > self.doc.job.deepest_allowed() + 1e-9;
                let (first, count) = (editing.unwrap_or(self.doc.toolpaths.len()), made.len());
                self.keep_calculated(made, editing);
                if !asked {
                    self.warn_if_too_deep(&(first..first + count).collect::<Vec<_>>());
                }
                1
            }
        }
    }

    /// Puts new moves on the toolpaths, and the settings they were made with.
    fn keep_results(&mut self, results: Vec<Outcome>) -> usize {
        let mut done = 0;
        for (at, op, result) in results {
            match result {
                Ok(moves) => {
                    let Some(tp) = self.doc.toolpaths.get_mut(at) else { continue };
                    tp.op = op;
                    tp.moves = Arc::new(moves);
                    tp.kept_moves = None;
                    tp.kept_steps = None;
                    done += 1;
                }
                Err(_) => self.status = tf!("{} no longer fits its shapes. Check its tool.", op.name),
            }
        }
        // What was carved in the preview was carved with the old moves.
        if done > 0 {
            self.reset_preview();
        }
        done
    }

    /// The bar, over a program that can't be touched until it is done or stopped.
    pub(crate) fn recalc_window(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.recalc else { return };
        ctx.request_repaint_after(Duration::from_millis(40));
        // Even before the bar shows, clicks must not reach what is being worked on.
        let waited = running.started.elapsed();
        if waited < SHOW_AFTER {
            egui::Modal::new(Id::NULL.with("recalc-wait")).backdrop_color(Color32::TRANSPARENT).frame(egui::Frame::NONE).show(ctx, |ui| {
                ui.allocate_space(vec2(1.0, 1.0));
            });
            return;
        }
        let (done, total, name) = (running.results.len(), running.total, running.current.clone());
        let mut stop = false;
        egui::Modal::new(Id::NULL.with("recalc")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(crate::i18n::tr("Calculating toolpaths")).strong());
            ui.add_space(6.0);
            let text = tf!("Toolpath {} of {total}: {name}", (done + 1).min(total));
            ui.add(egui::ProgressBar::new(done as f32 / total.max(1) as f32).animate(true).desired_width(340.0));
            ui.add(egui::Label::new(text).truncate());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} s", waited.as_secs())).size(11.5).color(crate::theme::MUTED()));
                ui.add_space((ui.available_width() - 66.0).max(0.0));
                stop = ui.button(crate::i18n::tr("Stop")).clicked();
            });
        });
        if stop {
            self.stop_recalc();
        }
    }
}
