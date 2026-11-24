//! the timers page: a countdown, a stopwatch and a pomodoro clock.
//!
//! The countdown and the pomodoro keep their lengths in the deck config, so a
//! chosen 25 minutes survives a restart; a running clock is only part of the
//! session. Everything is drawn in the Grid language: a big monospace time on
//! a hairline bar, hard-edged buttons and square round markers.

use crate::app::App;
use crate::theme::{self, Tokens};
use eframe::egui::{self, Align, Color32, FontFamily, Layout, RichText, Stroke, Ui, Vec2};
use sdw_core::model::TimerCfg;
use sdw_core::t;
use std::time::Instant;

// ---------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Countdown,
    Stopwatch,
    Pomodoro,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Work,
    Short,
    Long,
}

#[derive(Debug, Clone)]
pub struct Countdown {
    /// Length of the current run, in seconds; the bar is drawn against it.
    pub total: f64,
    pub remaining: f64,
    pub running: bool,
    /// The run ended and the banner has not been dismissed yet.
    pub fired: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Stopwatch {
    pub elapsed: f64,
    pub running: bool,
    /// Running totals at each lap, oldest first.
    pub laps: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct Pomodoro {
    pub phase: Phase,
    pub total: f64,
    pub remaining: f64,
    pub running: bool,
    /// Work phases finished in the current set.
    pub round: u32,
    /// The phase ended and the next has not been started yet.
    pub fired: bool,
}

pub struct Timers {
    pub tab: Tab,
    pub countdown: Countdown,
    pub stopwatch: Stopwatch,
    pub pomodoro: Pomodoro,
    /// Wall clock of the previous tick: a frame that arrives late must not
    /// stretch a timer, and a suspended app catches up when it returns.
    last: Instant,
}

impl Timers {
    pub fn new(cfg: &TimerCfg) -> Self {
        let work = cfg.work_secs.max(1) as f64;
        Self {
            tab: Tab::Countdown,
            countdown: Countdown {
                total: cfg.countdown_secs.max(1) as f64,
                remaining: cfg.countdown_secs.max(1) as f64,
                running: false,
                fired: false,
            },
            stopwatch: Stopwatch::default(),
            pomodoro: Pomodoro {
                phase: Phase::Work,
                total: work,
                remaining: work,
                running: false,
                round: 0,
                fired: false,
            },
            last: Instant::now(),
        }
    }

    /// Whether any clock is running, so the frame loop can keep a fine beat.
    pub fn running(&self) -> bool {
        self.countdown.running || self.stopwatch.running || self.pomodoro.running
    }

    /// The line a home tile shows while a clock runs: which clock and how much
    /// is left. `None` when everything is stopped, so the tile can stay dim.
    pub fn tile_status(&self, lang: sdw_core::Lang) -> Option<String> {
        if self.countdown.running {
            return Some(format!(
                "{} · {}",
                t!(lang, "отсчёт", "countdown"),
                clock_text(self.countdown.remaining, false)
            ));
        }
        if self.pomodoro.running {
            let phase = match self.pomodoro.phase {
                Phase::Work => t!(lang, "работа", "work"),
                Phase::Short => t!(lang, "перерыв", "break"),
                Phase::Long => t!(lang, "длинный перерыв", "long break"),
            };
            return Some(format!(
                "{phase} · {}",
                clock_text(self.pomodoro.remaining, false)
            ));
        }
        if self.stopwatch.running {
            return Some(format!(
                "{} · {}",
                t!(lang, "секундомер", "stopwatch"),
                clock_text(self.stopwatch.elapsed, true)
            ));
        }
        None
    }

    /// Advance every running clock to now. Returns `true` when a clock just
    /// ended and the caller should play a cue.
    pub fn tick(&mut self, cfg: &TimerCfg) -> bool {
        let now = Instant::now();
        // A laptop that slept for a day should simply find its timer finished,
        // not add a day to a stopwatch in one step.
        let dt = now
            .duration_since(self.last)
            .as_secs_f64()
            .clamp(0.0, 3600.0);
        self.last = now;
        let mut chime = false;
        if self.countdown.running {
            self.countdown.remaining -= dt;
            if self.countdown.remaining <= 0.0 {
                self.countdown.remaining = 0.0;
                self.countdown.running = false;
                self.countdown.fired = true;
                chime = true;
            }
        }
        if self.stopwatch.running {
            self.stopwatch.elapsed += dt;
        }
        if self.pomodoro.running {
            self.pomodoro.remaining -= dt;
            if self.pomodoro.remaining <= 0.0 {
                self.pomodoro.remaining = 0.0;
                chime = true;
                if cfg.auto_start {
                    self.pomodoro.next(cfg);
                    self.pomodoro.running = true;
                } else {
                    self.pomodoro.running = false;
                    self.pomodoro.fired = true;
                }
            }
        }
        chime
    }
}

impl Pomodoro {
    /// Move to the phase that follows the current one.
    fn next(&mut self, cfg: &TimerCfg) {
        match self.phase {
            Phase::Work => {
                self.round = self.round.saturating_add(1);
                if self.round >= cfg.rounds.max(1) {
                    self.phase = Phase::Long;
                    self.total = cfg.long_secs.max(1) as f64;
                } else {
                    self.phase = Phase::Short;
                    self.total = cfg.short_secs.max(1) as f64;
                }
            }
            Phase::Short => {
                self.phase = Phase::Work;
                self.total = cfg.work_secs.max(1) as f64;
            }
            Phase::Long => {
                self.phase = Phase::Work;
                self.round = 0;
                self.total = cfg.work_secs.max(1) as f64;
            }
        }
        self.remaining = self.total;
        self.fired = false;
    }

    fn reset(&mut self, cfg: &TimerCfg) {
        self.phase = Phase::Work;
        self.total = cfg.work_secs.max(1) as f64;
        self.remaining = self.total;
        self.round = 0;
        self.running = false;
        self.fired = false;
    }
}

// ---------------------------------------------------------------- helpers

/// `MM:SS`, or `H:MM:SS` past an hour. With `tenths` the stopwatch keeps a
/// tenth, which is what makes a lap time worth reading.
fn clock_text(secs: f64, tenths: bool) -> String {
    let total = secs.max(0.0);
    let h = (total / 3600.0) as u64;
    let m = ((total % 3600.0) / 60.0) as u64;
    let s = (total % 60.0) as u64;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else if tenths {
        format!("{m:02}:{s:02}.{}", ((total * 10.0) % 10.0) as u64)
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// A hard-edged button of a fixed width; `primary` paints it in the accent.
fn action(ui: &mut Ui, t: &Tokens, text: &str, primary: bool, width: f32) -> bool {
    ui.add(
        egui::Button::new(RichText::new(text).size(15.0).strong().color(if primary {
            Color32::WHITE
        } else {
            t.ink
        }))
        .fill(if primary { t.accent } else { t.card })
        .stroke(Stroke::new(
            1.0,
            if primary { t.accent } else { t.line_strong },
        ))
        .corner_radius(0.0)
        .min_size(Vec2::new(width.max(64.0), 44.0)),
    )
    .clicked()
}

/// A preset chip: minutes to set, drawn like the rest of the Grid surface.
fn preset(ui: &mut Ui, t: &Tokens, text: &str, active: bool) -> bool {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .family(FontFamily::Monospace)
                .size(13.0)
                .color(if active { Color32::WHITE } else { t.ink }),
        )
        .fill(if active { t.accent } else { t.card })
        .stroke(Stroke::new(
            1.0,
            if active { t.accent } else { t.line_strong },
        ))
        .corner_radius(0.0)
        .min_size(Vec2::new(58.0, 34.0)),
    )
    .clicked()
}

fn tab_button(ui: &mut Ui, t: &Tokens, label: &str, active: bool) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(14.5).strong().color(if active {
            Color32::WHITE
        } else {
            t.ink
        }))
        .fill(if active { t.accent } else { t.card })
        .stroke(Stroke::new(
            1.0,
            if active { t.accent } else { t.line_strong },
        ))
        .corner_radius(0.0)
        .min_size(Vec2::new(0.0, 42.0)),
    )
    .clicked()
}

/// A centred column so the clock does not stretch over a wide window.
fn column<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = ui.available_width().min(820.0);
    let margin = ((ui.available_width() - width) * 0.5).max(0.0);
    ui.horizontal(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_max_width(width);
            add(ui)
        })
        .inner
    })
    .inner
}

// ---------------------------------------------------------------- page

pub fn page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    ui.add_space(2.0);
    ui.label(
        RichText::new(t!(lang, "таймеры", "timers"))
            .size(28.0)
            .strong()
            .color(t.strong),
    );
    ui.label(
        RichText::new(t!(
            lang,
            "обратный отсчёт, секундомер и помодоро — с пресетами и звуком в конце.",
            "a countdown, a stopwatch and a pomodoro — with presets and a cue at the end."
        ))
        .size(13.5)
        .color(t.muted),
    );
    ui.add_space(10.0);

    let current = app.timers.tab;
    ui.horizontal(|ui| {
        if tab_button(
            ui,
            t,
            t!(lang, "отсчёт", "countdown"),
            current == Tab::Countdown,
        ) {
            app.timers.tab = Tab::Countdown;
        }
        if tab_button(
            ui,
            t,
            t!(lang, "секундомер", "stopwatch"),
            current == Tab::Stopwatch,
        ) {
            app.timers.tab = Tab::Stopwatch;
        }
        if tab_button(
            ui,
            t,
            t!(lang, "помодоро", "pomodoro"),
            current == Tab::Pomodoro,
        ) {
            app.timers.tab = Tab::Pomodoro;
        }
    });
    ui.add_space(10.0);

    match current {
        Tab::Countdown => countdown_page(app, ui, t),
        Tab::Stopwatch => stopwatch_page(app, ui, t),
        Tab::Pomodoro => pomodoro_page(app, ui, t),
    }

    // A running clock needs a fine beat even when the idle frame loop would
    // sleep for a third of a second.
    if app.timers.running() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn big_time(ui: &mut Ui, text: &str, color: Color32, size: f32) {
    ui.label(
        RichText::new(text)
            .family(FontFamily::Monospace)
            .size(size)
            .strong()
            .color(color),
    );
}

fn countdown_page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    // The length lives in the config, the run in the page state. They are kept
    // in separate locals so neither borrows the app while the other is used.
    let mut secs = app.cfg.timers.countdown_secs;
    let mut dirty = false;
    let length = secs.max(1) as f64;
    let cd = &mut app.timers.countdown;
    // Only an untouched clock follows the chosen length. A clock paused in the
    // middle keeps its own remaining time, even after a trip to another page.
    let untouched = (cd.remaining - cd.total).abs() < 0.01;
    if !cd.running && !cd.fired && untouched && (cd.total - length).abs() > 0.01 {
        cd.total = length;
        cd.remaining = length;
    }

    column(ui, |ui| {
        theme::card(ui, t, |ui| {
            ui.vertical_centered(|ui| {
                let cd = &mut app.timers.countdown;
                if cd.fired {
                    ui.label(
                        RichText::new(t!(lang, "время вышло", "time is up"))
                            .size(18.0)
                            .strong()
                            .color(t.warn),
                    );
                    ui.add_space(2.0);
                }
                let color = if cd.fired {
                    t.warn
                } else if cd.running {
                    t.accent
                } else {
                    t.strong
                };
                big_time(ui, &clock_text(cd.remaining, false), color, 92.0);
                ui.add_space(8.0);
                theme::progress(
                    ui,
                    t,
                    (cd.total - cd.remaining).max(0.0) as u64,
                    cd.total.max(1.0) as u64,
                    "",
                );
                ui.add_space(14.0);

                let width = (ui.available_width() - 16.0) / 3.0;
                ui.horizontal(|ui| {
                    if cd.fired {
                        if action(ui, t, t!(lang, "ещё раз", "again"), true, width) {
                            cd.fired = false;
                            cd.remaining = cd.total;
                            cd.running = true;
                        }
                    } else if cd.running {
                        if action(ui, t, t!(lang, "пауза", "pause"), true, width) {
                            cd.running = false;
                        }
                    } else {
                        let label = if cd.remaining > 0.01 && cd.remaining < cd.total - 0.01 {
                            t!(lang, "продолжить", "resume")
                        } else {
                            t!(lang, "старт", "start")
                        };
                        if action(ui, t, label, true, width) {
                            if cd.remaining < 0.01 {
                                cd.remaining = cd.total;
                            }
                            cd.fired = false;
                            cd.running = true;
                        }
                    }
                    // A minute up and down without touching the slider: the
                    // common correction after starting a tea timer.
                    let minus = format!("−1 {}", t!(lang, "мин", "min"));
                    let minus_hit = ui
                        .add_enabled_ui(!cd.running, |ui| action(ui, t, &minus, false, width))
                        .inner;
                    if minus_hit {
                        let seconds = (secs as i64 - 60).max(60);
                        secs = seconds as u32;
                        cd.total = seconds as f64;
                        cd.remaining = cd.total;
                        cd.fired = false;
                        dirty = true;
                    }
                    let plus = format!("+1 {}", t!(lang, "мин", "min"));
                    let plus_hit = ui
                        .add_enabled_ui(!cd.running, |ui| action(ui, t, &plus, false, width))
                        .inner;
                    if plus_hit {
                        let seconds = (secs as i64 + 60).min(180 * 60);
                        secs = seconds as u32;
                        cd.total = seconds as f64;
                        cd.remaining = cd.total;
                        cd.fired = false;
                        dirty = true;
                    }
                });
                ui.add_space(8.0);
                if action(ui, t, t!(lang, "сброс", "reset"), false, width) {
                    cd.fired = false;
                    cd.running = false;
                    cd.total = secs.max(1) as f64;
                    cd.remaining = cd.total;
                }
            });
        });

        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(t!(lang, "пресеты", "presets"), t.faint));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for minutes in [1u32, 3, 5, 10, 15, 25, 45, 60] {
                    let active = secs == minutes * 60;
                    let label = format!("{minutes} {}", t!(lang, "мин", "min"));
                    if preset(ui, t, &label, active) {
                        secs = minutes * 60;
                        let cd = &mut app.timers.countdown;
                        cd.total = secs as f64;
                        cd.remaining = cd.total;
                        cd.running = false;
                        cd.fired = false;
                        dirty = true;
                    }
                }
            });
            ui.add_space(6.0);
            let mut minutes = secs as f32 / 60.0;
            let changed = theme::slider_step(
                ui,
                t,
                t!(lang, "своя длина, мин", "your own length, min"),
                &mut minutes,
                1.0..=180.0,
                1.0,
                |v| format!("{v:.0} {}", t!(lang, "мин", "min")),
            );
            if changed {
                secs = (minutes * 60.0).round().max(60.0) as u32;
                dirty = true;
            }
        });
    });

    if dirty {
        app.cfg.timers.countdown_secs = secs;
        app.save_soon();
    }
}

fn stopwatch_page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    column(ui, |ui| {
        theme::card(ui, t, |ui| {
            ui.vertical_centered(|ui| {
                let sw = &mut app.timers.stopwatch;
                let color = if sw.running { t.accent } else { t.strong };
                big_time(ui, &clock_text(sw.elapsed, true), color, 92.0);
                ui.add_space(14.0);
                let width = (ui.available_width() - 16.0) / 3.0;
                ui.horizontal(|ui| {
                    let label = if sw.running {
                        t!(lang, "пауза", "pause")
                    } else if sw.elapsed > 0.0 {
                        t!(lang, "продолжить", "resume")
                    } else {
                        t!(lang, "старт", "start")
                    };
                    if action(ui, t, label, !sw.running, width) {
                        sw.running = !sw.running;
                    }
                    if action(ui, t, t!(lang, "круг", "lap"), false, width) && sw.running {
                        sw.laps.push(sw.elapsed);
                    }
                    if action(ui, t, t!(lang, "сброс", "reset"), false, width) {
                        *sw = Stopwatch::default();
                    }
                });
            });
        });

        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            let sw = &app.timers.stopwatch;
            theme::row_ends(
                ui,
                |ui| {
                    ui.label(theme::hud(t!(lang, "круги", "laps"), t.faint));
                },
                |ui| {
                    ui.label(
                        RichText::new(format!("{}", sw.laps.len()))
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.faint),
                    );
                },
            );
            if sw.laps.is_empty() {
                ui.label(
                    RichText::new(t!(
                        lang,
                        "нажмите «круг» на ходу — разница с предыдущим появится здесь",
                        "press \"lap\" while running — the difference from the previous one shows here"
                    ))
                    .size(12.0)
                    .color(t.muted),
                );
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("timer-laps")
                .max_height(300.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (i, total) in sw.laps.iter().enumerate().rev() {
                        let previous = if i == 0 { 0.0 } else { sw.laps[i - 1] };
                        let split = total - previous;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("#{}", i + 1))
                                    .family(FontFamily::Monospace)
                                    .size(12.0)
                                    .color(t.faint),
                            );
                            ui.label(
                                RichText::new(clock_text(*total, true))
                                    .family(FontFamily::Monospace)
                                    .size(15.0)
                                    .color(t.strong),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(format!("+{}", clock_text(split, true)))
                                        .family(FontFamily::Monospace)
                                        .size(13.0)
                                        .color(t.accent),
                                );
                            });
                        });
                        ui.separator();
                    }
                });
        });
    });
}

fn pomodoro_page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    column(ui, |ui| {
        theme::card(ui, t, |ui| {
            ui.vertical_centered(|ui| {
                let cfg = app.cfg.timers.clone();
                let po = &mut app.timers.pomodoro;
                let (label, color) = match po.phase {
                    Phase::Work => (t!(lang, "работа", "work"), t.accent),
                    Phase::Short => (t!(lang, "перерыв", "break"), t.good),
                    Phase::Long => (t!(lang, "длинный перерыв", "long break"), t.warn),
                };
                ui.label(RichText::new(label).size(15.0).strong().color(color));
                ui.add_space(2.0);
                big_time(ui, &clock_text(po.remaining, false), color, 92.0);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let rounds = cfg.rounds.max(1);
                    let done = po.round.min(rounds);
                    for i in 0..rounds {
                        if i > 0 {
                            ui.add_space(2.0);
                        }
                        theme::dot_scaled(ui, color, i < done, 14.0);
                    }
                    ui.add_space(8.0);
                    // The phase being counted: work is the next one after the
                    // finished rounds, a break belongs to the round just done.
                    let current = match po.phase {
                        Phase::Work => (done + 1).min(rounds),
                        Phase::Short | Phase::Long => done.max(1),
                    };
                    ui.label(
                        RichText::new(format!("{} {current}/{rounds}", t!(lang, "раунд", "round")))
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.muted),
                    );
                });
                ui.add_space(10.0);
                let width = (ui.available_width() - 10.0) / 2.0;
                ui.horizontal(|ui| {
                    let label = if po.running {
                        t!(lang, "пауза", "pause")
                    } else if po.fired {
                        t!(lang, "дальше", "next")
                    } else {
                        t!(lang, "старт", "start")
                    };
                    if action(ui, t, label, true, width) {
                        if po.fired {
                            po.next(&cfg);
                            po.running = true;
                        } else {
                            po.running = !po.running;
                        }
                    }
                    if action(ui, t, t!(lang, "пропустить", "skip"), false, width) {
                        po.next(&cfg);
                    }
                });
                ui.add_space(6.0);
                if action(
                    ui,
                    t,
                    t!(lang, "сбросить помодоро", "reset the pomodoro"),
                    false,
                    240.0,
                ) {
                    po.reset(&cfg);
                }
            });
        });

        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(
                t!(lang, "настройки помодоро", "pomodoro settings"),
                t.faint,
            ));
            let mut work = app.cfg.timers.work_secs as f32 / 60.0;
            if theme::slider_step(
                ui,
                t,
                t!(lang, "работа, мин", "work, min"),
                &mut work,
                5.0..=60.0,
                1.0,
                |v| format!("{v:.0} {}", t!(lang, "мин", "min")),
            ) {
                app.cfg.timers.work_secs = (work * 60.0) as u32;
                app.save_soon();
            }
            let mut short = app.cfg.timers.short_secs as f32 / 60.0;
            if theme::slider_step(
                ui,
                t,
                t!(lang, "перерыв, мин", "break, min"),
                &mut short,
                1.0..=30.0,
                1.0,
                |v| format!("{v:.0} {}", t!(lang, "мин", "min")),
            ) {
                app.cfg.timers.short_secs = (short * 60.0) as u32;
                app.save_soon();
            }
            let mut long = app.cfg.timers.long_secs as f32 / 60.0;
            if theme::slider_step(
                ui,
                t,
                t!(lang, "длинный перерыв, мин", "long break, min"),
                &mut long,
                5.0..=60.0,
                1.0,
                |v| format!("{v:.0} {}", t!(lang, "мин", "min")),
            ) {
                app.cfg.timers.long_secs = (long * 60.0) as u32;
                app.save_soon();
            }
            let mut rounds = app.cfg.timers.rounds;
            if theme::slider_u32(
                ui,
                t,
                t!(
                    lang,
                    "раундов до длинного перерыва",
                    "rounds before the long break"
                ),
                &mut rounds,
                1..=12,
            ) {
                app.cfg.timers.rounds = rounds;
                app.save_soon();
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                let mut auto = app.cfg.timers.auto_start;
                theme::switch(ui, &mut auto, t);
                if auto != app.cfg.timers.auto_start {
                    app.cfg.timers.auto_start = auto;
                    app.save_soon();
                }
                ui.label(
                    RichText::new(t!(
                        lang,
                        "начинать следующую фазу сам",
                        "start the next phase by itself"
                    ))
                    .size(13.0)
                    .color(t.muted),
                );
                ui.add_space(14.0);
                let mut sound = app.cfg.timers.sound;
                theme::switch(ui, &mut sound, t);
                if sound != app.cfg.timers.sound {
                    app.cfg.timers.sound = sound;
                    app.save_soon();
                }
                ui.label(
                    RichText::new(t!(lang, "звук в конце", "sound at the end"))
                        .size(13.0)
                        .color(t.muted),
                );
            });
        });
    });
}
