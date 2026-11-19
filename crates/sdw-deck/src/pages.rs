//! every page of the deck app.
//!
//! the home page is a grid-style grid of big touch tiles; the gamepad page is the
//! drawn steam deck; the rest are focused panels that stay readable at 1280×800.

use crate::app::{App, Page, Task, notification_color};
use crate::binds::BindEditor;
use crate::deck_view::{self, DeckInput, DeckViewState};
use crate::theme::{self, Tile, Tokens};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, Layout, Margin, RichText, Stroke,
    TextEdit, Ui, Vec2,
};
use sdw_core::model::*;
use sdw_core::util;
use sdw_core::{t, Lang};
use serde_json::json;

// ---------------------------------------------------------------- shared bits

fn page_title(ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(28.0).strong().color(t.strong));
    if !subtitle.is_empty() {
        ui.add(
            egui::Label::new(RichText::new(subtitle).size(13.5).color(t.muted)).wrap(),
        );
    }
    ui.add_space(4.0);
}

fn stat(ui: &mut Ui, t: &Tokens, label: &str, value: &str, color: Color32) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .family(FontFamily::Monospace)
                .size(10.5)
                .color(t.faint),
        );
        ui.label(RichText::new(value).size(15.0).strong().color(color));
    });
}

fn age(ts_ms: u64, lang: Lang) -> String {
    if ts_ms == 0 {
        return String::new();
    }
    let secs = now_ms().saturating_sub(ts_ms) / 1000;
    if secs < 45 {
        t!(lang, "только что", "just now").to_string()
    } else if secs < 3600 {
        format!("{} {}", secs / 60, t!(lang, "мин назад", "min ago"))
    } else if secs < 86400 {
        format!("{} {}", secs / 3600, t!(lang, "ч назад", "h ago"))
    } else {
        format!("{} {}", secs / 86400, t!(lang, "дн назад", "d ago"))
    }
}

fn monitor_state(app: &App) -> MonitorStatus {
    app.status
        .as_ref()
        .map(|s| s.monitor.clone())
        .unwrap_or_default()
}

fn quick_action(
    app: &mut App,
    ui: &mut Ui,
    t: &Tokens,
    mut tile: Tile,
    size: Vec2,
    page: Page,
    index: f32,
) {
    let key = match page {
        Page::Gamepad => "gamepad",
        Page::Screen => "screen",
        Page::Notifications => "notifications",
        Page::HomeAssistant => "ha",
        Page::Binds => "binds",
        Page::Computers => "computers",
        _ => "",
    };
    if !key.is_empty() && !app.tile_shown(key) {
        return;
    }
    tile.reveal = app.reveal(index);
    if tile.reveal < 1.0 {
        ui.ctx().request_repaint();
    }
    let resp = theme::tile(ui, t, &tile, size);
    if resp.clicked() {
        app.set_page(page);
    }
}

// ---------------------------------------------------------------- home

pub fn home(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let online = app.status.is_some();
    let host = app.active_host_name();
    let status = app.status.clone();
    let monitor = monitor_state(app);

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(&host).size(30.0).strong().color(t.strong));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut facts: Vec<(String, Color32)> = Vec::new();
            facts.push((
                if online {
                    t!(app.lang, "онлайн", "online").to_string()
                } else {
                    t!(app.lang, "нет связи", "offline").to_string()
                },
                if online { t.good } else { t.bad },
            ));
            if let Some(s) = &status {
                facts.push((
                    format!(
                        "viiper {}",
                        if s.viiper.running { "on" } else { "off" }
                    ),
                    if s.viiper.running { t.good } else { t.faint },
                ));
                facts.push((format!("{} pads", s.pads.len()), t.faint));
            }
            facts.push((
                format!("{} {}", app.cfg.binds.len(), t!(app.lang, "биндов", "binds")),
                t.faint,
            ));
            for (text, color) in facts.into_iter().rev() {
                ui.label(
                    RichText::new(text)
                        .family(FontFamily::Monospace)
                        .size(11.0)
                        .color(color),
                );
                ui.add_space(12.0);
            }
        });
    });
    ui.add_space(14.0);

    if app.cfg.hosts.is_empty() {
        let size = Vec2::new(ui.available_width(), 220.0);
        let resp = theme::tile(
            ui,
            t,
            &Tile {
                title: t!(app.lang, "подключить компьютер", "connect a computer").into(),
                subtitle: t!(
                    app.lang,
                    "запустите sdw-daemon на пк и подключитесь по паролю",
                    "run sdw-daemon on the PC and pair with its password"
                )
                .into(),
                glyph: theme::icon::ADD.into(),
                accent: t.accent,
                badge: None,
                badge_color: t.accent,
                dim: false,
                reveal: 1.0,
            },
            size,
        );
        if resp.clicked() {
            app.set_page(Page::Computers);
            app.add_open = true;
            app.scan_lan();
        }
        return;
    }

    // The grid: big landscape tiles on a deck-sized screen.
    let st = theme::style();
    let width = ui.available_width();
    let cols = st.tile_columns.clamp(1, 4) as usize;
    let cols = if width < 720.0 { cols.min(2) } else { cols };
    let gap = 14.0;
    let tile_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).max(160.0);
    let tile_h = st.tile_height.clamp(120.0, 300.0);
    let size = Vec2::new(tile_w, tile_h);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "геймпад", "gamepad").into(),
                subtitle: if app.gamepad_on {
                    t!(app.lang, "передаётся на пк", "streaming to the PC").into()
                } else if let Some(pad) = &app.gamepad_pad {
                    pad.name.clone()
                } else {
                    t!(app.lang, "виртуальный пад", "virtual pad").into()
                },
                glyph: theme::icon::GAMEPAD.into(),
                accent: t.accent,
                badge: Some(if app.gamepad_on {
                    "live".into()
                } else {
                    t!(app.lang, "выкл", "off").into()
                }),
                badge_color: if app.gamepad_on { t.good } else { t.faint },
                dim: !app.gamepad_on,
                reveal: 1.0,
            },
            size,
            Page::Gamepad,
            0.0,
        );
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "экран", "screen").into(),
                subtitle: if monitor.running {
                    format!("{}×{} · {}", monitor.width, monitor.height, monitor.backend_label)
                } else {
                    t!(app.lang, "дополнительный монитор", "extra monitor").into()
                },
                glyph: theme::icon::SCREEN.into(),
                accent: Color32::from_rgb(0x3d, 0xd6, 0xcf),
                badge: Some(if monitor.running { "live".into() } else { "off".into() }),
                badge_color: if monitor.running { t.good } else { t.faint },
                dim: !monitor.running,
                reveal: 1.0,
            },
            size,
            Page::Screen,
            1.0,
        );
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "уведомления", "notifications").into(),
                subtitle: if app.notifications.is_empty() {
                    t!(app.lang, "тихо", "all quiet").into()
                } else {
                    app.notifications
                        .last()
                        .map(|n| util::truncate(&n.summary, 34))
                        .unwrap_or_default()
                },
                glyph: theme::icon::NOTIFICATIONS.into(),
                accent: Color32::from_rgb(0xff, 0x6f, 0xb5),
                badge: (app.notif_unread > 0).then(|| app.notif_unread.to_string()),
                badge_color: t.warn,
                dim: app.notifications.is_empty(),
                reveal: 1.0,
            },
            size,
            Page::Notifications,
            2.0,
        );
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "home assistant", "home assistant").into(),
                subtitle: status
                    .as_ref()
                    .map(|s| {
                        if s.ha.connected {
                            format!("{} {}", s.ha.entities, t!(app.lang, "сущностей", "entities"))
                        } else if s.ha.configured {
                            t!(app.lang, "настроен", "configured").to_string()
                        } else {
                            t!(app.lang, "умный дом", "smart home").to_string()
                        }
                    })
                    .unwrap_or_else(|| t!(app.lang, "умный дом", "smart home").to_string()),
                glyph: theme::icon::HOME_ASSISTANT.into(),
                accent: t.warn,
                badge: None,
                badge_color: t.accent,
                dim: app
                    .status
                    .as_ref()
                    .map(|s| !s.ha.configured)
                    .unwrap_or(true),
                reveal: 1.0,
            },
            size,
            Page::HomeAssistant,
            3.0,
        );
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "бинды", "binds").into(),
                subtitle: format!(
                    "{} {} · {}",
                    app.cfg.binds.len(),
                    t!(app.lang, "штук", "total"),
                    t!(app.lang, "кнопки и жесты", "buttons and gestures")
                ),
                glyph: theme::icon::BINDS.into(),
                accent: Color32::from_rgb(0xb9, 0xe0, 0x5a),
                badge: None,
                badge_color: t.warn,
                dim: app.cfg.binds.is_empty(),
                reveal: 1.0,
            },
            size,
            Page::Binds,
            4.0,
        );
        quick_action(
            app,
            ui,
            t,
            Tile {
                title: t!(app.lang, "компьютеры", "computers").into(),
                subtitle: format!("{} · {}", app.cfg.hosts.len(), host),
                glyph: theme::icon::COMPUTERS.into(),
                accent: Color32::from_rgb(0x5b, 0x9b, 0xff),
                badge: None,
                badge_color: t.good,
                dim: false,
                reveal: 1.0,
            },
            size,
            Page::Computers,
            5.0,
        );
    });

    // A quiet stream of the newest notifications, one line each.
    if !app.notifications.is_empty() {
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.label(theme::hud(
                t!(app.lang, "последние уведомления", "latest notifications"),
                t.faint,
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "все", "all"), t).clicked() {
                    app.set_page(Page::Notifications);
                }
            });
        });
        ui.add_space(2.0);
        let recent: Vec<Notification> = app
            .notifications
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect();
        for n in recent {
            let color = notification_color(t, &n);
            ui.horizontal(|ui| {
                theme::dot(ui, color, true);
                ui.label(
                    RichText::new(if n.app.is_empty() { "system" } else { &n.app })
                        .family(FontFamily::Monospace)
                        .size(10.5)
                        .color(color),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(util::truncate(&n.summary, 72))
                            .size(13.5)
                            .color(t.strong),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(age(n.ts, app.lang))
                            .family(FontFamily::Monospace)
                            .size(10.5)
                            .color(t.faint),
                    );
                });
            });
        }
    }
}

// ---------------------------------------------------------------- computers

pub fn computers(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "компьютеры", "computers"),
        t!(
            app.lang,
            "один дек — много пк. переключайтесь одним нажатием.",
            "one deck, many PCs. switch with a single tap."
        ),
    );

    ui.horizontal(|ui| {
        if theme::primary(
            ui,
            t!(app.lang, "+ добавить компьютер", "+ add computer"),
            t,
        )
        .clicked()
        {
            app.add_open = !app.add_open;
        }
        if theme::ghost(ui, t!(app.lang, "искать в сети", "scan the network"), t).clicked() {
            app.scan_lan();
        }
        if app.discovering {
            ui.spinner();
            ui.label(
                RichText::new(t!(app.lang, "ищу…", "scanning…"))
                    .size(13.0)
                    .color(t.muted),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{} · {}", app.cfg.device_name, app.lang.label()))
                    .family(FontFamily::Monospace)
                    .size(11.5)
                    .color(t.faint),
            );
        });
    });
    ui.add_space(8.0);

    if app.cfg.hosts.is_empty() {
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "пока нет ни одного компьютера",
                    "no computers yet"
                ))
                .size(18.0)
                .strong()
                .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "запустите sdw-daemon на компьютере и нажмите «искать в сети».",
                    "run sdw-daemon on the computer and press “scan the network”."
                ))
                .size(13.5)
                .color(t.muted),
            );
        });
        ui.add_space(8.0);
    }

    let hosts = app.cfg.hosts.clone();
    let active = app.cfg.active_host.clone();
    let mut activate: Option<String> = None;
    let mut forget: Option<String> = None;
    for host in hosts {
        let is_active = active.as_deref() == Some(host.id.as_str());
        let reachable = is_active && app.status.is_some();
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(
                if is_active { 1.5 } else { 1.0 },
                if is_active {
                    t.accent.gamma_multiply(0.7)
                } else {
                    t.line
                },
            ))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::same(18))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&host.name)
                                    .size(19.0)
                                    .strong()
                                    .color(t.strong),
                            );
                            if is_active {
                                theme::chip(
                                    ui,
                                    if reachable {
                                        t!(app.lang, "активен", "active")
                                    } else {
                                        t!(app.lang, "нет связи", "offline")
                                    },
                                    if reachable { t.good } else { t.warn },
                                );
                            }
                        });
                        ui.label(
                            RichText::new(&host.url)
                                .family(FontFamily::Monospace)
                                .size(12.0)
                                .color(t.muted),
                        );
                        ui.horizontal(|ui| {
                            if let Some(pad) = &host.pad {
                                ui.label(
                                    RichText::new(format!(
                                        "{} {} · {}",
                                        t!(app.lang, "пад", "pad"),
                                        pad.name,
                                        pad.kind.label()
                                    ))
                                    .size(12.0)
                                    .color(t.faint),
                                );
                            }
                            if host.last_seen_ms > 0 {
                                ui.label(
                                    RichText::new(age(host.last_seen_ms, app.lang))
                                        .family(FontFamily::Monospace)
                                        .size(11.0)
                                        .color(t.faint),
                                );
                            }
                        });
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if is_active {
                            if theme::ghost(ui, t!(app.lang, "забыть", "forget"), t).clicked() {
                                forget = Some(host.id.clone());
                            }
                            theme::chip(ui, t!(app.lang, "выбран", "selected"), t.accent);
                        } else if theme::primary(
                            ui,
                            t!(app.lang, "переключиться", "switch"),
                            t,
                        )
                        .clicked()
                        {
                            activate = Some(host.id.clone());
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }
    if let Some(id) = activate {
        app.set_active(&id);
    }
    if let Some(id) = forget {
        app.forget_host(&id);
        app.toast(
            t!(app.lang, "компьютер забыт", "computer forgotten"),
            true,
        );
    }

    if !app.discovery.is_empty() {
        ui.add_space(4.0);
        ui.label(theme::hud(t!(app.lang, "найдено в сети", "found on the network"), t.faint));
        ui.add_space(4.0);
        let found = app.discovery.clone();
        for d in found {
            let base = format!("http://{}:{}", d.address, d.port);
            let already = app.cfg.hosts.iter().any(|h| h.id == d.daemon_id);
            egui::Frame::new()
                .fill(t.well)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::dot(ui, t.good, true);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&d.name)
                                    .size(15.0)
                                    .strong()
                                    .color(t.strong),
                            );
                            ui.label(
                                RichText::new(format!("{base} · sd-widgets {}", d.version))
                                    .family(FontFamily::Monospace)
                                    .size(11.5)
                                    .color(t.muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if already {
                                theme::chip(ui, t!(app.lang, "уже добавлен", "already added"), t.faint);
                            } else if theme::ghost(ui, t!(app.lang, "выбрать", "choose"), t)
                                .clicked()
                            {
                                app.pair_url = base;
                                app.pair_name = d.name.clone();
                                app.add_open = true;
                                app.pair_error.clear();
                            }
                            if !d.setup_done {
                                theme::chip(ui, t!(app.lang, "нужна настройка", "setup needed"), t.warn);
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
    }

    if app.add_open {
        ui.add_space(4.0);
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(app.lang, "подключение по паролю", "pair with a password"))
                    .size(18.0)
                    .strong()
                    .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "пароль показан в окне sdw-daemon на компьютере. он нужен только один раз.",
                    "the password is shown in sdw-daemon on the computer. it is needed only once."
                ))
                .size(13.0)
                .color(t.muted),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(app.lang, "адрес", "address")).size(13.0).color(t.muted));
                ui.add(
                    TextEdit::singleline(&mut app.pair_url)
                        .hint_text("http://192.168.1.20:47830")
                        .desired_width(320.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(app.lang, "имя", "name")).size(13.0).color(t.muted));
                ui.add(
                    TextEdit::singleline(&mut app.pair_name)
                        .hint_text(app.cfg.device_name.clone())
                        .desired_width(240.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new(t!(app.lang, "пароль", "password")).size(13.0).color(t.muted));
                ui.add(
                    TextEdit::singleline(&mut app.pair_pass)
                        .password(!app.pair_show)
                        .hint_text("••••••••")
                        .desired_width(240.0),
                );
                if ui
                    .selectable_label(app.pair_show, t!(app.lang, "показать", "show"))
                    .clicked()
                {
                    app.pair_show = !app.pair_show;
                }
            });
            if !app.pair_error.is_empty() {
                ui.label(RichText::new(&app.pair_error).size(13.0).color(t.bad));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if app.pairing {
                    ui.spinner();
                    ui.label(
                        RichText::new(t!(app.lang, "подключаюсь…", "pairing…"))
                            .size(13.0)
                            .color(t.muted),
                    );
                } else if theme::primary(
                    ui,
                    t!(app.lang, "подключить", "connect"),
                    t,
                )
                .clicked()
                {
                    app.start_pair();
                }
                if theme::ghost(ui, t!(app.lang, "отмена", "cancel"), t).clicked() {
                    app.add_open = false;
                    app.pair_error.clear();
                }
            });
        });
    }

    ui.add_space(12.0);
    ui.label(theme::hud(t!(app.lang, "этот дек", "this deck"), t.faint));
    ui.add_space(4.0);
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&app.cfg.device_name).size(16.0).color(t.strong));
                ui.label(
                    RichText::new(format!(
                        "{} {} · {} {}",
                        t!(app.lang, "id", "id"),
                        app.cfg.client_id,
                        t!(app.lang, "язык", "language"),
                        app.lang.label()
                    ))
                    .family(FontFamily::Monospace)
                    .size(11.5)
                    .color(t.faint),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "настройки", "settings"), t).clicked() {
                    app.set_page(Page::Settings);
                }
            });
        });
    });
}

// ---------------------------------------------------------------- gamepad

pub fn gamepad(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "геймпад", "gamepad"),
        t!(
            app.lang,
            "нажмите любую кнопку на рисунке, чтобы назначить на неё бинд.",
            "tap any control in the drawing to bind it."
        ),
    );

    // pad controls
    let has_pad = app.gamepad_pad.is_some();
    let status_pads = app
        .status
        .as_ref()
        .map(|s| s.pads.clone())
        .unwrap_or_default();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                if let Some(pad) = &app.gamepad_pad {
                    ui.label(
                        RichText::new(&pad.name)
                            .size(18.0)
                            .strong()
                            .color(t.strong),
                    );
                    let live = status_pads.iter().find(|p| p.id == pad.id);
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}",
                            pad.kind.label(),
                            if live.map(|p| p.connected).unwrap_or(false) {
                                t!(app.lang, "подключён", "connected")
                            } else {
                                t!(app.lang, "ожидает", "waiting")
                            },
                            if app.gamepad_on {
                                t!(app.lang, "передаётся", "streaming")
                            } else {
                                t!(app.lang, "на паузе", "paused")
                            }
                        ))
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.muted),
                    );
                } else {
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "виртуальный пад ещё не создан",
                            "no virtual pad yet"
                        ))
                        .size(17.0)
                        .strong()
                        .color(t.strong),
                    );
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "выберите тип и создайте пад на компьютере.",
                            "pick a type and create it on the computer."
                        ))
                        .size(13.0)
                        .color(t.muted),
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if has_pad {
                    let mut on = app.gamepad_on;
                    if theme::switch(ui, &mut on, t).changed() {
                        app.set_gamepad_on(on);
                    }
                    ui.label(
                        RichText::new(if app.gamepad_on {
                            t!(app.lang, "live", "live")
                        } else {
                            t!(app.lang, "off", "off")
                        })
                        .size(12.5)
                        .color(if app.gamepad_on { t.good } else { t.faint }),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(t!(app.lang, "удалить пад", "delete pad"))
                                    .size(12.5)
                                    .color(t.bad),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, t.bad.gamma_multiply(0.4)))
                            .corner_radius(CornerRadius::ZERO),
                        )
                        .clicked()
                    {
                        app.remove_pad();
                    }
                }
            });
        });
        if !has_pad {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for kind in PadKind::ALL {
                    if ui
                        .selectable_label(
                            app.gamepad_kind == kind,
                            RichText::new(kind.label()).size(13.5),
                        )
                        .clicked()
                    {
                        app.gamepad_kind = kind;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut app.gamepad_name)
                        .hint_text(format!(
                            "{} · {}",
                            app.cfg.device_name,
                            app.gamepad_kind.label()
                        ))
                        .desired_width(300.0),
                );
                if app.gamepad_pending {
                    ui.spinner();
                } else if theme::primary(
                    ui,
                    t!(app.lang, "создать пад", "create pad"),
                    t,
                )
                .clicked()
                {
                    app.create_pad();
                }
            });
        }
    });

    ui.add_space(10.0);

    // the drawing
    let state = app.gamepad.clone();
    let pressed = app.pressed.clone();
    let host = app.active_host_name();
    let pad_label = app
        .gamepad_pad
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| t!(app.lang, "пад не создан", "no pad").to_string());
    let online = app.status.is_some();
    let mut view = DeckViewState {
        cfg: app.deck_view.cfg.clone(),
        selected: app.deck_view.selected,
    };
    let clicked = deck_view::show(
        ui,
        t,
        &mut view,
        DeckInput {
            state: &state,
            pressed: &pressed,
            host: &host,
            pad_label: &pad_label,
            online,
            enabled: app.gamepad_on,
            devices: app.input_devices.len(),
        },
    );
    if clicked.is_some() {
        view.selected = clicked;
    }
    app.deck_view = view;

    if let Some(control) = app.deck_view.selected {
        ui.add_space(10.0);
        let code = control.code().to_string();
        let matching: Vec<Bind> = app
            .cfg
            .binds
            .iter()
            .filter(|b| match &b.trigger {
                Trigger::Button { code: c, .. } => c == &code,
                _ => false,
            })
            .cloned()
            .collect();
        theme::card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            t!(app.lang, "выбрано", "selected"),
                            control.name()
                        ))
                        .size(18.0)
                        .strong()
                        .color(t.strong),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} {code} · {}",
                            t!(app.lang, "код кнопки", "button code"),
                            t!(app.lang, "нажмите кнопку на деке, чтобы увидеть её здесь", "press a button on the deck to see it here")
                        ))
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.faint),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(theme::hud("×", t.muted))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        app.deck_view.selected = None;
                    }
                    if theme::ghost(
                        ui,
                        t!(app.lang, "снять выделение", "clear selection"),
                        t,
                    )
                    .clicked()
                    {
                        app.deck_view.selected = None;
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::primary(
                    ui,
                    t!(app.lang, "назначить бинд", "assign a bind"),
                    t,
                )
                .clicked()
                {
                    app.binds.editor = Some(BindEditor::for_trigger(
                        control.name(),
                        control.trigger(),
                    ));
                    app.binds.capture = false;
                }
                if theme::ghost(
                    ui,
                    t!(app.lang, "записать с деки", "capture from deck"),
                    t,
                )
                .clicked()
                {
                    let mut editor =
                        BindEditor::for_trigger(control.name(), control.trigger());
                    editor.name = control.name().to_string();
                    app.binds.editor = Some(editor);
                    app.binds.capture = true;
                }
                if ui
                    .selectable_label(
                        !app.cfg.binds.iter().any(|b| matches!(&b.trigger, Trigger::Button { code: c, .. } if c == &code)),
                        t!(app.lang, "свободна", "unbound"),
                    )
                    .clicked()
                {
                    // nothing to do, it is just a state hint
                }
            });
            if matching.is_empty() {
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "на эту кнопку пока ничего не назначено",
                        "nothing is assigned to this control yet"
                    ))
                    .size(13.0)
                    .color(t.faint),
                );
            } else {
                ui.add_space(4.0);
                for bind in matching {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&bind.name)
                                .size(14.5)
                                .color(if bind.enabled { t.strong } else { t.muted }),
                        );
                        ui.label(
                            RichText::new(bind.action.label())
                                .size(12.5)
                                .color(t.muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::ghost(ui, t!(app.lang, "изменить", "edit"), t).clicked() {
                                app.binds.editor = Some(BindEditor::from_bind(&bind));
                            }
                        });
                    });
                }
            }
        });
    }

    // look and feel
    ui.add_space(10.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "вид", "appearance"), t.faint));
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t!(app.lang, "масштаб", "scale"))
                    .size(13.0)
                    .color(t.muted),
            );
            let r = ui.add(
                egui::Slider::new(&mut app.deck_view.cfg.scale, 0.6..=1.4)
                    .show_value(false),
            );
            if r.drag_stopped() || r.lost_focus() {
                app.save();
            }
            theme::switch(ui, &mut app.deck_view.cfg.labels, t);
            ui.label(RichText::new(t!(app.lang, "подписи", "labels")).size(13.0).color(t.muted));
            theme::switch(ui, &mut app.deck_view.cfg.glow, t);
            ui.label(RichText::new(t!(app.lang, "свечение", "glow")).size(13.0).color(t.muted));
            theme::switch(ui, &mut app.deck_view.cfg.sticks, t);
            ui.label(RichText::new(t!(app.lang, "стики", "sticks")).size(13.0).color(t.muted));
        });
    });

    let devices = app.input_devices.clone();
    let errors = app.input_errors.clone();
    if !devices.is_empty() || !errors.is_empty() {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(t!(app.lang, "ввод с деки", "deck input"), t.faint));
            for d in &devices {
                ui.label(
                    RichText::new(d)
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.muted),
                );
            }
            for e in &errors {
                ui.label(RichText::new(e).size(12.5).color(t.bad));
            }
            if let Some((code, label, at)) = &app.last_button {
                ui.label(
                    RichText::new(format!(
                        "{}: {label} ({code}) · {:.1}s",
                        t!(app.lang, "последняя кнопка", "last button"),
                        at.elapsed().as_secs_f32()
                    ))
                    .family(FontFamily::Monospace)
                    .size(11.0)
                    .color(t.faint),
                );
            }
        });
    }
}

// ---------------------------------------------------------------- screen

fn backend_list() -> [MonitorBackend; 8] {
    [
        MonitorBackend::Auto,
        MonitorBackend::Hyprland,
        MonitorBackend::Vkms,
        MonitorBackend::Krfb,
        MonitorBackend::Mirror,
        MonitorBackend::X11,
        MonitorBackend::Gdigrab,
        MonitorBackend::MacCapture,
    ]
}

pub fn screen(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let monitor = app.monitor();
    page_title(
        ui,
        t,
        t!(app.lang, "экран", "screen"),
        t!(
            app.lang,
            "дополнительный монитор 1280×800 для дека, прямо в этой вкладке.",
            "an extra 1280×800 monitor for the deck, right inside this tab."
        ),
    );

    ui.horizontal(|ui| {
        if monitor.running {
            if theme::primary(
                ui,
                t!(app.lang, "остановить экран", "stop the screen"),
                t,
            )
            .clicked()
            {
                if let Some(c) = app.client() {
                    app.spawn(move || {
                        Task::MonitorStopped(c.stop_monitor().map_err(|e| e.to_string()))
                    });
                }
            }
        } else if theme::primary(
            ui,
            t!(app.lang, "запустить экран", "start the screen"),
            t,
        )
        .clicked()
        {
            let req = MonitorStart {
                mode: app.stream.mode,
                backend: app.stream.backend,
                position: app.stream.position,
                output: monitor.output.clone(),
                width: app.stream.width,
                height: app.stream.height,
                fps: app.stream.fps,
                quality: app.stream.quality,
                input_enabled: app.stream.input,
                sudo_password: String::new(),
            };
            if let Some(c) = app.client() {
                app.spawn(move || Task::Monitor(c.start_monitor(&req).map_err(|e| e.to_string())));
            }
        }
        if monitor.running {
            theme::chip(
                ui,
                &format!("{} · {}", monitor.backend_label, monitor.output),
                t.good,
            );
        } else {
            theme::chip(ui, t!(app.lang, "выключен", "off"), t.faint);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if monitor.running
                && theme::ghost(
                    ui,
                    if app.stream.fullscreen {
                        t!(app.lang, "выйти из полного экрана", "exit fullscreen")
                    } else {
                        t!(app.lang, "на весь экран", "fullscreen")
                    },
                    t,
                )
                .clicked()
            {
                let on = !app.stream.fullscreen;
                app.set_fullscreen(ui.ctx(), on);
            }
        });
    });

    ui.add_space(8.0);

    let area = ui.available_rect_before_wrap();
    let stream_h = (area.width() / 1.6).clamp(240.0, 520.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(area.width(), stream_h),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0x02, 0x02, 0x06));
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(8.0);
    if monitor.running {
        app.draw_stream(ui, t, inner, false);
        if app.stream.frames > 0 {
            ui.painter().text(
                inner.left_bottom() + Vec2::new(10.0, -10.0),
                Align2::LEFT_BOTTOM,
                format!(
                    "{}×{} · {} fps · {}",
                    monitor.width,
                    monitor.height,
                    monitor.fps,
                    age(
                        app.stream.last_frame_at.elapsed().as_millis() as u64,
                        app.lang
                    )
                ),
                egui::FontId::monospace(11.0),
                Color32::from_white_alpha(140),
            );
        }
    } else {
        ui.painter().text(
            inner.center(),
            Align2::CENTER_CENTER,
            t!(
                app.lang,
                "экран выключен — нажмите «запустить экран»",
                "the screen is off — press “start the screen”"
            ),
            egui::FontId::proportional(17.0),
            t.faint,
        );
    }

    ui.add_space(10.0);
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(theme::hud(t!(app.lang, "режим", "mode"), t.faint));
                ui.horizontal(|ui| {
                    for mode in [MonitorMode::Extend, MonitorMode::Mirror] {
                        if ui
                            .selectable_label(
                                app.stream.mode == mode,
                                match mode {
                                    MonitorMode::Extend => t!(app.lang, "расширить", "extend"),
                                    MonitorMode::Mirror => t!(app.lang, "зеркало", "mirror"),
                                },
                            )
                            .clicked()
                        {
                            app.stream.mode = mode;
                        }
                    }
                });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label(theme::hud(t!(app.lang, "бэкенд", "backend"), t.faint));
                egui::ComboBox::from_id_salt("monitor-backend")
                    .selected_text(app.stream.backend.label())
                    .show_ui(ui, |ui| {
                        for b in backend_list() {
                            if ui
                                .selectable_label(app.stream.backend == b, b.label())
                                .clicked()
                            {
                                app.stream.backend = b;
                            }
                        }
                    });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label(theme::hud(t!(app.lang, "позиция", "position"), t.faint));
                ui.add_enabled_ui(app.stream.mode == MonitorMode::Extend, |ui| {
                    egui::ComboBox::from_id_salt("monitor-position")
                        .selected_text(app.stream.position.label_lang(app.lang))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for p in MonitorPosition::ALL {
                                if ui
                                    .selectable_label(
                                        app.stream.position == p,
                                        p.label_lang(app.lang),
                                    )
                                    .clicked()
                                {
                                    app.stream.position = p;
                                }
                            }
                        });
                });
            });
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.label(theme::hud(t!(app.lang, "размер", "size"), t.faint));
                ui.horizontal(|ui| {
                    for (w, h) in [(1280, 800), (1920, 1080), (1280, 720)] {
                        if ui
                            .selectable_label(
                                app.stream.width == w && app.stream.height == h,
                                format!("{w}×{h}"),
                            )
                            .clicked()
                        {
                            app.stream.width = w;
                            app.stream.height = h;
                        }
                    }
                });
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!(app.lang, "fps", "fps")).size(12.5).color(t.muted));
            ui.add(egui::Slider::new(&mut app.stream.fps, 5..=60).show_value(true));
            ui.add_space(12.0);
            ui.label(RichText::new(t!(app.lang, "качество", "quality")).size(12.5).color(t.muted));
            ui.add(egui::Slider::new(&mut app.stream.quality, 30..=95).suffix("%"));
            ui.add_space(12.0);
            theme::switch(ui, &mut app.stream.input, t);
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "мышь и клавиатура",
                    "mouse and keyboard"
                ))
                .size(12.5)
                .color(t.muted),
            );
        });
        if !monitor.config_hint.is_empty() {
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.label(theme::hud(t!(app.lang, "подсказка", "hint"), t.faint));
                ui.add(
                    egui::Label::new(
                        RichText::new(&monitor.config_hint)
                            .family(FontFamily::Monospace)
                            .size(11.5)
                            .color(t.muted),
                    )
                    .wrap(),
                );
            });
        }
        if !monitor.last_error.is_empty() {
            ui.label(RichText::new(&monitor.last_error).size(12.5).color(t.bad));
        }
    });
}

// ---------------------------------------------------------------- notifications

pub fn notifications(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "уведомления", "notifications"),
        t!(
            app.lang,
            "всё, что всплывает на компьютере, появляется и здесь.",
            "everything that pops up on the PC shows up here too."
        ),
    );

    ui.horizontal(|ui| {
        if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
            app.last_notif =
                std::time::Instant::now() - std::time::Duration::from_secs(5);
        }
        if theme::ghost(ui, t!(app.lang, "проверить", "test"), t).clicked() {
            if let Some(c) = app.client() {
                app.spawn(move || {
                    Task::Msg(
                        c.test_notification()
                            .map(|_| t!(Lang::Ru, "тестовое уведомление", "test notification").to_string())
                            .map_err(|e| e.to_string()),
                    )
                });
            }
        }
        if !app.notifications.is_empty()
            && theme::ghost(ui, t!(app.lang, "очистить", "clear"), t).clicked()
        {
            if let Some(c) = app.client() {
                app.spawn(move || {
                    Task::Msg(
                        c.clear_notifications()
                            .map(|_| "очищено".to_string())
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            app.notifications.clear();
            app.notif_last_id = 0;
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(s) = &app.status {
                let n = &s.notifications;
                theme::chip(
                    ui,
                    if n.listening {
                        t!(app.lang, "слушаю", "listening")
                    } else if n.enabled {
                        t!(app.lang, "нет демона", "no daemon")
                    } else {
                        t!(app.lang, "выключено", "disabled")
                    },
                    if n.listening { t.good } else { t.warn },
                );
            }
        });
    });
    ui.add_space(6.0);

    if app.notifications.is_empty() {
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "пока тихо",
                    "all quiet"
                ))
                .size(18.0)
                .strong()
                .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "включите зеркалирование в sdw-daemon на пк: notifications → mirror to deck.",
                    "enable mirroring in sdw-daemon on the PC: notifications → mirror to deck."
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
        return;
    }

    let list: Vec<Notification> = app.notifications.iter().rev().cloned().collect();
    let mut dismiss: Option<u64> = None;
    for n in list {
        let color = notification_color(t, &n);
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if n.app.is_empty() {
                                    "system"
                                } else {
                                    &n.app
                                })
                                .family(FontFamily::Monospace)
                                .size(11.0)
                                .color(color),
                            );
                            ui.label(
                                RichText::new(age(n.ts, app.lang))
                                    .family(FontFamily::Monospace)
                                    .size(10.5)
                                    .color(t.faint),
                            );
                            if n.urgency >= 2 {
                                theme::chip(ui, t!(app.lang, "важно", "urgent"), t.bad);
                            }
                        });
                        ui.label(
                            RichText::new(&n.summary)
                                .size(16.0)
                                .strong()
                                .color(t.strong),
                        );
                        if !n.body.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&n.body).size(13.0).color(t.muted),
                                )
                                .wrap(),
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(theme::hud("×", t.muted))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE),
                            )
                            .on_hover_text(t!(app.lang, "скрыть", "dismiss"))
                            .clicked()
                        {
                            dismiss = Some(n.id);
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }
    if let Some(id) = dismiss {
        app.notifications.retain(|n| n.id != id);
        if let Some(c) = app.client() {
            app.spawn(move || {
                Task::Msg(
                    c.dismiss_notification(id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }
}

// ---------------------------------------------------------------- home assistant

fn ha_glyph(domain: &str) -> &'static str {
    match domain {
        "light" => theme::icon::BULB,
        "switch" => theme::icon::POWER,
        "sensor" => theme::icon::SENSORS,
        "binary_sensor" => theme::icon::CHECK,
        "climate" => theme::icon::THERMOSTAT,
        "media_player" => theme::icon::PLAY,
        "cover" => theme::icon::BLINDS,
        "lock" => theme::icon::LOCK,
        "scene" => theme::icon::SCENE,
        "script" => theme::icon::CODE,
        "camera" => theme::icon::CAMERA,
        "person" => theme::icon::PERSON,
        _ => theme::icon::WIDGETS,
    }
}

pub fn home_assistant(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let ha = app
        .status
        .as_ref()
        .map(|s| s.ha.clone())
        .unwrap_or_default();
    page_title(
        ui,
        t,
        t!(app.lang, "home assistant", "home assistant"),
        t!(
            app.lang,
            "свет, климат, медиа — удобными плитками, без браузера.",
            "lights, climate, media — as friendly tiles, no browser."
        ),
    );

    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if ha.connected {
                        t!(app.lang, "подключено", "connected")
                    } else if ha.configured {
                        t!(app.lang, "настроено, нет связи", "configured, offline")
                    } else {
                        t!(app.lang, "не настроено", "not configured")
                    })
                    .size(17.0)
                    .strong()
                    .color(if ha.connected { t.good } else { t.strong }),
                );
                if ha.configured {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} {} · {}",
                            util::truncate(&ha.url, 48),
                            ha.entities,
                            t!(app.lang, "сущностей", "entities"),
                            if ha.error.is_empty() {
                                t!(app.lang, "ok", "ok").to_string()
                            } else {
                                util::truncate(&ha.error, 60)
                            }
                        ))
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.muted),
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ha.configured {
                    if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
                        app.ha.last_fetch =
                            std::time::Instant::now() - std::time::Duration::from_secs(60);
                    }
                }
            });
        });
    });

    if !ha.configured {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(app.lang, "подключить home assistant", "connect home assistant"))
                    .size(18.0)
                    .strong()
                    .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "адрес и долгоживущий токен из профиля home assistant.",
                    "the address and a long-lived token from your home assistant profile."
                ))
                .size(13.0)
                .color(t.muted),
            );
            if app.ha.url.is_empty() {
                app.ha.url = "http://homeassistant.local:8123".into();
            }
            ui.add(
                TextEdit::singleline(&mut app.ha.url)
                    .hint_text("http://homeassistant.local:8123")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut app.ha.token)
                        .password(!app.ha.token_show)
                        .hint_text(t!(app.lang, "токен", "token"))
                        .desired_width(380.0),
                );
                if ui
                    .selectable_label(app.ha.token_show, t!(app.lang, "показать", "show"))
                    .clicked()
                {
                    app.ha.token_show = !app.ha.token_show;
                }
            });
            if !app.ha.error.is_empty() {
                ui.label(RichText::new(&app.ha.error).size(12.5).color(t.bad));
            }
            if theme::primary(ui, t!(app.lang, "подключить", "connect"), t).clicked() {
                let url = app.ha.url.trim().to_string();
                let token = app.ha.token.trim().to_string();
                if let Some(c) = app.client() {
                    app.spawn(move || {
                        Task::Msg(
                            c.ha_config(&url, &token, &[])
                                .map(|_| "home assistant подключён".into())
                                .map_err(|e| e.to_string()),
                        )
                    });
                }
                app.last_status = std::time::Instant::now()
                    - std::time::Duration::from_secs(5);
            }
        });
        return;
    }

    // favorites and search
    let mut favorites = ha.favorites.clone();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut app.ha.search)
                .hint_text(t!(app.lang, "поиск по имени", "search by name"))
                .desired_width(300.0),
        );
        if !favorites.is_empty()
            && ui
                .selectable_label(
                    !app.ha.search.is_empty() && app.ha.search.starts_with("★"),
                    t!(app.lang, "только избранное", "favorites only"),
                )
                .clicked()
        {
            if app.ha.search.starts_with("★") {
                app.ha.search.clear();
            } else {
                app.ha.search = "★".into();
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    app.ha.entities.len(),
                    t!(app.lang, "всего", "total")
                ))
                .family(FontFamily::Monospace)
                .size(11.5)
                .color(t.faint),
            );
        });
    });
    ui.add_space(4.0);

    if app.ha.entities.is_empty() {
        theme::card(ui, t, |ui| {
            if app.ha.pending {
                ui.spinner();
            } else {
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "нет сущностей. проверьте токен и адрес.",
                        "no entities. check the token and the address."
                    ))
                    .size(13.5)
                    .color(t.muted),
                );
            }
        });
        return;
    }

    let needle = app.ha.search.trim().trim_start_matches('★').to_lowercase();
    let favorites_only = app.ha.search.starts_with('★');
    let mut entities: Vec<&HaEntity> = app
        .ha
        .entities
        .iter()
        .filter(|e| {
            let name = e.name().to_lowercase();
            let id = e.entity_id.to_lowercase();
            (needle.is_empty() || name.contains(&needle) || id.contains(&needle))
                && (!favorites_only || favorites.contains(&e.entity_id))
        })
        .collect();
    entities.sort_by_key(|e| {
        (
            !favorites.contains(&e.entity_id),
            e.domain().to_string(),
            e.name().to_lowercase(),
        )
    });

    let mut toggle: Option<String> = None;
    let mut brightness: Option<(String, u8)> = None;
    let mut star: Option<String> = None;

    ui.horizontal_wrapped(|ui| {
        let width = ui.available_width();
        let cols = if width > 920.0 { 3 } else if width > 620.0 { 2 } else { 1 };
        let w = ((width - 10.0 * (cols as f32 - 1.0)) / cols as f32).max(170.0);
        for e in entities {
            let on = e.is_on();
            let accent = if on { t.accent } else { t.faint };
            egui::Frame::new()
                .fill(t.card)
                .stroke(Stroke::new(1.0, if on { accent.gamma_multiply(0.45) } else { t.line }))
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(w - 28.0);
                    ui.horizontal(|ui| {
                        ui.label(theme::icon(ha_glyph(e.domain()), 19.0, accent));
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(e.name()).size(14.0).strong().color(t.strong),
                                )
                                .truncate(),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    sdw_core::ha::domain_label(e.domain(), app.lang),
                                    util::truncate(&e.state, 18)
                                ))
                                .family(FontFamily::Monospace)
                                .size(10.5)
                                .color(t.faint),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if favorites.contains(&e.entity_id) {
                                if ui
                                    .add(
                                        egui::Button::new(theme::icon(
                                            theme::icon::STAR,
                                            16.0,
                                            t.warn,
                                        ))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE),
                                    )
                                    .clicked()
                                {
                                    star = Some(e.entity_id.clone());
                                }
                            } else if ui
                                .add(
                                    egui::Button::new(theme::icon(
                                        theme::icon::STAR,
                                        16.0,
                                        t.faint,
                                    ))
                                    .fill(Color32::TRANSPARENT)
                                    .stroke(Stroke::NONE),
                                )
                                .clicked()
                            {
                                star = Some(e.entity_id.clone());
                            }
                        });
                    });
                    if sdw_core::ha::is_controllable(e) {
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            let label = if on {
                                t!(app.lang, "выключить", "turn off")
                            } else {
                                t!(app.lang, "включить", "turn on")
                            };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(label).size(12.0).color(accent))
                                        .fill(accent.gamma_multiply(0.12))
                                        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.4)))
                                        .corner_radius(CornerRadius::ZERO),
                                )
                                .clicked()
                            {
                                toggle = Some(e.entity_id.clone());
                            }
                            if let Some(b) = e.brightness() {
                                let mut v = b;
                                let r = ui.add(
                                    egui::Slider::new(&mut v, 0..=255)
                                        .show_value(false),
                                );
                                if r.drag_stopped() {
                                    brightness = Some((e.entity_id.clone(), v));
                                }
                                ui.label(
                                    RichText::new(format!("{}%", (v as u32 * 100 / 255)))
                                        .family(FontFamily::Monospace)
                                        .size(10.5)
                                        .color(t.faint),
                                );
                            }
                        });
                    } else if let Some(temp) = e.temperature() {
                        ui.label(
                            RichText::new(format!("{temp:.1}°"))
                                .size(20.0)
                                .strong()
                                .color(t.warn),
                        );
                    } else if let Some(media) = e.media_title() {
                        ui.label(
                            RichText::new(util::truncate(&media, 40))
                                .size(12.5)
                                .color(t.muted),
                        );
                        if let Some(artist) = e.media_artist() {
                            ui.label(
                                RichText::new(util::truncate(&artist, 40))
                                    .size(11.0)
                                    .color(t.faint),
                            );
                        }
                    }
                });
            ui.add_space(2.0);
        }
    });

    if let Some(id) = toggle {
        if let Some(e) = app.ha.entities.iter().find(|e| e.entity_id == id).cloned() {
            let domain = e.domain().to_string();
            let service = if matches!(domain.as_str(), "scene" | "script" | "button") {
                "turn_on".to_string()
            } else if e.is_on() {
                "turn_off".to_string()
            } else {
                "turn_on".to_string()
            };
            app.ha_call(HaCall {
                domain,
                service,
                target: json!({ "entity_id": e.entity_id }),
                data: serde_json::Value::Null,
            });
            app.ha.last_fetch =
                std::time::Instant::now() - std::time::Duration::from_secs(60);
        }
    }
    if let Some((id, value)) = brightness {
        if let Some(e) = app.ha.entities.iter().find(|e| e.entity_id == id).cloned() {
            app.ha_call(HaCall {
                domain: "light".into(),
                service: "turn_on".into(),
                target: json!({ "entity_id": e.entity_id }),
                data: json!({ "brightness": value }),
            });
        }
    }
    if let Some(id) = star {
        if favorites.contains(&id) {
            favorites.retain(|f| f != &id);
        } else {
            favorites.push(id);
        }
        if let Some(c) = app.client() {
            let list = favorites.clone();
            app.spawn(move || {
                Task::Msg(
                    c.set_favorites(&list)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string()),
                )
            });
        }
    }
}

// ---------------------------------------------------------------- settings

pub fn settings(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "настройки", "settings"),
        t!(
            app.lang,
            "вид, язык и поведение приложения на деке.",
            "look, language and behaviour of the app on the deck."
        ),
    );

    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "устройство", "device"), t.faint));
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!(app.lang, "имя дека", "deck name")).size(13.0).color(t.muted));
            ui.add(
                TextEdit::singleline(&mut app.settings_name)
                    .desired_width(280.0),
            );
            if theme::ghost(ui, t!(app.lang, "сохранить", "save"), t).clicked() {
                let name = app.settings_name.trim().to_string();
                if !name.is_empty() {
                    app.cfg.device_name = name.clone();
                    app.pair_name = name;
                    app.save();
                    app.toast(t!(app.lang, "имя сохранено", "name saved"), true);
                }
            }
        });
        ui.label(
            RichText::new(format!(
                "{} {} · {} {}",
                t!(app.lang, "id", "id"),
                app.cfg.client_id,
                t!(app.lang, "версия", "version"),
                VERSION
            ))
            .family(FontFamily::Monospace)
            .size(11.0)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "язык и тема", "language and theme"), t.faint));
        ui.horizontal(|ui| {
            for lang in [Lang::Ru, Lang::En] {
                if ui
                    .selectable_label(app.lang == lang, lang.label())
                    .clicked()
                {
                    app.set_lang(lang);
                }
            }
            ui.add_space(16.0);
            let mut dark = app.dark;
            theme::switch(ui, &mut dark, t);
            if dark != app.dark {
                app.set_dark(dark);
            }
            ui.label(
                RichText::new(if app.dark {
                    t!(app.lang, "тёмная", "dark")
                } else {
                    t!(app.lang, "светлая", "light")
                })
                .size(13.0)
                .color(t.muted),
            );
            ui.add_space(16.0);
            ui.label(RichText::new(t!(app.lang, "масштаб ui", "ui scale")).size(13.0).color(t.muted));
            let r = ui.add(egui::Slider::new(&mut app.cfg.ui_scale, 0.8..=1.4));
            if r.drag_stopped() {
                app.save();
            }
        });
        ui.horizontal(|ui| {
            theme::switch(ui, &mut app.cfg.show_clock, t);
            ui.label(RichText::new(t!(app.lang, "часы в шапке", "clock in the header")).size(13.0).color(t.muted));
            ui.add_space(14.0);
            let mut gestures = app.cfg.global_gestures;
            theme::switch(ui, &mut gestures, t);
            if gestures != app.cfg.global_gestures {
                app.cfg.global_gestures = gestures;
                app.save();
            }
            ui.label(RichText::new(t!(app.lang, "свайпы страниц", "page swipes")).size(13.0).color(t.muted));
            ui.add_space(14.0);
            let mut grid = app.cfg.style.grid;
            if theme::switch(ui, &mut grid, t).changed() {
                app.cfg.style.grid = grid;
                app.apply_style(ui.ctx());
            }
            ui.label(RichText::new(t!(app.lang, "сетка", "grid")).size(13.0).color(t.muted));
        });
        ui.label(
            RichText::new(t!(
                app.lang,
                "3 пальца влево/вправо — страницы, 4 пальца влево/вправо — компьютеры, 4 вверх — домой.",
                "3 fingers left/right — pages, 4 fingers left/right — computers, 4 up — home."
            ))
            .size(11.5)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "геймпад и вид", "gamepad and view"), t.faint));
        ui.horizontal(|ui| {
            let mut cfg = app.deck_view.cfg.clone();
            let r = ui.add(egui::Slider::new(&mut cfg.scale, 0.6..=1.4).text(t!(app.lang, "масштаб рисунка", "drawing scale")));
            if r.drag_stopped() {
                app.deck_view.cfg = cfg;
                app.save();
            }
            theme::switch(ui, &mut app.deck_view.cfg.labels, t);
            ui.label(RichText::new(t!(app.lang, "подписи", "labels")).size(13.0).color(t.muted));
            theme::switch(ui, &mut app.deck_view.cfg.glow, t);
            ui.label(RichText::new(t!(app.lang, "свечение", "glow")).size(13.0).color(t.muted));
            theme::switch(ui, &mut app.deck_view.cfg.sticks, t);
            ui.label(RichText::new(t!(app.lang, "стики", "sticks")).size(13.0).color(t.muted));
        });
        ui.horizontal(|ui| {
            if theme::ghost(ui, t!(app.lang, "сбросить вид", "reset view"), t).clicked() {
                app.deck_view.cfg = DeckViewCfg::default();
                app.save();
            }
            if theme::ghost(ui, t!(app.lang, "бинды", "binds"), t).clicked() {
                app.set_page(Page::Binds);
            }
        });
    });

    let devices = app.input_devices.clone();
    let errors = app.input_errors.clone();
    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "ввод", "input"), t.faint));
        if devices.is_empty() {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "устройства не найдены. добавьте пользователя в группу input.",
                    "no devices found. add the user to the input group."
                ))
                .size(13.0)
                .color(t.muted),
            );
        }
        for d in devices {
            ui.label(
                RichText::new(d)
                    .family(FontFamily::Monospace)
                    .size(11.5)
                    .color(t.muted),
            );
        }
        for e in errors {
            ui.label(RichText::new(e).size(12.0).color(t.bad));
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} {}",
                t!(app.lang, "конфиг", "config"),
                sdw_core::config::path().display()
            ))
            .family(FontFamily::Monospace)
            .size(11.0)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("sd-widgets")
                        .size(18.0)
                        .strong()
                        .color(t.strong),
                );
                ui.label(
                    RichText::new(format!(
                        "{} {} · protocol {} · {}",
                        t!(app.lang, "версия", "version"),
                        VERSION,
                        PROTOCOL,
                        "MIT"
                    ))
                    .family(FontFamily::Monospace)
                    .size(11.0)
                    .color(t.faint),
                );
                ui.label(
                    RichText::new("https://git.entitybtw.ru/entitybtw/sd-widgets")
                        .family(FontFamily::Monospace)
                        .size(11.0)
                        .color(t.accent),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "выйти", "quit"), t).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });
    });
}

// ---------------------------------------------------------------- appearance

fn hex_of(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn color_row(ui: &mut Ui, t: &Tokens, style: &mut ThemeCfg, name: &str, label: &str) -> bool {
    let raw = theme::css_var(&style.css, name)
        .or_else(|| theme::css_var(&style.css, &format!("light-{name}")));
    let c = raw
        .as_deref()
        .and_then(theme::parse_color)
        .unwrap_or(t.ink);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.5).color(t.muted));
        let mut rgb = [c.r(), c.g(), c.b()];
        let resp = ui.color_edit_button_srgb(&mut rgb);
        if resp.changed() {
            let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            theme::css_set(&mut style.css, name, &hex_of(c));
            return true;
        }
        ui.label(
            RichText::new(hex_of(c))
                .family(FontFamily::Monospace)
                .size(11.0)
                .color(t.faint),
        );
        false
    })
    .inner
}

fn palette(ui: &mut Ui, t: &Tokens, style: &mut ThemeCfg, prefix: &str, lang: Lang) -> bool {
    let p = |n: &str| {
        if prefix.is_empty() {
            n.to_string()
        } else {
            format!("{prefix}-{n}")
        }
    };
    let mut dirty = false;
    for (key, label) in [
        ("bg", t!(lang, "фон", "background")),
        ("card", t!(lang, "карточки", "cards")),
        ("ink", t!(lang, "текст", "text")),
        ("muted", t!(lang, "вторичный", "secondary")),
        ("line", t!(lang, "линии", "lines")),
        ("accent", t!(lang, "акцент", "accent")),
    ] {
        dirty |= color_row(ui, t, style, &p(key), label);
    }
    dirty
}

pub fn appearance(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "вид", "look"),
        t!(
            app.lang,
            "всё, что можно настроить: палитра, css, плотность, страницы и звуки.",
            "everything you can tune: palette, css, density, pages and sounds."
        ),
    );

    let mut style = app.cfg.style.clone();
    let mut style_dirty = false;

    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "пресеты", "presets"), t.faint));
        ui.horizontal_wrapped(|ui| {
            for (key, name) in theme::PRESETS {
                let active = theme::css_var(&style.css, "bg")
                    .as_deref()
                    .and_then(theme::parse_color)
                    == theme::css_var(theme::preset_css(key), "bg")
                        .as_deref()
                        .and_then(theme::parse_color);
                let resp = ui.add(
                    egui::Button::new(
                        RichText::new(name)
                            .size(13.5)
                            .color(if active { t.strong } else { t.muted }),
                    )
                    .fill(if active { t.accent_soft } else { t.well })
                    .stroke(Stroke::new(
                        1.0,
                        if active { t.accent } else { t.line },
                    ))
                    .corner_radius(0.0)
                    .min_size(Vec2::new(0.0, 38.0)),
                );
                if resp.clicked() {
                    theme::apply_preset(&mut style, key);
                    app.style_draft = style.css.clone();
                    style_dirty = true;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let mut dark = app.dark;
            theme::switch(ui, &mut dark, t);
            if dark != app.dark {
                app.set_dark(dark);
                style_dirty = true;
            }
            ui.label(
                RichText::new(if app.dark {
                    t!(app.lang, "тёмная", "dark")
                } else {
                    t!(app.lang, "светлая", "light")
                })
                .size(13.0)
                .color(t.muted),
            );
            ui.add_space(18.0);
            ui.label(RichText::new(t!(app.lang, "масштаб ui", "ui scale")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut app.cfg.ui_scale, 0.8..=1.4).fixed_decimals(2))
                .drag_stopped()
            {
                app.save();
            }
        });
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "палитра", "palette"), t.faint));
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                theme::small(ui, t!(app.lang, "тёмная", "dark"), t.faint);
                style_dirty |= palette(ui, t, &mut style, "", app.lang);
            });
            ui.add_space(28.0);
            ui.vertical(|ui| {
                theme::small(ui, t!(app.lang, "светлая", "light"), t.faint);
                style_dirty |= palette(ui, t, &mut style, "light", app.lang);
            });
        });
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "сетка и плотность", "grid and density"), t.faint));
        ui.horizontal(|ui| {
            let mut grid = style.grid;
            theme::switch(ui, &mut grid, t);
            if grid != style.grid {
                style.grid = grid;
                style_dirty = true;
            }
            ui.label(RichText::new(t!(app.lang, "сетка", "grid")).size(13.0).color(t.muted));
            ui.add_space(14.0);
            let mut compact = style.compact;
            theme::switch(ui, &mut compact, t);
            if compact != style.compact {
                style.compact = compact;
                style_dirty = true;
            }
            ui.label(RichText::new(t!(app.lang, "компактно", "compact")).size(13.0).color(t.muted));
            ui.add_space(14.0);
            let mut anim = style.animations;
            theme::switch(ui, &mut anim, t);
            if anim != style.animations {
                style.animations = anim;
                style_dirty = true;
            }
            ui.label(RichText::new(t!(app.lang, "анимации", "animations")).size(13.0).color(t.muted));
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!(app.lang, "шаг сетки", "grid step")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut style.grid_step, 16.0..=64.0).step_by(8.0))
                .changed()
            {
                theme::css_set(&mut style.css, "grid-step", &format!("{}", style.grid_step as u32));
                style_dirty = true;
            }
            ui.add_space(14.0);
            ui.label(RichText::new(t!(app.lang, "яркость", "alpha")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut style.grid_alpha, 0.0..=40.0))
                .changed()
            {
                theme::css_set(
                    &mut style.css,
                    "grid-alpha",
                    &format!("{}", style.grid_alpha.round() as u32),
                );
                style_dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!(app.lang, "плотность", "density")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut style.density, 0.65..=1.9).fixed_decimals(2))
                .changed()
            {
                style_dirty = true;
            }
            ui.add_space(14.0);
            ui.label(RichText::new(t!(app.lang, "шрифт", "font")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut style.font_scale, 0.75..=1.8).fixed_decimals(2))
                .changed()
            {
                style_dirty = true;
            }
            ui.add_space(14.0);
            ui.label(RichText::new(t!(app.lang, "высота плиток", "tile height")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut style.tile_height, 120.0..=300.0).step_by(8.0))
                .changed()
            {
                style_dirty = true;
            }
            ui.add_space(14.0);
            ui.label(RichText::new(t!(app.lang, "колонки", "columns")).size(13.0).color(t.muted));
            if ui
                .add(egui::DragValue::new(&mut style.tile_columns).range(1..=4))
                .changed()
            {
                style_dirty = true;
            }
        });
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "интерфейс", "interface"), t.faint));
        ui.horizontal_wrapped(|ui| {
            for (value, ru, en) in [
                (&mut style.arrows, "стрелки", "arrows"),
                (&mut style.dock, "док", "dock"),
                (&mut style.labels, "подписи", "labels"),
                (&mut style.glyphs, "глифы", "glyphs"),
            ] {
                let before = *value;
                theme::switch(ui, value, t);
                if *value != before {
                    style_dirty = true;
                }
                ui.label(RichText::new(t!(app.lang, ru, en)).size(13.0).color(t.muted));
                ui.add_space(14.0);
            }
        });
        ui.add_space(4.0);
        ui.label(theme::hud(t!(app.lang, "страницы в доке", "pages in the dock"), t.faint));
        let keys: Vec<&str> = Page::ORDER.iter().map(|p| p.key()).collect();
        ui.horizontal_wrapped(|ui| {
            for page in Page::ORDER {
                let mut shown = style.pages.is_empty() || style.pages.iter().any(|p| p == page.key());
                let before = shown;
                theme::switch(ui, &mut shown, t);
                if shown != before {
                    if style.pages.is_empty() {
                        style.pages = keys.iter().map(|k| k.to_string()).collect();
                    }
                    let key = page.key().to_string();
                    if shown && !style.pages.contains(&key) {
                        style.pages.push(key);
                    } else if !shown {
                        style.pages.retain(|k| *k != key);
                    }
                    style_dirty = true;
                }
                ui.label(RichText::new(page.label(app.lang)).size(12.5).color(t.muted));
                ui.add_space(10.0);
            }
        });
        ui.add_space(4.0);
        ui.label(theme::hud(t!(app.lang, "плитки на главной", "home tiles"), t.faint));
        let tiles = [
            ("gamepad", "геймпад", "gamepad"),
            ("screen", "экран", "screen"),
            ("notifications", "уведомления", "notifications"),
            ("ha", "home assistant", "home assistant"),
            ("binds", "бинды", "binds"),
            ("computers", "компьютеры", "computers"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in tiles {
                let mut shown = style.home_tiles.is_empty()
                    || style.home_tiles.iter().any(|k| k == key);
                let before = shown;
                theme::switch(ui, &mut shown, t);
                if shown != before {
                    if style.home_tiles.is_empty() {
                        style.home_tiles = tiles.iter().map(|(k, _, _)| k.to_string()).collect();
                    }
                    let k = key.to_string();
                    if shown && !style.home_tiles.contains(&k) {
                        style.home_tiles.push(k);
                    } else if !shown {
                        style.home_tiles.retain(|x| *x != k);
                    }
                    style_dirty = true;
                }
                ui.label(RichText::new(t!(app.lang, ru, en)).size(12.5).color(t.muted));
                ui.add_space(10.0);
            }
        });
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(app.lang, "css темы", "theme css"), t.faint));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "сброс", "reset"), t).clicked() {
                    style.css = PRESET_GRID.to_string();
                    app.style_draft = style.css.clone();
                    style_dirty = true;
                }
                if theme::primary(ui, t!(app.lang, "применить", "apply"), t).clicked() {
                    style.css = app.style_draft.clone();
                    style_dirty = true;
                }
            });
        });
        if ui
            .add(
                TextEdit::multiline(&mut app.style_draft)
                    .font(egui::TextStyle::Monospace)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            style.css = app.style_draft.clone();
            style_dirty = true;
        }
        ui.label(
            RichText::new(t!(
                app.lang,
                "переменные: --bg, --card, --ink, --muted, --line, --accent и их light- версии.",
                "variables: --bg, --card, --ink, --muted, --line, --accent and their light- versions."
            ))
            .family(FontFamily::Monospace)
            .size(11.0)
            .color(t.faint),
        );
    });

    // ---------------------------------------------------------------- sound
    let mut snd = app.cfg.sound.clone();
    let mut snd_dirty = false;
    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "звуки", "sounds"), t.faint));
        ui.horizontal(|ui| {
            let before = snd.enabled;
            theme::switch(ui, &mut snd.enabled, t);
            if snd.enabled != before {
                snd_dirty = true;
                if snd.enabled {
                    crate::sound::play(&snd, crate::sound::Cue::Click);
                }
            }
            ui.label(
                RichText::new(t!(app.lang, "звуки интерфейса", "interface sounds"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add_space(18.0);
            ui.label(RichText::new(t!(app.lang, "громкость", "volume")).size(13.0).color(t.muted));
            if ui
                .add(egui::Slider::new(&mut snd.volume, 0..=100))
                .changed()
            {
                snd_dirty = true;
            }
            ui.add_space(18.0);
            ui.label(RichText::new(t!(app.lang, "набор", "pack")).size(13.0).color(t.muted));
            for (key, label) in [("grid", "Grid"), ("soft", "Soft"), ("chip", "Chip")] {
                if ui
                    .selectable_label(snd.pack == key, label)
                    .clicked()
                    && snd.pack != key
                {
                    snd.pack = key.to_string();
                    snd_dirty = true;
                    crate::sound::play(&snd, crate::sound::Cue::Click);
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (cue, ru, en) in [
                (crate::sound::Cue::Click, "клик", "click"),
                (crate::sound::Cue::Navigate, "страница", "page"),
                (crate::sound::Cue::Open, "открыть", "open"),
                (crate::sound::Cue::Back, "назад", "back"),
                (crate::sound::Cue::Notify, "уведомление", "notify"),
                (crate::sound::Cue::Error, "ошибка", "error"),
                (crate::sound::Cue::Startup, "запуск", "startup"),
            ] {
                if theme::ghost(ui, t!(app.lang, ru, en), t).clicked() {
                    crate::sound::play(&snd, cue);
                }
            }
        });
        egui::CollapsingHeader::new(t!(app.lang, "свои файлы", "custom files"))
            .id_salt("sound-files")
            .show(ui, |ui| {
                for (ru, en, cue) in [
                    ("клик", "click", crate::sound::Cue::Click),
                    ("страница", "page", crate::sound::Cue::Navigate),
                    ("открыть", "open", crate::sound::Cue::Open),
                    ("назад", "back", crate::sound::Cue::Back),
                    ("уведомление", "notify", crate::sound::Cue::Notify),
                    ("ошибка", "error", crate::sound::Cue::Error),
                    ("запуск", "startup", crate::sound::Cue::Startup),
                ] {
                    let mut changed = false;
                    let mut test = false;
                    {
                        let buf = match cue {
                            crate::sound::Cue::Click => &mut snd.click,
                            crate::sound::Cue::Navigate => &mut snd.navigate,
                            crate::sound::Cue::Open => &mut snd.open,
                            crate::sound::Cue::Back => &mut snd.back,
                            crate::sound::Cue::Notify => &mut snd.notify,
                            crate::sound::Cue::Error => &mut snd.error,
                            crate::sound::Cue::Startup => &mut snd.startup,
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t!(app.lang, ru, en))
                                    .size(12.5)
                                    .color(t.muted),
                            );
                            changed = ui
                                .add(
                                    TextEdit::singleline(buf)
                                        .hint_text("/home/deck/sounds/click.wav")
                                        .desired_width(360.0),
                                )
                                .changed();
                            test = theme::ghost(ui, "▶", t).clicked();
                        });
                    }
                    if changed {
                        snd_dirty = true;
                    }
                    if test {
                        crate::sound::play(&snd, cue);
                    }
                }
            });
    });

    if style_dirty {
        app.cfg.style = style;
        app.style_draft = app.cfg.style.css.clone();
        app.apply_style(ui.ctx());
    }
    if snd_dirty {
        app.cfg.sound = snd;
        app.save();
    }
}
