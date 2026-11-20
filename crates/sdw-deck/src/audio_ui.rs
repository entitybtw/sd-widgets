//! PC audio on the Deck: pick a source, confirm, play.
//!
//! The daemon records the computer (or just the picked applications) and
//! serves raw `s16le` PCM over the authenticated API. The Deck pipes it into
//! `pw-play` (or `paplay`/`aplay`), so no audio codec is linked into the app.

use crate::app::{App, Task};
use crate::theme::{self, Tokens};
use eframe::egui::{self, Align, Align2, Color32, FontFamily, Layout, RichText, Stroke, Ui, Vec2};
use sdw_core::model::{AudioApp, AudioStart, AudioStatus};
use sdw_core::t;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn page(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let status = app
        .status
        .as_ref()
        .map(|s| s.audio.clone())
        .unwrap_or_default();
    let running = app.audio_player.is_some() || status.running;

    ui.label(
        RichText::new(t!(app.lang, "звук с пк", "pc audio"))
            .size(28.0)
            .strong()
            .color(t.strong),
    );
    ui.add(
        egui::Label::new(
            RichText::new(t!(
                app.lang,
                "весь компьютер или выбранные приложения играют в динамики деки. звук идёт по локальной сети.",
                "the whole computer or picked applications play through the Deck speakers. the audio travels over the LAN."
            ))
            .size(13.5)
            .color(t.muted),
        )
        .wrap(),
    );
    ui.add_space(8.0);

    status_card(app, ui, t, &status, running);
    ui.add_space(8.0);

    // Refresh the app list when the page is opened or after a while.
    let stale = app
        .audio_apps_at
        .map(|at| at.elapsed() > Duration::from_secs(15))
        .unwrap_or(true);
    if stale && app.client().is_some() {
        refresh_apps(app);
    }

    source_card(app, ui, t, running);
    ui.add_space(8.0);
    volume_card(app, ui, t, running);
}

fn status_card(app: &mut App, ui: &mut Ui, t: &Tokens, status: &AudioStatus, running: bool) {
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    theme::dot(ui, if running { t.good } else { t.faint }, running);
                    ui.label(
                        RichText::new(if running {
                            t!(app.lang, "звук идёт на деку", "streaming to the deck")
                        } else {
                            t!(app.lang, "звук выключен", "audio off")
                        })
                        .size(18.0)
                        .strong()
                        .color(t.strong),
                    );
                });
                let detail = if !status.available {
                    t!(
                        app.lang,
                        "на компьютере нет parec/pactl (нужен pipewire или pulseaudio)",
                        "the computer has no parec/pactl (pipewire or pulseaudio is required)"
                    )
                    .to_string()
                } else if running {
                    format!(
                        "{} · {} · {} {} · {} {} · {:.1} {}",
                        status.backend,
                        if status.source == "apps" {
                            t!(app.lang, "приложения", "applications")
                        } else {
                            t!(app.lang, "весь пк", "whole pc")
                        },
                        status.rate,
                        t!(app.lang, "гц", "hz"),
                        status.channels,
                        t!(app.lang, "кан", "ch"),
                        status.bytes as f64 / 1_048_576.0,
                        t!(app.lang, "мб", "mb"),
                    )
                } else {
                    format!(
                        "{} · {}",
                        status.backend,
                        t!(
                            app.lang,
                            "запуск только после подтверждения",
                            "starts only after confirmation"
                        )
                    )
                };
                ui.label(
                    RichText::new(detail)
                        .family(FontFamily::Monospace)
                        .size(11.0)
                        .color(t.muted),
                );
                if !status.apps.is_empty() {
                    ui.label(
                        RichText::new(status.apps.join(" · "))
                            .size(11.5)
                            .color(t.faint),
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if running {
                    if theme::primary(ui, t!(app.lang, "остановить", "stop"), t).clicked()
                    {
                        stop(app);
                    }
                } else if theme::primary(ui, t!(app.lang, "включить звук", "start audio"), t)
                    .clicked()
                {
                    begin(app);
                }
                if app.audio_starting {
                    ui.spinner();
                }
            });
        });
        let error = if !app.audio_error.is_empty() {
            app.audio_error.clone()
        } else {
            status.error.clone()
        };
        if !error.is_empty() {
            ui.label(RichText::new(error).size(12.0).color(t.bad));
        }
    });
}

fn source_card(app: &mut App, ui: &mut Ui, t: &Tokens, running: bool) {
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "источник", "source"), t.faint));
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in [
                ("system", "весь компьютер", "whole computer"),
                ("apps", "приложения", "applications"),
            ] {
                let active = app.cfg.audio.source == key;
                let resp = ui.add(
                    egui::Button::new(
                        RichText::new(t!(app.lang, ru, en))
                            .size(13.5)
                            .color(if active { t.strong } else { t.muted }),
                    )
                    .fill(if active { t.accent_soft } else { t.well })
                    .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
                    .corner_radius(0.0)
                    .min_size(Vec2::new(0.0, 36.0)),
                );
                if resp.clicked() {
                    app.cfg.audio.source = key.to_string();
                    app.save();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
                    refresh_apps(app);
                }
            });
        });

        if app.cfg.audio.source == "apps" {
            ui.add_space(4.0);
            if app.audio_apps.is_empty() {
                theme::small(
                    ui,
                    t!(
                        app.lang,
                        "сейчас никто не играет звук — запустите музыку или видео и нажмите «обновить».",
                        "nothing is playing right now — start some music or video and press refresh."
                    ),
                    t.muted,
                );
            } else {
                let mut selected = app.cfg.audio.apps.clone();
                for item in &app.audio_apps {
                    app_row(ui, t, item, &mut selected);
                }
                if selected != app.cfg.audio.apps {
                    app.cfg.audio.apps = selected;
                    app.save();
                }
            }
        } else if running {
            ui.add_space(2.0);
            theme::small(
                ui,
                t!(
                    app.lang,
                    "смена источника применится после остановки и нового запуска.",
                    "changing the source applies after stopping and starting again."
                ),
                t.faint,
            );
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(t!(app.lang, "качество", "quality"))
                    .size(12.5)
                    .color(t.muted),
            );
            for rate in [48_000u32, 44_100] {
                let active = app.cfg.audio.rate == rate;
                if ui
                    .selectable_label(
                        active,
                        RichText::new(format!("{} {}", rate / 1000, t!(app.lang, "кгц", "khz")))
                            .size(12.0),
                    )
                    .clicked()
                {
                    app.cfg.audio.rate = rate;
                    app.save();
                }
            }
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "48 кгц — лучшее качество; 44.1 — чуть меньше трафика.",
                    "48 khz is the best quality; 44.1 uses a little less bandwidth."
                ))
                .size(11.0)
                .color(t.faint),
            );
        });
    });
}

fn app_row(ui: &mut Ui, t: &Tokens, item: &AudioApp, selected: &mut Vec<String>) {
    let mut on = selected.iter().any(|x| x == &item.name);
    if ui
        .add(
            egui::Button::new(
                RichText::new(format!("{}  {}", item.name, item.binary))
                    .size(12.5)
                    .color(if on { t.strong } else { t.muted }),
            )
            .fill(if on {
                t.accent_soft
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::new(1.0, if on { t.accent } else { t.line }))
            .corner_radius(0.0)
            .min_size(Vec2::new(0.0, 32.0)),
        )
        .clicked()
    {
        on = !on;
        if on {
            selected.push(item.name.clone());
        } else {
            selected.retain(|x| x != &item.name);
        }
        ui.ctx().request_repaint();
    }
}

fn volume_card(app: &mut App, ui: &mut Ui, t: &Tokens, playing: bool) {
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(
            t!(
                app.lang,
                "громкость и подтверждения",
                "volume and confirmations"
            ),
            t.faint,
        ));
        // While the computer plays on the deck, its level is the level: the
        // deck player is only a gain on top of the PC's own volume, so a
        // slider that ignored the PC would lie about what is heard.
        let mut vol = match (playing, app.pc_volume) {
            (true, Some((v, at))) if at.elapsed() < Duration::from_secs(4) => v as f32,
            _ => app.cfg.audio.volume as f32,
        };
        let (changed, released) = theme::slider_ex(
            ui,
            t,
            t!(app.lang, "громкость", "volume"),
            &mut vol,
            0.0..=100.0,
            |v| format!("{v:.0}%"),
        );
        if changed {
            app.cfg.audio.volume = vol.round().clamp(0.0, 100.0) as u8;
            app.save_soon();
            if playing {
                // "volume" sets the computer's sink to the value, so the PC
                // system-volume slider moves with the deck and the two never
                // disagree. The reply comes back as the next `PcVolume`.
                let level = app.cfg.audio.volume;
                if let Some(c) = app.client() {
                    app.spawn(move || {
                        Task::Msg(
                            c.media_control("volume", level as f64, "")
                                .map(|_| String::new())
                                .map_err(|e| e.to_string()),
                        )
                    });
                }
            }
        }
        if playing {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "громкость общая с компьютером: слайдер двигает системный звук на пк.",
                    "the level is shared with the computer: this slider moves the system sound on the pc."
                ))
                .size(11.0)
                .color(t.faint),
            );
        }
        if released {
            app.save();
            if app.audio_player.is_some() {
                // Reconnect the local player with the new gain.
                if let Some(mut child) = app.audio_player.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                spawn_player(app);
            }
        }
        let mut n = app.cfg.audio.confirmations as u32;
        if theme::slider_u32(
            ui,
            t,
            t!(app.lang, "подтверждений", "confirmations"),
            &mut n,
            0..=10,
        ) {
            app.cfg.audio.confirmations = n.min(10) as u8;
            app.save_soon();
        }
        ui.label(
            RichText::new(t!(
                app.lang,
                "«вы точно хотите перевести звук?» — столько раз",
                "“are you sure about moving the sound?” — this many times"
            ))
            .size(11.0)
            .color(t.faint),
        );
        theme::small(
            ui,
            t!(
                app.lang,
                "на компьютере эти приложения станут тише или замолчат: звук уходит на деку.",
                "those applications become quiet or silent on the computer: the sound goes to the deck."
            ),
            t.faint,
        );
    });
}

// ---------------------------------------------------------------- actions

/// Ask for the configured number of confirmations, then start.
pub fn begin(app: &mut App) {
    if app.client().is_none() {
        app.toast(t!(app.lang, "нет компьютера", "no computer"), false);
        return;
    }
    if app.audio_starting {
        return;
    }
    let n = app.cfg.audio.confirmations;
    if n > 0 {
        app.audio_confirm_left = n;
        app.audio_error.clear();
    } else {
        start_daemon(app);
    }
}

fn start_daemon(app: &mut App) {
    let Some(client) = app.client() else {
        return;
    };
    app.audio_starting = true;
    app.audio_error.clear();
    let start = AudioStart {
        source: app.cfg.audio.source.clone(),
        apps: app.cfg.audio.apps.clone(),
        rate: app.cfg.audio.rate,
        channels: 2,
    };
    app.spawn(move || Task::Audio(client.audio_start(&start).map_err(|e| e.to_string())));
}

/// Stop the daemon capture and the local player.
pub fn stop(app: &mut App) {
    if let Some(mut child) = app.audio_player.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    app.audio_confirm_left = 0;
    app.audio_starting = false;
    if let Some(client) = app.client() {
        app.spawn(move || Task::Audio(client.audio_stop().map_err(|e| e.to_string())));
    }
    if let Some(s) = app.status.as_mut() {
        s.audio.running = false;
    }
}

fn refresh_apps(app: &mut App) {
    let Some(client) = app.client() else {
        return;
    };
    app.audio_apps_at = Some(Instant::now());
    app.spawn(move || Task::AudioApps(client.audio_sources().map_err(|e| e.to_string())));
}

/// Spawn the local player that drains the HTTP stream into the speakers.
pub fn spawn_player(app: &mut App) {
    if let Some(mut old) = app.audio_player.take() {
        let _ = old.kill();
        let _ = old.wait();
    }
    // Trust the daemon's actual rate when it is known, otherwise the setting.
    let known = app.status.as_ref().map(|s| s.audio.rate).unwrap_or(0);
    let rate = if known > 0 { known } else { app.cfg.audio.rate };
    // Unity gain: the computer's own sink level is the level. The slider on
    // this page moves that level (see `volume_card`), so applying the same
    // number again here would halve the sound.
    let volume = 1.0f32;
    let rate = rate.to_string();
    let (program, args): (&str, Vec<String>) = if sdw_core::util::has("pw-play") {
        (
            "pw-play",
            vec![
                "--raw".into(),
                format!("--rate={rate}"),
                "--channels=2".into(),
                "--format=s16".into(),
                format!("--volume={volume:.2}"),
                "-".into(),
            ],
        )
    } else if sdw_core::util::has("paplay") {
        (
            "paplay",
            vec![
                "--raw".into(),
                "--format=s16le".into(),
                format!("--rate={rate}"),
                "--channels=2".into(),
                format!("--volume={}", (volume * 65536.0) as u32),
            ],
        )
    } else {
        (
            "aplay",
            vec![
                "-q".into(),
                "-t".into(),
                "raw".into(),
                "-f".into(),
                "S16_LE".into(),
                "-r".into(),
                rate,
                "-c".into(),
                "2".into(),
            ],
        )
    };

    let mut child = match Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            app.audio_error = format!("{program}: {e}");
            app.toast(app.audio_error.clone(), false);
            stop(app);
            return;
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            app.audio_error = format!("{program}: нет stdin");
            return;
        }
    };
    let Some(client) = app.client() else {
        return;
    };
    let tx = app.task_tx.clone();
    std::thread::spawn(move || match client.audio_stream() {
        Ok(mut reader) => {
            let mut buf = [0u8; 16_384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stdin.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Task::Msg(Err(format!("звук: {e}"))));
                        break;
                    }
                }
            }
        }
        Err(e) => {
            let _ = tx.send(Task::Msg(Err(format!("звук: {e}"))));
        }
    });
    app.audio_player = Some(child);
    app.audio_error.clear();
    app.toast(
        t!(app.lang, "звук идёт на деку", "audio is on the deck"),
        true,
    );
}

/// The confirmation dialog. Each press is one explicit "yes".
pub fn confirm_modal(app: &mut App, ctx: &egui::Context) {
    if app.audio_confirm_left == 0 {
        return;
    }
    let t = Tokens::get(ctx);
    let total = app.cfg.audio.confirmations.max(1);
    let left = app.audio_confirm_left;
    let mut yes = false;
    let mut no = false;
    egui::Area::new(egui::Id::new("audio-confirm"))
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(t.card)
                .stroke(Stroke::new(1.5, t.warn))
                .corner_radius(0.0)
                .inner_margin(24)
                .show(ui, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(theme::hud(
                        t!(app.lang, "подтверждение", "confirmation"),
                        t.warn,
                    ));
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "Вы точно хотите перевести звук с компьютера на деку?",
                            "Are you sure you want to move the computer's sound to the deck?"
                        ))
                        .size(21.0)
                        .strong()
                        .color(t.strong),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "на компьютере эти приложения станут тише или замолчат.",
                            "those applications become quiet or silent on the computer."
                        ))
                        .size(12.5)
                        .color(t.muted),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} {left} / {total}",
                            t!(app.lang, "осталось подтверждений:", "confirmations left:")
                        ))
                        .family(FontFamily::Monospace)
                        .size(12.0)
                        .color(t.warn),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if theme::primary(ui, t!(app.lang, "да, перевести", "yes, move it"), &t)
                            .clicked()
                        {
                            yes = true;
                        }
                        if theme::ghost(ui, t!(app.lang, "нет, отмена", "no, cancel"), &t).clicked()
                        {
                            no = true;
                        }
                    });
                });
        });
    if no {
        app.audio_confirm_left = 0;
        app.toast(t!(app.lang, "звук не тронут", "sound left alone"), true);
    } else if yes {
        app.audio_confirm_left = left.saturating_sub(1);
        if app.audio_confirm_left == 0 {
            if let Some(mut player) = app.audio_player.take() {
                let _ = player.kill();
                let _ = player.wait();
            }
            start_daemon(app);
        }
    }
}

/// Notice an unexpected player exit and stop the daemon capture with it.
pub fn tick(app: &mut App) {
    // The daemon window can start a stream too; attach the local player when
    // that happens so the Deck always plays what the daemon sends.
    if app.audio_player.is_none() && !app.audio_starting {
        let running = app
            .status
            .as_ref()
            .map(|s| s.audio.running)
            .unwrap_or(false);
        if running && app.client().is_some() {
            spawn_player(app);
            return;
        }
    }
    let exited = match app.audio_player.as_mut() {
        Some(child) => child.try_wait().ok().flatten().is_some(),
        None => false,
    };
    if !exited {
        return;
    }
    app.audio_player = None;
    let daemon_running = app
        .status
        .as_ref()
        .map(|s| s.audio.running)
        .unwrap_or(false);
    if !daemon_running {
        // The stream ended because capture was stopped on the other side:
        // that is a normal stop, not a failure.
        return;
    }
    app.audio_error = t!(
        app.lang,
        "плеер звука завершился; поток остановлен",
        "the audio player exited; the stream was stopped"
    )
    .to_string();
    if let Some(client) = app.client() {
        std::thread::spawn(move || {
            let _ = client.audio_stop();
        });
    }
}
