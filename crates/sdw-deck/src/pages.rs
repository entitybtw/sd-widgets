//! every page of the deck app.
//!
//! the home page is a grid-style grid of big touch tiles; the gamepad page is the
//! drawn steam deck; the rest are focused panels that stay readable at 1280×800.

use crate::app::{notification_color, App, Page, Task};
use crate::binds::BindEditor;
use crate::deck_view::{self, DeckInput, DeckViewState};
use crate::theme::{self, Tile, Tokens};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, Layout, Margin, Rect, RichText, Sense,
    Stroke, TextEdit, Ui, Vec2,
};
use sdw_core::model::*;
use sdw_core::util;
use sdw_core::{t, Lang};
use serde_json::json;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- shared bits

fn page_title(ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(28.0).strong().color(t.strong));
    if !subtitle.is_empty() {
        ui.add(egui::Label::new(RichText::new(subtitle).size(13.5).color(t.muted)).wrap());
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

fn hairline(ui: &mut Ui, t: &Tokens) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.line);
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

/// One entry of the home grid: a page shortcut or an addon action.
enum HomeItem {
    Page(Tile, Page, f32),
    Action(Tile, sdw_core::model::Action, f32),
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
        Page::Control => "control",
        Page::Bluetooth => "bluetooth",
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
    let online = app.online();
    let host = app.active_host_name();
    let status = app.status.clone();
    let monitor = monitor_state(app);

    let s = status.clone();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    theme::dot(ui, if online { t.good } else { t.bad }, true);
                    ui.label(RichText::new(&host).size(28.0).strong().color(t.strong));
                });
                let detail = match &s {
                    Some(s) => format!(
                        "{} · {} · viiper {} · {} {}",
                        s.version,
                        if online {
                            t!(app.lang, "связь есть", "connected")
                        } else {
                            t!(app.lang, "нет связи", "offline")
                        },
                        if s.viiper.running { "on" } else { "off" },
                        s.pads.len(),
                        t!(app.lang, "падов", "pads")
                    ),
                    None => t!(
                        app.lang,
                        "компьютер не выбран или выключен",
                        "no computer selected or it is offline"
                    )
                    .to_string(),
                };
                ui.label(
                    RichText::new(detail)
                        .family(FontFamily::Monospace)
                        .size(11.0)
                        .color(t.muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let gp = if app.gamepad_on {
                        t!(app.lang, "выключить геймпад", "gamepad off")
                    } else {
                        t!(app.lang, "включить геймпад", "gamepad on")
                    };
                    if theme::primary(ui, gp, t).clicked() {
                        app.set_gamepad_on(!app.gamepad_on);
                    }
                    let sc = if monitor.running {
                        t!(app.lang, "остановить экран", "screen off")
                    } else {
                        t!(app.lang, "запустить экран", "screen on")
                    };
                    if theme::ghost(ui, sc, t).clicked() {
                        app.toggle_monitor_default();
                    }
                    if theme::ghost(ui, t!(app.lang, "компьютеры", "computers"), t).clicked()
                    {
                        app.set_page(Page::Computers);
                    }
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let mut facts: Vec<(String, Color32)> = Vec::new();
                facts.push((
                    if online {
                        t!(app.lang, "онлайн", "online").to_string()
                    } else {
                        t!(app.lang, "офлайн", "offline").to_string()
                    },
                    if online { t.good } else { t.bad },
                ));
                if let Some(s) = &s {
                    facts.push((
                        format!("{} {}", s.pads.len(), t!(app.lang, "падов", "pads")),
                        t.faint,
                    ));
                }
                facts.push((
                    format!(
                        "{} {}",
                        app.cfg.binds.len(),
                        t!(app.lang, "биндов", "binds")
                    ),
                    t.faint,
                ));
                for (text, color) in facts.into_iter().rev() {
                    ui.label(
                        RichText::new(text)
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(color),
                    );
                    ui.add_space(8.0);
                }
            });
        });
        ui.add_space(10.0);
        hairline(ui, t);
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            let mut items: Vec<(String, String, Color32)> = Vec::new();
            items.push((
                "gamepad".into(),
                if app.gamepad_on {
                    t!(app.lang, "в эфире", "live").to_string()
                } else {
                    t!(app.lang, "выкл", "off").to_string()
                },
                if app.gamepad_on { t.good } else { t.faint },
            ));
            items.push((
                "screen".into(),
                if monitor.running {
                    format!("{}×{}", monitor.width, monitor.height)
                } else {
                    t!(app.lang, "выкл", "off").to_string()
                },
                if monitor.running { t.good } else { t.faint },
            ));
            items.push((
                "viiper".into(),
                s.as_ref()
                    .map(|s| {
                        if s.viiper.running {
                            s.viiper.version.clone()
                        } else {
                            t!(app.lang, "выкл", "off").to_string()
                        }
                    })
                    .unwrap_or_else(|| "—".into()),
                if s.as_ref().map(|s| s.viiper.running).unwrap_or(false) {
                    t.good
                } else {
                    t.faint
                },
            ));
            items.push((
                t!(app.lang, "home assistant", "home assistant").to_string(),
                s.as_ref()
                    .map(|s| {
                        if s.ha.connected {
                            format!("{}", s.ha.entities)
                        } else if s.ha.configured {
                            t!(app.lang, "нет связи", "offline").to_string()
                        } else {
                            t!(app.lang, "не настроен", "not set").to_string()
                        }
                    })
                    .unwrap_or_else(|| "—".into()),
                if s.as_ref().map(|s| s.ha.connected).unwrap_or(false) {
                    t.good
                } else {
                    t.faint
                },
            ));
            items.push((
                t!(app.lang, "уведомления", "notifications").to_string(),
                if app.notif_unread > 0 {
                    app.notif_unread.to_string()
                } else {
                    "0".into()
                },
                if app.notif_unread > 0 {
                    t.warn
                } else {
                    t.faint
                },
            ));
            for (label, value, color) in items {
                stat(ui, t, &label, &value, color);
                ui.add_space(26.0);
            }
        });
    });
    ui.add_space(14.0);

    if cfg!(target_os = "linux") && app.input_devices.is_empty() {
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(
                t!(app.lang, "нет доступа к контролам", "no controller access"),
                t.warn,
            ));
            ui.add_space(4.0);
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "геймпад и экран деки не читаются. добавьте пользователя в группу input и перезапустите приложение.",
                    "the deck's controls cannot be read. add your user to the input group and restart the app."
                ))
                .size(13.5)
                .color(t.muted),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("sudo usermod -aG input $USER")
                        .family(FontFamily::Monospace)
                        .size(12.5)
                        .color(t.strong),
                );
                if theme::ghost(ui, t!(app.lang, "копировать", "copy"), t).clicked() {
                    ui.ctx().copy_text("sudo usermod -aG input $USER".into());
                    app.toast(t!(app.lang, "команда скопирована", "command copied"), true);
                }
            });
            for e in &app.input_errors {
                ui.label(
                    RichText::new(crate::input::error_text(app.lang, e))
                        .size(12.0)
                        .color(t.bad),
                );
            }
        });
        ui.add_space(14.0);
    }

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

    // The grid: big landscape tiles on a deck-sized screen. Rows are chunked by
    // hand instead of using `horizontal_wrapped`: with wrapping, a rounding
    // error of a few tenths of a point makes the third tile drop onto its own
    // line, which is exactly what the Deck must never show.
    let st = theme::style();
    let width = ui.available_width();
    let cols = if crate::app::deck_mode() {
        3
    } else {
        st.tile_columns.clamp(1, 4) as usize
    };
    let cols = if width < 620.0 { cols.min(2) } else { cols };
    let gap = 14.0;
    let tile_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).max(140.0);
    let tile_h = st.tile_height.clamp(120.0, 300.0);
    let size = Vec2::new(tile_w, tile_h);

    let mut items: Vec<HomeItem> = Vec::new();

    if app.tile_shown("gamepad") {
        items.push(HomeItem::Page(
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
            Page::Gamepad,
            0.0,
        ));
    }
    if app.tile_shown("screen") {
        items.push(HomeItem::Page(
            Tile {
                title: t!(app.lang, "экран", "screen").into(),
                subtitle: if monitor.running {
                    format!(
                        "{}×{} · {}",
                        monitor.width, monitor.height, monitor.backend_label
                    )
                } else {
                    t!(app.lang, "дополнительный монитор", "extra monitor").into()
                },
                glyph: theme::icon::SCREEN.into(),
                accent: Color32::from_rgb(0x3d, 0xd6, 0xcf),
                badge: Some(if monitor.running {
                    "live".into()
                } else {
                    "off".into()
                }),
                badge_color: if monitor.running { t.good } else { t.faint },
                dim: !monitor.running,
                reveal: 1.0,
            },
            Page::Screen,
            1.0,
        ));
    }
    if app.tile_shown("notifications") {
        items.push(HomeItem::Page(
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
            Page::Notifications,
            2.0,
        ));
    }
    if app.tile_shown("timers") {
        let running = app.timers.tile_status(app.lang);
        items.push(HomeItem::Page(
            Tile {
                title: t!(app.lang, "таймеры", "timers").into(),
                subtitle: running.clone().unwrap_or_else(|| {
                    t!(
                        app.lang,
                        "отсчёт, секундомер и помодоро",
                        "countdown, stopwatch and pomodoro"
                    )
                    .into()
                }),
                glyph: theme::icon::TIMER.into(),
                accent: Color32::from_rgb(0xff, 0xb3, 0x4d),
                badge: running
                    .is_some()
                    .then(|| t!(app.lang, "идёт", "running").into()),
                badge_color: t.accent,
                dim: running.is_none(),
                reveal: 1.0,
            },
            Page::Timers,
            2.5,
        ));
    }
    if app.tile_shown("ha") {
        items.push(HomeItem::Page(
            Tile {
                title: t!(app.lang, "home assistant", "home assistant").into(),
                subtitle: status
                    .as_ref()
                    .map(|s| {
                        if s.ha.connected {
                            format!(
                                "{} {}",
                                s.ha.entities,
                                t!(app.lang, "сущностей", "entities")
                            )
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
            Page::HomeAssistant,
            3.0,
        ));
    }
    if app.tile_shown("binds") {
        items.push(HomeItem::Page(
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
            Page::Binds,
            4.0,
        ));
    }
    if app.tile_shown("control") {
        items.push(HomeItem::Page(
            Tile {
                title: t!(app.lang, "пульт", "control").into(),
                subtitle: t!(
                    app.lang,
                    "питание и приложения пк",
                    "pc power and applications"
                )
                .into(),
                glyph: theme::icon::POWER.into(),
                accent: Color32::from_rgb(0xa9, 0x8b, 0xff),
                badge: None,
                badge_color: t.accent,
                dim: status.is_none(),
                reveal: 1.0,
            },
            Page::Control,
            4.5,
        ));
    }
    if app.tile_shown("bluetooth") {
        items.push(HomeItem::Page(
            Tile {
                title: t!(app.lang, "bluetooth", "bluetooth").into(),
                subtitle: t!(
                    app.lang,
                    "наушники и устройства пк",
                    "headphones and pc devices"
                )
                .into(),
                glyph: theme::icon::BLUETOOTH.into(),
                accent: Color32::from_rgb(0x53, 0x8b, 0xff),
                badge: None,
                badge_color: t.accent,
                dim: app.bluetooth.as_ref().map(|b| !b.available).unwrap_or(true),
                reveal: 1.0,
            },
            Page::Bluetooth,
            4.7,
        ));
    }
    if app.tile_shown("computers") {
        items.push(HomeItem::Page(
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
            Page::Computers,
            5.0,
        ));
    }

    // Tiles contributed by enabled addons. They run any action, local or remote.
    let addons = app.cfg.addons.clone();
    let mut index = 6.0;
    for addon in sdw_core::addons::list() {
        if !addons.is_on(addon.id()) {
            continue;
        }
        let vars = sdw_core::addons::variables(&addons, addon.id());
        for tile in &addon.manifest.deck.tiles {
            let color = theme::parse_color(&tile.accent).unwrap_or(t.accent);
            let glyph = if tile.icon.trim().is_empty() {
                theme::icon::ADDON.to_string()
            } else {
                tile.icon.clone()
            };
            items.push(HomeItem::Action(
                Tile {
                    title: tile.title.clone(),
                    subtitle: tile.subtitle.clone(),
                    glyph,
                    accent: color,
                    badge: Some(addon.manifest.name.clone()),
                    badge_color: color,
                    dim: false,
                    reveal: 1.0,
                },
                sdw_core::addons::expand_action(&tile.action, &vars),
                index,
            ));
            index += 1.0;
        }
    }

    for row in items.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for item in row {
                match item {
                    HomeItem::Page(tile, page, index) => {
                        quick_action(app, ui, t, tile.clone(), size, *page, *index);
                    }
                    HomeItem::Action(tile, action, index) => {
                        let mut tile = tile.clone();
                        tile.reveal = app.reveal(*index);
                        if tile.reveal < 1.0 {
                            ui.ctx().request_repaint();
                        }
                        if theme::tile(ui, t, &tile, size).clicked() {
                            app.execute_action(action);
                        }
                    }
                }
            }
        });
        ui.add_space(gap);
    }

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
        let recent: Vec<Notification> = app.notifications.iter().rev().take(3).cloned().collect();
        for n in recent {
            let color = notification_color(t, &n);
            let app_name = notification_line(if n.app.is_empty() { "system" } else { &n.app }, 40);
            ui.horizontal(|ui| {
                theme::dot(ui, color, true);
                ui.label(
                    RichText::new(&app_name)
                        .family(FontFamily::Monospace)
                        .size(10.5)
                        .color(color),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(notification_line(&n.summary, 72))
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

pub fn clock(app: &mut App, ui: &mut Ui, t: &Tokens) {
    crate::clock::ui(app, ui, t);
}

pub fn audio(app: &mut App, ui: &mut Ui, t: &Tokens) {
    crate::audio_ui::page(app, ui, t);
}

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
        if theme::ghost(ui, t!(app.lang, "искать в сети", "scan the network"), t).clicked()
        {
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
        let reachable = is_active && app.online();
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
                                        pad.kind.label_lang(app.lang)
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
                            if theme::ghost(ui, t!(app.lang, "забыть", "forget"), t).clicked()
                            {
                                forget = Some(host.id.clone());
                            }
                            theme::chip(ui, t!(app.lang, "выбран", "selected"), t.accent);
                        } else if theme::primary(ui, t!(app.lang, "переключиться", "switch"), t)
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
        app.toast(t!(app.lang, "компьютер забыт", "computer forgotten"), true);
    }

    if !app.discovery.is_empty() {
        ui.add_space(4.0);
        ui.label(theme::hud(
            t!(app.lang, "найдено в сети", "found on the network"),
            t.faint,
        ));
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
                            ui.label(RichText::new(&d.name).size(15.0).strong().color(t.strong));
                            ui.label(
                                RichText::new(format!("{base} · sd-widgets {}", d.version))
                                    .family(FontFamily::Monospace)
                                    .size(11.5)
                                    .color(t.muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if already {
                                theme::chip(
                                    ui,
                                    t!(app.lang, "уже добавлен", "already added"),
                                    t.faint,
                                );
                            } else if theme::ghost(ui, t!(app.lang, "выбрать", "choose"), t)
                                .clicked()
                            {
                                app.pair_url = base;
                                app.pair_name = d.name.clone();
                                app.add_open = true;
                                app.pair_error.clear();
                            }
                            if !d.setup_done {
                                theme::chip(
                                    ui,
                                    t!(app.lang, "нужна настройка", "setup needed"),
                                    t.warn,
                                );
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
                RichText::new(t!(
                    app.lang,
                    "подключение по паролю",
                    "pair with a password"
                ))
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
                ui.label(
                    RichText::new(t!(app.lang, "адрес", "address"))
                        .size(13.0)
                        .color(t.muted),
                );
                ui.add(
                    TextEdit::singleline(&mut app.pair_url)
                        .hint_text("http://192.168.1.20:47830")
                        .desired_width(320.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(app.lang, "имя", "name"))
                        .size(13.0)
                        .color(t.muted),
                );
                ui.add(
                    TextEdit::singleline(&mut app.pair_name)
                        .hint_text(app.cfg.device_name.clone())
                        .desired_width(240.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(app.lang, "пароль", "password"))
                        .size(13.0)
                        .color(t.muted),
                );
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
                } else if theme::primary(ui, t!(app.lang, "подключить", "connect"), t).clicked()
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
                ui.label(
                    RichText::new(&app.cfg.device_name)
                        .size(16.0)
                        .color(t.strong),
                );
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

/// Power and applications of the active computer.
///
/// Everything here goes through declared commands: the deck sends an action
/// name or an application id discovered by the daemon, never a command line.
pub fn control(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    page_title(
        ui,
        t,
        t!(lang, "пульт", "control"),
        t!(
            lang,
            "питание, приложения и экран компьютера — с деки.",
            "power, applications and the screen of the computer from the deck."
        ),
    );

    let online = app.online();
    let stale = app
        .control_apps_at
        .map(|at| at.elapsed() > Duration::from_secs(90))
        .unwrap_or(true);
    if online && stale {
        app.load_control_apps();
    }

    let mut run: Option<PowerAction> = None;
    let mut ask: Option<PowerAction> = None;
    let mut cancel = false;
    theme::card(ui, t, |ui| {
        theme::row_ends(
            ui,
            |ui| {
                ui.label(theme::hud(t!(lang, "питание", "power"), t.faint));
            },
            |ui| {
                if !online {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "нет связи с компьютером",
                            "the computer is offline"
                        ))
                        .size(12.0)
                        .color(t.warn),
                    );
                }
            },
        );
        ui.add_space(6.0);

        if let Some(action) = app.power_confirm {
            ui.label(
                RichText::new(format!(
                    "{} {}?",
                    t!(lang, "точно", "really"),
                    power_name(lang, action)
                ))
                .size(17.0)
                .strong()
                .color(t.strong),
            );
            ui.horizontal(|ui| {
                if theme::primary(ui, t!(lang, "да, выполнить", "yes, do it"), t).clicked()
                {
                    run = Some(action);
                }
                if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
                    cancel = true;
                }
            });
        } else {
            ui.horizontal_top(|ui| {
                let count = PowerAction::ALL.len() as f32;
                // The slots run without gaps and centre their own contents:
                // five slots plus four gaps rounded up past the card and the
                // border jumped out by a dozen pixels.
                ui.spacing_mut().item_spacing.x = 0.0;
                let width = (ui.available_width() / count).floor().max(104.0);
                for action in PowerAction::ALL {
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        Layout::top_down(Align::Center),
                        |ui| {
                            let color = if action == PowerAction::Shutdown {
                                t.bad
                            } else {
                                t.muted
                            };
                            ui.label(theme::icon(power_glyph(action), 24.0, color));
                            ui.add_space(2.0);
                            if theme::ghost(ui, power_name(lang, action), t).clicked() {
                                if action.destructive() {
                                    ask = Some(action);
                                } else {
                                    run = Some(action);
                                }
                            }
                        },
                    );
                }
            });
        }
    });

    ui.add_space(10.0);

    let mut refresh = false;
    let mut launched: Option<(String, String)> = None;
    theme::card(ui, t, |ui| {
        theme::row_ends(
            ui,
            |ui| {
                ui.label(theme::hud(t!(lang, "приложения", "applications"), t.faint));
                if app.control_apps_pending {
                    ui.spinner();
                }
            },
            |ui| {
                if theme::ghost(ui, t!(lang, "обновить", "refresh"), t).clicked() {
                    refresh = true;
                }
            },
        );
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut app.control_filter)
                    .hint_text(t!(lang, "поиск приложения", "search applications"))
                    .desired_width(360.0),
            );
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    app.control_apps.len(),
                    t!(lang, "на компьютере", "on the computer")
                ))
                .family(FontFamily::Monospace)
                .size(11.0)
                .color(t.faint),
            );
        });
        if !app.control_apps_err.is_empty() {
            ui.label(
                RichText::new(util::truncate(&app.control_apps_err, 220))
                    .size(12.0)
                    .color(t.warn),
            );
        }
        if app.control_apps.is_empty() {
            if !app.control_apps_pending && app.control_apps_err.is_empty() {
                theme::small(
                    ui,
                    t!(
                        lang,
                        "на компьютере не нашлось приложений с ярлыками.",
                        "no applications with shortcuts were found on the computer."
                    ),
                    t.muted,
                );
            }
            return;
        }

        let needle = app.control_filter.trim().to_lowercase();
        let list: Vec<AppEntry> = app
            .control_apps
            .iter()
            .filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.comment.to_lowercase().contains(&needle)
                    || a.source.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        if list.is_empty() {
            theme::small(ui, t!(lang, "ничего не найдено", "nothing found"), t.muted);
            return;
        }
        // Pictures come from the computer on demand; the list itself is
        // already ordered by how often the user starts each program there.
        app.want_app_icons(&list);
        app.ensure_app_icons(ui.ctx());
        let cols = if ui.available_width() >= 760.0 { 2 } else { 1 };
        let width = ((ui.available_width() - 8.0 * (cols as f32 - 1.0)) / cols as f32).max(220.0);
        for row in list.chunks(cols) {
            ui.horizontal_top(|ui| {
                for entry in row {
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            egui::Frame::new()
                                .fill(t.well)
                                .stroke(Stroke::new(1.0, t.line))
                                .corner_radius(CornerRadius::ZERO)
                                .inner_margin(Margin::symmetric(12, 9))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let glyph = match entry.kind {
                                            AppKind::Terminal => theme::icon::TERMINAL,
                                            AppKind::Gui => theme::icon::WIDGETS,
                                        };
                                        // The real icon when the computer has
                                        // one; the Grid glyph otherwise.
                                        match app.app_icons.get(&entry.id) {
                                            Some(tex) => {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    Vec2::splat(18.0),
                                                    Sense::hover(),
                                                );
                                                let uv = egui::Rect::from_min_max(
                                                    egui::pos2(0.0, 0.0),
                                                    egui::pos2(1.0, 1.0),
                                                );
                                                ui.painter().image(
                                                    tex.id(),
                                                    rect,
                                                    uv,
                                                    Color32::WHITE,
                                                );
                                            }
                                            None => {
                                                ui.label(theme::icon(glyph, 18.0, t.accent));
                                            }
                                        }
                                        ui.vertical(|ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&entry.name)
                                                        .size(14.5)
                                                        .strong()
                                                        .color(t.strong),
                                                )
                                                .truncate(),
                                            );
                                            if !entry.comment.is_empty() {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(util::truncate(
                                                            &entry.comment,
                                                            72,
                                                        ))
                                                        .size(11.5)
                                                        .color(t.muted),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                            if !entry.source.is_empty() {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(util::truncate(
                                                            &entry.source,
                                                            48,
                                                        ))
                                                        .family(FontFamily::Monospace)
                                                        .size(10.0)
                                                        .color(t.faint),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if theme::ghost(
                                                    ui,
                                                    t!(lang, "запустить", "start"),
                                                    t,
                                                )
                                                .clicked()
                                                {
                                                    launched = Some((
                                                        entry.id.clone(),
                                                        entry.name.clone(),
                                                    ));
                                                }
                                            },
                                        );
                                    });
                                });
                        },
                    );
                }
            });
            ui.add_space(4.0);
        }
    });

    if let Some(action) = run {
        app.power_confirm = None;
        app.remote_power(action);
    }
    if let Some(action) = ask {
        app.power_confirm = Some(action);
    }
    if cancel {
        app.power_confirm = None;
    }
    if refresh {
        app.control_apps_at = None;
        app.load_control_apps();
    }
    if let Some((id, name)) = launched {
        app.launch_app(id, name);
    }
}

/// The pull-down panel: notifications, then the Deck's own sound and light.
///
/// It is an overlay over the current page and leaves with a slide, the handle,
/// the close button, a tap outside or B.
pub fn quick_panel(app: &mut App, ctx: &egui::Context, t: &Tokens) {
    let lang = app.lang;
    // `quick_off` is the whole state of the slide: a pull from the top, a push
    // up while it is open and the settle after a release all move this one
    // number, so the panel follows the finger and nothing is animated twice.
    let height = app.quick_height.max(1.0);
    let off = app.quick_off;
    if !app.quick_open && off >= height - 0.5 {
        return;
    }
    let reveal = (1.0 - off / height).clamp(0.0, 1.0);
    let rect = ctx.content_rect();
    let width = (rect.width() - 24.0).clamp(320.0, 980.0);
    let pos = egui::pos2(rect.center().x - width / 2.0, rect.top() + 6.0 - off);
    let mut close = false;
    let mut shade_clicked = false;

    let mut jump_notif = false;
    let mut dismiss: Option<u64> = None;
    let mut activate: Option<u64> = None;
    // The panel is a modal: the dim layer is drawn inside the same egui layer
    // as the panel, and a tap on it arrives as the modal's backdrop. Two areas
    // at the same order (a separate shade and the panel) were a trap: pressing
    // the shade moved it above the panel for good, and from then on every press
    // inside the panel went to the shade and closed it instead of reaching a
    // button — exactly how a working "read all" looked dead. A modal's backdrop
    // can never get above its own content, and it also keeps the dock, the
    // arrows and the page behind out of reach while the panel is up.
    let frame = egui::Frame::new()
        .fill(if t.dark {
            Color32::from_rgba_premultiplied(0x12, 0x18, 0x20, (252.0 * reveal) as u8)
        } else {
            Color32::from_rgba_premultiplied(0xf7, 0xf9, 0xfc, (252.0 * reveal) as u8)
        })
        .stroke(Stroke::new(
            1.0,
            t.line_strong.gamma_multiply(reveal.max(0.15)),
        ))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(18, 12));
    let area = egui::Area::new(egui::Id::new("deck-quick-panel"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        // The panel has to leave the screen when it is pulled up: with the
        // default constraint egui pinned it to the top edge and only the
        // fade was left, which is why the close looked like a snap.
        .constrain(false)
        // The finger drags the panel through the raw pointer stream, not
        // through egui: the area itself must not sense clicks.
        .sense(egui::Sense::hover());
    let panel = egui::Modal::new(egui::Id::new("deck-quick-panel"))
        .area(area)
        // The dim fades out with the panel instead of vanishing on the last
        // frame of the close.
        .backdrop_color(Color32::from_black_alpha((96.0 * reveal).round() as u8))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(width - 36.0);
            ui.horizontal(|ui| {
                ui.label(theme::hud(
                    t!(lang, "центр управления", "control center"),
                    t.faint,
                ));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(ui, theme::icon::CLOSE, t, 26.0, false).clicked() {
                        close = true;
                    }
                    if theme::icon_button(ui, theme::icon::NOTIFICATIONS, t, 26.0, false).clicked()
                    {
                        jump_notif = true;
                    }
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "стяните вниз сверху экрана",
                            "drag down from the top"
                        ))
                        .size(11.0)
                        .color(t.faint),
                    );
                });
            });
            ui.add_space(4.0);

            if app.cfg.viz.enabled && app.cfg.viz.shade {
                crate::viz::ui(
                    ui,
                    app,
                    t,
                    crate::viz::size(app, Some(Vec2::new(ui.available_width().min(420.0), 60.0))),
                );
                ui.add_space(6.0);
            }

            // The Deck's own sound and light.
            if let Some(level) = app.quick_volume {
                let mut volume = level as f32;
                let muted = app.quick_muted;
                let (_, released) = theme::slider_ex(
                    ui,
                    t,
                    t!(lang, "громкость деки", "deck volume"),
                    &mut volume,
                    0.0..=100.0,
                    |x| {
                        if muted {
                            t!(lang, "выкл", "mute").to_string()
                        } else {
                            format!("{}%", x.round() as u8)
                        }
                    },
                );
                if released {
                    app.quick_set_volume(volume.round() as u8);
                } else if volume.round() as u8 != level {
                    app.quick_volume = Some(volume.round() as u8);
                }
            } else {
                theme::small(
                    ui,
                    t!(
                        lang,
                        "на деке не найден звуковой сервер (wpctl или pactl).",
                        "no sound server was found on the deck (wpctl or pactl)."
                    ),
                    t.muted,
                );
            }

            if let Some(level) = app.quick_brightness {
                let mut value = level as f32;
                let (_, released) = theme::slider_ex(
                    ui,
                    t,
                    t!(lang, "яркость", "brightness"),
                    &mut value,
                    1.0..=100.0,
                    |x| format!("{}%", x.round() as u8),
                );
                if released {
                    app.quick_set_brightness(value.round() as u8);
                } else if value.round() as u8 != level {
                    app.quick_brightness = Some(value.round() as u8);
                }
            }

            // The deck's screen going to the computer is a switch here,
            // not a hidden setting: one press stops the share and the
            // sound, and the daemon's page goes dark.
            ui.horizontal(|ui| {
                let mut share = app.cfg.mirror_deck;
                if theme::switch(ui, &mut share, t).changed() {
                    app.cfg.mirror_deck = share;
                    app.save();
                    if !share {
                        app.stop_share();
                    }
                }
                ui.label(
                    RichText::new(t!(
                        lang,
                        "трансляция экрана на пк",
                        "share this screen with the pc"
                    ))
                    .size(13.0)
                    .color(t.muted),
                );
            });
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let muted = app.quick_muted;
                let glyph = if muted {
                    theme::icon::VOLUME_OFF
                } else {
                    theme::icon::VOLUME_UP
                };
                if theme::icon_button(ui, glyph, t, 28.0, muted).clicked() {
                    app.quick_toggle_mute();
                }
                if theme::ghost(ui, t!(lang, "убавить", "quieter"), t).clicked() {
                    let next = app.quick_volume.unwrap_or(30).saturating_sub(5);
                    app.quick_set_volume(next);
                }
                if theme::ghost(ui, t!(lang, "прибавить", "louder"), t).clicked() {
                    let next = (app.quick_volume.unwrap_or(30) + 5).min(100);
                    app.quick_set_volume(next);
                }
            });

            ui.add_space(6.0);
            // The notification actions sit on their own row. In the
            // volume row a large font pushed them past the panel edge,
            // and a widget clipped by its parent can no longer be
            // pressed at all — which is exactly how "read all" looked
            // broken. A wrapped row never clips.
            ui.horizontal_wrapped(|ui| {
                if app.notif_unread > 0
                    && theme::ghost(ui, t!(lang, "прочитать все", "read all"), t).clicked()
                {
                    app.read_all_notifs();
                }
                if theme::ghost(ui, t!(lang, "все уведомления", "all notifications"), t).clicked()
                {
                    jump_notif = true;
                }
            });
            if app.notifications.is_empty() {
                theme::small(ui, t!(lang, "уведомлений нет", "no notifications"), t.muted);
                return;
            }
            // The drawer is the unread shade, not a second copy of the page:
            // once something is read it leaves this list. "Read all" therefore
            // empties the drawer for real instead of leaving the same cards
            // behind, only dimmer, which is what made it look like a trick.
            if app.notif_unread == 0 {
                theme::small(
                    ui,
                    t!(lang, "все уведомления прочитаны", "all notifications read"),
                    t.muted,
                );
                return;
            }
            ui.label(theme::hud(
                &format!(
                    "{} · {}",
                    t!(lang, "уведомления", "notifications"),
                    app.notif_unread
                ),
                t.faint,
            ));
            ui.add_space(2.0);
            // How many cards fit between here and the bottom edge.
            // At a larger UI scale the screen holds far fewer points,
            // and a fixed four pushed the buttons under the edge:
            // the panel could not be used at all. The list answers
            // to the room that is really left, measured from the
            // cards the last frame drew.
            let list_top = ui.cursor().top();
            // Eight points to the screen edge and twelve of the frame's
            // own bottom margin stay free, so a row never ends flush
            // against the edge.
            let room = (rect.bottom() - 20.0 - list_top).max(0.0);
            let card_h = app.quick_card_h.clamp(48.0, 240.0);
            let rows = ((room / card_h).floor() as usize).clamp(1, 4);
            let newest: Vec<Notification> = app
                .notifications
                .iter()
                .rev()
                .filter(|n| !app.notif_seen(n.id))
                .take(rows)
                .cloned()
                .collect();
            let mut drawn = 0usize;
            let mut span_top = list_top;
            let mut span_bottom = list_top;
            for n in newest {
                let color = notification_color(t, &n);
                let (off, alpha) = app.notif_slide(n.id);
                let leaving = app.notif_leaving(n.id);
                let mut close_rect: Option<Rect> = None;
                let card = notification_slot(ui, n.id, "deck-quick-slot", off, alpha, |ui| {
                    egui::Frame::new()
                        .fill(t.card)
                        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
                        .corner_radius(CornerRadius::ZERO)
                        .inner_margin(Margin::symmetric(14, 11))
                        // A tap anywhere on the card opens it on the
                        // computer, the way clicking it there would.
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(theme::icon(notif_glyph(&n.app, &n.icon), 20.0, color));
                                ui.add_space(3.0);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(if n.app.is_empty() {
                                                t!(lang, "уведомление", "notification")
                                            } else {
                                                &n.app
                                            })
                                            .size(10.5)
                                            .strong()
                                            .color(color),
                                        );
                                        unread_marker(ui, t);
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                let close = theme::icon_button(
                                                    ui,
                                                    theme::icon::CLOSE,
                                                    t,
                                                    22.0,
                                                    false,
                                                );
                                                close_rect = Some(close.rect);
                                                if close.clicked() {
                                                    dismiss = Some(n.id);
                                                }
                                                ui.label(
                                                    RichText::new(age(n.ts, lang))
                                                        .family(FontFamily::Monospace)
                                                        .size(10.0)
                                                        .color(t.faint),
                                                );
                                            },
                                        );
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(if n.summary.is_empty() {
                                                &n.app
                                            } else {
                                                &n.summary
                                            })
                                            .size(15.0)
                                            .strong()
                                            .color(t.strong),
                                        )
                                        .truncate(),
                                    );
                                    if !n.body.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(util::truncate(&n.body, 160))
                                                    .size(12.0)
                                                    .color(t.muted),
                                            )
                                            .wrap(),
                                        );
                                    }
                                });
                            });
                        })
                        .response
                });
                if leaving {
                    // The card is already sliding out: it takes no taps.
                    if drawn == 0 {
                        span_top = card.rect.top();
                    }
                    span_bottom = card.rect.bottom();
                    drawn += 1;
                    ui.add_space(4.0);
                    continue;
                }
                app.notif_rows.push((n.id, card.rect));
                let hit = ui.interact(
                    card.rect,
                    egui::Id::new(("deck-quick-notif", n.id)),
                    Sense::click(),
                );
                if hit.clicked() {
                    // The card's click area sits over the close button;
                    // the button keeps its meaning by where the finger
                    // landed.
                    let on_close = hit
                        .interact_pointer_pos()
                        .is_some_and(|pos| close_rect.is_some_and(|r| r.contains(pos)));
                    if on_close {
                        dismiss = Some(n.id);
                    } else {
                        activate = Some(n.id);
                    }
                }
                if hit.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                hit.on_hover_text(t!(
                    lang,
                    "нажмите, чтобы открыть на компьютере",
                    "tap to open it on the computer"
                ));
                if drawn == 0 {
                    span_top = card.rect.top();
                }
                span_bottom = card.rect.bottom();
                drawn += 1;
                ui.add_space(4.0);
            }
            // Remember the real card height for the next opening: text
            // wraps differently in every language and at every width,
            // so a constant would be wrong somewhere.
            if drawn > 0 {
                app.quick_card_seen = (span_bottom - span_top + 4.0) / drawn as f32;
            }
        });
    if panel.backdrop_response.clicked() {
        close = true;
        shade_clicked = true;
    }
    // How far the panel has to rise to be fully out of sight, plus the 6 px
    // it sits below the top edge when open. A drag clamps to it and the settle
    // closes over it.
    app.quick_height = panel.response.rect.height() + 6.0;

    if shade_clicked {
        app.quick_clicked = true;
    }
    if ctx.input(|i| i.pointer.any_click())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|pos| panel.response.rect.contains(pos))
    {
        app.quick_clicked = true;
    }

    if let Some(id) = dismiss {
        app.dismiss_notif(id);
    }
    if let Some(id) = activate {
        app.activate_notif(id);
        app.set_quick(false);
    }
    if jump_notif {
        app.set_quick(false);
        app.set_page(Page::Notifications);
    } else if close {
        app.set_quick(false);
    }
}

/// Bluetooth devices on the computer, with headphone battery.
///
/// Pairing, connecting and forgetting are declared actions; the daemon checks
/// the address before it reaches `bluetoothctl`.
pub fn bluetooth(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    page_title(
        ui,
        t,
        t!(lang, "bluetooth", "bluetooth"),
        t!(
            lang,
            "наушники, клавиатуры и другие устройства компьютера.",
            "headphones, keyboards and other devices of the computer."
        ),
    );

    let online = app.online();
    // While BlueZ is scanning, new devices appear all the time: poll quickly so
    // the list grows under the finger instead of after a twenty-second wait.
    let discovering = app
        .bluetooth
        .as_ref()
        .map(|b| b.discovering)
        .unwrap_or(false);
    // A press must show up at once, so the page keeps a short poll: a
    // connected headphone, a scan that finished or a device that dropped out
    // reaches the list while the page is open instead of after a wait. While
    // an action runs it polls every second, so "paired" and "connected" appear
    // as BlueZ reaches each step instead of only at the end.
    let busy = app.bluetooth_busy.is_some();
    let every = if busy {
        1
    } else if discovering {
        2
    } else {
        4
    };
    let stale = app
        .bluetooth_at
        .map(|at| at.elapsed() > Duration::from_secs(every))
        .unwrap_or(true);
    if online && stale {
        app.load_bluetooth();
    }
    ui.ctx()
        .request_repaint_after(Duration::from_secs(if busy || discovering { 1 } else { 2 }));

    let status = app.bluetooth.clone();
    let mut action: Option<(&'static str, String)> = None;
    let mut remove_ask: Option<String> = None;
    // An action in flight: its row spins and its state has already changed, so
    // a press never looks ignored while BlueZ pairs.
    let busy = app.bluetooth_busy.clone();
    let busy_for = |name: &str, address: &str| {
        busy.as_ref()
            .is_some_and(|(a, addr)| a == name && addr == address)
    };

    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(lang, "адаптер", "adapter"), t.faint));
            if app.bluetooth_pending || busy.is_some() {
                ui.spinner();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(lang, "обновить", "refresh"), t).clicked() {
                    app.bluetooth_at = None;
                    app.load_bluetooth();
                }
            });
        });
        match &status {
            Some(bt) if bt.available => {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(&bt.adapter)
                            .family(FontFamily::Monospace)
                            .size(12.0)
                            .color(t.muted),
                    );
                    theme::chip(
                        ui,
                        if bt.powered {
                            t!(lang, "включён", "on")
                        } else {
                            t!(lang, "выключен", "off")
                        },
                        if bt.powered { t.good } else { t.faint },
                    );
                    if bt.discovering {
                        theme::chip(ui, t!(lang, "идёт поиск", "scanning"), t.accent);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    // One scan button that knows whether it starts or stops:
                    // a second press must not pile up another twelve-second
                    // scan on top of the running one.
                    let scan_label = if bt.discovering {
                        t!(lang, "остановить поиск", "stop scanning")
                    } else {
                        t!(lang, "искать устройства", "scan for devices")
                    };
                    let scan = if bt.discovering {
                        theme::ghost(ui, scan_label, t)
                    } else {
                        theme::primary(ui, scan_label, t)
                    };
                    if scan.clicked() {
                        action = Some((
                            if bt.discovering { "scan_off" } else { "scan" },
                            String::new(),
                        ));
                    }
                    let power = if bt.powered {
                        (
                            t!(lang, "выключить bluetooth", "bluetooth off"),
                            "power_off",
                        )
                    } else {
                        (t!(lang, "включить bluetooth", "bluetooth on"), "power_on")
                    };
                    if theme::ghost(ui, power.0, t).clicked() {
                        action = Some((power.1, String::new()));
                    }
                    if theme::ghost(ui, t!(lang, "перезагрузить", "power cycle"), t).clicked()
                    {
                        action = Some(("power_cycle", String::new()));
                    }
                    if busy_for("scan", "")
                        || busy_for("scan_off", "")
                        || busy_for("power_on", "")
                        || busy_for("power_off", "")
                        || busy_for("power_cycle", "")
                    {
                        ui.spinner();
                    }
                });
                if bt.discovering {
                    theme::small(
                        ui,
                        t!(
                            lang,
                            "держите устройство рядом и включите на нём режим сопряжения — оно появится в списке само.",
                            "hold the device nearby and put it in pairing mode — it will appear in the list by itself."
                        ),
                        t.faint,
                    );
                }
            }
            _ => {
                theme::small(
                    ui,
                    t!(
                        lang,
                        "bluetooth на компьютере недоступен; проверьте bluez и адаптер.",
                        "bluetooth is unavailable on the computer; check bluez and the adapter."
                    ),
                    t.muted,
                );
            }
        }
        if !online {
            theme::small(
                ui,
                t!(
                    lang,
                    "нет связи с компьютером — это список с прошлого раза.",
                    "no connection to the computer — this is the last list."
                ),
                t.muted,
            );
        } else if !app.bluetooth_err.is_empty() {
            // A BlueZ hiccup is a note, not a red alarm: the list is still
            // useful and the next poll usually fixes it.
            ui.label(
                RichText::new(util::truncate(&app.bluetooth_err, 220))
                    .size(12.0)
                    .color(t.muted),
            );
        }
    });

    let devices = status.map(|b| b.devices).unwrap_or_default();
    if devices.is_empty() {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            theme::small(
                ui,
                t!(
                    lang,
                    "устройств пока нет. включите bluetooth и нажмите «искать устройства».",
                    "no devices yet. turn bluetooth on and press “scan for devices”."
                ),
                t.muted,
            );
        });
    }

    for d in devices {
        ui.add_space(8.0);
        let remove_armed = app.bluetooth_remove.as_deref() == Some(d.address.as_str());
        let name = if d.name.is_empty() {
            d.address.clone()
        } else {
            d.name.clone()
        };
        // The BlueZ icon names the kind of device; the fallback is the plain
        // Bluetooth mark so an unknown device still looks like a device.
        let glyph = if d.icon.starts_with("audio-") {
            theme::icon::HEADPHONES
        } else if d.icon.contains("keyboard") {
            theme::icon::KEYBOARD
        } else if d.icon.contains("mouse") || d.icon.contains("pointing") {
            theme::icon::MOUSE
        } else {
            theme::icon::BLUETOOTH
        };
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(
                1.0,
                if d.connected {
                    t.good.gamma_multiply(0.45)
                } else {
                    t.line
                },
            ))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(
                        glyph,
                        24.0,
                        if d.connected {
                            t.good
                        } else if d.available {
                            t.muted
                        } else {
                            t.faint
                        },
                    ));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&name).size(16.0).strong().color(t.strong));
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&d.address)
                                    .family(FontFamily::Monospace)
                                    .size(10.5)
                                    .color(t.faint),
                            );
                            if d.connected {
                                theme::chip(ui, t!(lang, "подключено", "connected"), t.good);
                            } else if d.paired {
                                theme::chip(ui, t!(lang, "спарено", "paired"), t.accent);
                            }
                            if !d.available && !d.connected {
                                theme::chip(
                                    ui,
                                    t!(lang, "недоступно", "unavailable"),
                                    t.faint,
                                );
                            }
                            if let Some(pct) = d.battery {
                                let color = if pct >= 30 { t.good } else { t.warn };
                                theme::chip(ui, &format!("{pct}%"), color);
                            }
                            // A nearby device that is not paired yet lives by
                            // its signal: showing it tells "there it is, move
                            // closer" without a second screen.
                            if busy.as_ref().is_some_and(|(_, addr)| addr == &d.address) {
                                ui.spinner();
                                theme::chip(
                                    ui,
                                    t!(lang, "выполняется…", "working…"),
                                    t.accent,
                                );
                            }
                            if let Some(rssi) = d.rssi.filter(|_| !d.connected) {
                                ui.label(theme::icon(
                                    theme::icon::SIGNAL,
                                    13.0,
                                    if rssi >= -70 { t.muted } else { t.faint },
                                ));
                                ui.label(
                                    RichText::new(format!("{rssi} dBm"))
                                        .family(FontFamily::Monospace)
                                        .size(10.5)
                                        .color(t.faint),
                                );
                            }
                        });
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if remove_armed {
                            if theme::primary(ui, t!(lang, "точно удалить", "remove it"), t)
                                .clicked()
                            {
                                action = Some(("remove", d.address.clone()));
                            }
                            if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
                                remove_ask = Some(String::new());
                            }
                        } else if d.connected {
                            if theme::ghost(ui, t!(lang, "отключить", "disconnect"), t)
                                .clicked()
                            {
                                action = Some(("disconnect", d.address.clone()));
                            }
                            if theme::ghost(ui, t!(lang, "удалить", "remove"), t).clicked() {
                                remove_ask = Some(d.address.clone());
                            }
                        } else if d.blocked {
                            if theme::ghost(ui, t!(lang, "удалить", "remove"), t).clicked() {
                                remove_ask = Some(d.address.clone());
                            }
                        } else {
                            // The action a person actually wants sits on the
                            // right edge, and one press is enough: an unpaired
                            // device is paired and connected in one go.
                            if theme::primary(ui, t!(lang, "подключить", "connect"), t)
                                .clicked()
                            {
                                action = Some((
                                    if d.paired { "connect" } else { "pair_connect" },
                                    d.address.clone(),
                                ));
                            }
                            if theme::ghost(ui, t!(lang, "удалить", "remove"), t).clicked() {
                                remove_ask = Some(d.address.clone());
                            }
                            if !d.trusted
                                && theme::ghost(ui, t!(lang, "доверять", "trust"), t).clicked()
                            {
                                action = Some(("trust", d.address.clone()));
                            }
                        }
                    });
                });
                if d.blocked {
                    theme::small(
                        ui,
                        t!(
                            lang,
                            "устройство заблокировано в bluez — сначала разблокируйте его в системе.",
                            "the device is blocked in bluez — unblock it in the system first."
                        ),
                        t.warn,
                    );
                }
            });
    }

    if let Some(address) = remove_ask {
        app.bluetooth_remove = if address.is_empty() {
            None
        } else {
            Some(address)
        };
    }
    if let Some((action, address)) = action {
        app.bluetooth_remove = None;
        app.bluetooth_action(action, &address);
    }
}

fn power_name(lang: Lang, action: PowerAction) -> &'static str {
    match action {
        PowerAction::Lock => t!(lang, "заблокировать", "lock"),
        PowerAction::Sleep => t!(lang, "сон", "sleep"),
        PowerAction::Reboot => t!(lang, "перезагрузка", "restart"),
        PowerAction::Shutdown => t!(lang, "выключить", "shut down"),
        PowerAction::ScreenOff => t!(lang, "погасить экран", "screen off"),
    }
}

fn power_glyph(action: PowerAction) -> &'static str {
    match action {
        PowerAction::Lock => theme::icon::LOCK,
        PowerAction::Sleep => theme::icon::BEDTIME,
        PowerAction::Reboot => theme::icon::RESTART_ALT,
        PowerAction::Shutdown => theme::icon::POWER,
        PowerAction::ScreenOff => theme::icon::SCREEN,
    }
}

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
                    ui.label(RichText::new(&pad.name).size(18.0).strong().color(t.strong));
                    let live = status_pads.iter().find(|p| p.id == pad.id);
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {}",
                            pad.kind.label_lang(app.lang),
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
                        .color(if app.gamepad_on {
                            t.good
                        } else {
                            t.faint
                        }),
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
                            RichText::new(kind.label_lang(app.lang)).size(13.5),
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
                            app.gamepad_kind.label_lang(app.lang)
                        ))
                        .desired_width(300.0),
                );
                if app.gamepad_pending {
                    ui.spinner();
                } else if theme::primary(ui, t!(app.lang, "создать пад", "create pad"), t).clicked()
                {
                    app.create_pad();
                }
            });
        }
    });

    ui.add_space(10.0);

    // the live bar: one obvious button that starts or stops sending the pad
    let last = app.last_button.clone();
    let devices = app.input_devices.len();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            let (dot, label) = if app.gamepad_on {
                (
                    t.good,
                    t!(
                        app.lang,
                        "live · ввод идёт на компьютер",
                        "live · input goes to the computer"
                    ),
                )
            } else if has_pad {
                (t.warn, t!(app.lang, "пад на паузе", "pad paused"))
            } else {
                (t.faint, t!(app.lang, "пад не создан", "pad not created"))
            };
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 4.5, dot);
            ui.label(RichText::new(label).size(13.5).color(t.muted));
            if let Some((code, name, at)) = &last {
                if at.elapsed() < Duration::from_secs(6) {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(format!("{name} · {code:#x}"))
                            .family(FontFamily::Monospace)
                            .size(11.5)
                            .color(t.faint),
                    );
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = if app.gamepad_on {
                    t!(app.lang, "остановить", "stop")
                } else if has_pad {
                    t!(app.lang, "выйти в live", "go live")
                } else {
                    t!(app.lang, "создать и включить", "create and go live")
                };
                let fill = if app.gamepad_on { t.bad } else { t.accent };
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(text)
                                .size(15.5)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(fill)
                        .stroke(Stroke::new(1.0, fill))
                        .corner_radius(CornerRadius::ZERO)
                        .min_size(Vec2::new(210.0, 48.0)),
                    )
                    .clicked()
                {
                    if app.gamepad_on {
                        app.set_gamepad_on(false);
                    } else if has_pad {
                        app.set_gamepad_on(true);
                    } else {
                        app.create_pad();
                    }
                }
            });
        });
        ui.label(
            RichText::new(format!(
                "{} {} · {}",
                t!(app.lang, "устройств ввода", "input devices"),
                devices,
                if app.gamepad_on {
                    t!(
                        app.lang,
                        "стики и кнопки рисуются вживую",
                        "sticks and buttons are live"
                    )
                } else {
                    t!(
                        app.lang,
                        "нажатия только подсвечиваются",
                        "presses are only highlighted"
                    )
                }
            ))
            .family(FontFamily::Monospace)
            .size(11.0)
            .color(t.faint),
        );
    });

    ui.add_space(10.0);
    let state = app.gamepad.clone();
    let pressed = app.pressed.clone();
    let host = app.active_host_name();
    let pad_label = app
        .gamepad_pad
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| t!(app.lang, "пад не создан", "no pad").to_string());
    let online = app.online();
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
            lang: app.lang,
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
                            t!(
                                app.lang,
                                "нажмите кнопку на деке, чтобы увидеть её здесь",
                                "press a button on the deck to see it here"
                            )
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
                    if theme::ghost(ui, t!(app.lang, "снять выделение", "clear selection"), t)
                        .clicked()
                    {
                        app.deck_view.selected = None;
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if theme::primary(ui, t!(app.lang, "назначить бинд", "assign a bind"), t).clicked()
                {
                    app.binds.editor =
                        Some(BindEditor::for_trigger(control.name(), control.trigger()));
                    app.binds.capture = false;
                }
                if theme::ghost(ui, t!(app.lang, "записать с деки", "capture from deck"), t)
                    .clicked()
                {
                    let mut editor = BindEditor::for_trigger(control.name(), control.trigger());
                    editor.name = control.name().to_string();
                    app.binds.editor = Some(editor);
                    app.set_bind_capture(true);
                }
                if ui
                    .selectable_label(
                        !app.cfg.binds.iter().any(
                            |b| matches!(&b.trigger, Trigger::Button { code: c, .. } if c == &code),
                        ),
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
                        ui.label(RichText::new(&bind.name).size(14.5).color(if bind.enabled {
                            t.strong
                        } else {
                            t.muted
                        }));
                        ui.label(
                            RichText::new(bind.action.label_lang(app.lang))
                                .size(12.5)
                                .color(t.muted),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::ghost(ui, t!(app.lang, "изменить", "edit"), t).clicked()
                            {
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
            let (_, released, _) = theme::slider_bar(
                ui,
                t,
                150.0,
                30.0,
                &mut app.deck_view.cfg.scale,
                0.6..=1.4,
                0.01,
            );
            if released {
                app.save();
            }
            theme::switch(ui, &mut app.deck_view.cfg.labels, t);
            ui.label(
                RichText::new(t!(app.lang, "подписи", "labels"))
                    .size(13.0)
                    .color(t.muted),
            );
            theme::switch(ui, &mut app.deck_view.cfg.glow, t);
            ui.label(
                RichText::new(t!(app.lang, "свечение", "glow"))
                    .size(13.0)
                    .color(t.muted),
            );
            theme::switch(ui, &mut app.deck_view.cfg.sticks, t);
            ui.label(
                RichText::new(t!(app.lang, "стики", "sticks"))
                    .size(13.0)
                    .color(t.muted),
            );
        });
    });

    let devices = app.input_devices.clone();
    let errors = app.input_errors.clone();
    if !devices.is_empty() || !errors.is_empty() {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(
                t!(app.lang, "ввод с деки", "deck input"),
                t.faint,
            ));
            for d in &devices {
                ui.label(
                    RichText::new(d)
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.muted),
                );
            }
            for e in &errors {
                ui.label(
                    RichText::new(crate::input::error_text(app.lang, e))
                        .size(12.5)
                        .color(t.bad),
                );
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

fn backend_list() -> [MonitorBackend; 9] {
    [
        MonitorBackend::Auto,
        MonitorBackend::NiriVirtual,
        MonitorBackend::Hyprland,
        MonitorBackend::Vkms,
        MonitorBackend::Krfb,
        MonitorBackend::Mirror,
        MonitorBackend::X11,
        MonitorBackend::Gdigrab,
        MonitorBackend::Vdd,
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
            if theme::primary(ui, t!(app.lang, "остановить экран", "stop the screen"), t).clicked()
            {
                if let Some(c) = app.client() {
                    app.spawn(move || {
                        Task::MonitorStopped(c.stop_monitor().map_err(|e| e.to_string()))
                    });
                }
            }
        } else if theme::primary(ui, t!(app.lang, "запустить экран", "start the screen"), t)
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

    theme::small(
        ui,
        t!(
            app.lang,
            "по умолчанию: расширить, авто-бэкенд и 1280×800 — лучший вариант для деки. выход создаётся одной кнопкой.",
            "defaults: extend, auto backend and 1280×800 — the best fit for the deck. one press creates the output."
        ),
        t.faint,
    );
    if monitor.niri_virtual {
        theme::small(
            ui,
            t!(
                app.lang,
                "niri умеет виртуальные выходы (biri): дополнительный монитор рисуется тем же GPU и не зависит от VKMS.",
                "niri can create virtual outputs (biri): the extra monitor is drawn by the same GPU and needs no VKMS."
            ),
            t.good,
        );
    }
    ui.add_space(8.0);

    // The preview takes what the page has left after the controls. On the Deck
    // (1280×800) the settings card must stay on screen instead of hiding under
    // the bottom bar, so its height and the bar are reserved first.
    let area = ui.available_rect_before_wrap();
    let reserved = 104.0 + 250.0;
    let room = (area.height() - reserved).max(210.0);
    let stream_h = (area.width() / 1.6).clamp(210.0, 520.0).min(room);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(area.width(), stream_h), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::ZERO,
        Color32::from_rgb(0x02, 0x02, 0x06),
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(8.0);
    if monitor.running || app.stream.texture.is_some() {
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
        if !monitor.running {
            // The capture is restarting after a mode switch: the last frame
            // stays, and a chip says why it is not moving.
            let label = t!(app.lang, "кадр с прошлого запуска", "the last frame");
            let galley = ui.painter().layout_no_wrap(
                label.to_string(),
                egui::FontId::proportional(12.0),
                Color32::from_white_alpha(210),
            );
            let pad = Vec2::new(8.0, 4.0);
            let chip = Rect::from_min_size(
                inner.left_top() + Vec2::new(10.0, 10.0),
                galley.size() + pad * 2.0,
            );
            ui.painter()
                .rect_filled(chip, CornerRadius::ZERO, Color32::from_black_alpha(170));
            ui.painter()
                .galley(chip.min + pad, galley, Color32::from_white_alpha(210));
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
        monitor_settings(ui, app, t);
        ui.add_space(6.0);
        let mut fps = app.stream.fps;
        if theme::slider_u32(ui, t, t!(app.lang, "fps", "fps"), &mut fps, 5..=60) {
            app.stream.fps = fps;
            app.save_soon();
        }
        let mut quality = app.stream.quality as u32;
        if theme::slider_u32(
            ui,
            t,
            t!(app.lang, "качество", "quality"),
            &mut quality,
            30..=95,
        ) {
            app.stream.quality = quality.min(95) as u8;
            app.save_soon();
        }
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

/// The four groups of streaming settings. At 100 % on a deck-sized window they
/// share one row; at a bigger UI scale they no longer fit next to each other
/// (the size buttons alone take a third of the width), and a plain row would
/// push the last group past the card. The row is split in two instead.
fn monitor_settings(ui: &mut Ui, app: &mut App, t: &Tokens) {
    // Measured: the four groups need 905 points in Russian and 917 in
    // Ukrainian, whatever the UI scale. Anything narrower gets two rows.
    let one_row = ui.available_width() >= 950.0;
    if one_row {
        ui.horizontal(|ui| {
            monitor_mode_ui(ui, app, t);
            ui.add_space(16.0);
            monitor_backend_ui(ui, app, t);
            ui.add_space(16.0);
            monitor_position_ui(ui, app, t);
            ui.add_space(16.0);
            monitor_size_ui(ui, app, t);
        });
    } else {
        ui.horizontal(|ui| {
            monitor_mode_ui(ui, app, t);
            ui.add_space(16.0);
            monitor_backend_ui(ui, app, t);
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            monitor_position_ui(ui, app, t);
            ui.add_space(16.0);
            monitor_size_ui(ui, app, t);
        });
    }
}

fn monitor_mode_ui(ui: &mut Ui, app: &mut App, t: &Tokens) {
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
                    app.save_soon();
                }
            }
        });
    });
}

fn monitor_backend_ui(ui: &mut Ui, app: &mut App, t: &Tokens) {
    ui.vertical(|ui| {
        ui.label(theme::hud(t!(app.lang, "бэкенд", "backend"), t.faint));
        egui::ComboBox::from_id_salt("monitor-backend")
            .selected_text(app.stream.backend.label_lang(app.lang))
            .show_ui(ui, |ui| {
                for b in backend_list() {
                    if ui
                        .selectable_label(app.stream.backend == b, b.label_lang(app.lang))
                        .clicked()
                    {
                        app.stream.backend = b;
                        app.save_soon();
                    }
                }
            });
    });
}

fn monitor_position_ui(ui: &mut Ui, app: &mut App, t: &Tokens) {
    ui.vertical(|ui| {
        ui.label(theme::hud(t!(app.lang, "позиция", "position"), t.faint));
        ui.add_enabled_ui(app.stream.mode == MonitorMode::Extend, |ui| {
            egui::ComboBox::from_id_salt("monitor-position")
                .selected_text(app.stream.position.label_lang(app.lang))
                .width(130.0)
                .show_ui(ui, |ui| {
                    for p in MonitorPosition::ALL {
                        if ui
                            .selectable_label(app.stream.position == p, p.label_lang(app.lang))
                            .clicked()
                        {
                            app.stream.position = p;
                            app.save_soon();
                        }
                    }
                });
        });
    });
}

fn monitor_size_ui(ui: &mut Ui, app: &mut App, t: &Tokens) {
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
                    app.save_soon();
                }
            }
        });
    });
}

// ---------------------------------------------------------------- notifications

/// A small Grid tile with the notification picture, or a domain icon when the
/// daemon had no image to send.
fn notification_thumb(
    ui: &mut Ui,
    t: &Tokens,
    n: &Notification,
    art: Option<&egui::TextureHandle>,
    color: Color32,
) {
    let side = 34.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.well);
    match art {
        Some(tex) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
        }
        None => {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                notif_glyph(&n.app, &n.icon),
                egui::FontId::new(side * 0.5, theme::icon_family()),
                color,
            );
        }
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    ui.add_space(12.0);
}

/// One notification card, drawn in a slot that can be pulled aside.
///
/// The card is laid out inside a child shifted by `off`, so the border, the
/// picture and the text move together as one thing, while the list still
/// reserves the card's height at its original place: the neighbours never jump
/// while a finger holds one card.
fn notification_slot(
    ui: &mut Ui,
    id: u64,
    salt: &str,
    off: f32,
    alpha: f32,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    let pos = ui.cursor().min;
    let width = ui.available_width();
    // The card may only be seen inside the list slot it belongs to: a slide
    // must not paint over the dim layer or the page beside the control centre.
    let outer = ui.clip_rect();
    let clip = Rect::from_min_max(
        egui::pos2(pos.x, outer.top()),
        egui::pos2(pos.x + width, outer.bottom()),
    )
    .intersect(outer);
    let room = ui.available_height().max(1.0);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .id_salt((salt, id))
            .max_rect(Rect::from_min_size(
                pos + Vec2::new(off, 0.0),
                Vec2::new(width, room),
            ))
            .layout(*ui.layout()),
    );
    row.set_clip_rect(clip);
    row.set_opacity(alpha);
    let card = add(&mut row);
    let used = row.min_rect().size();
    ui.advance_cursor_after_rect(Rect::from_min_size(pos, Vec2::new(width, used.y)));
    card
}

/// A tiny accent square marks a notification that has not been read yet. It
/// disappears with the card's "read" state, not with the card itself.
fn unread_marker(ui: &mut Ui, t: &Tokens) {
    theme::dot_scaled(ui, t.accent, true, 11.0);
}

/// Notification text is external input. A few desktop tools send a body with
/// many newlines or a very long path; letting that raw text choose the row
/// height makes one notification push the whole list away. Keep the content
/// readable, but make every preview a single bounded line.
pub(crate) fn notification_line(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    util::truncate(&compact, max_chars)
}

/// Screenshot tools commonly emit a fresh notification for every saved file,
/// with only the path in the body changing. Keep the newest row and show a
/// count for consecutive screenshot notices, otherwise a burst can turn the
/// notification page into a wall of identical rows.
struct NotificationRow {
    notification: Notification,
    repeats: usize,
}

fn screenshot_group(n: &Notification) -> Option<String> {
    let key = format!("{} {} {}", n.app, n.icon, n.summary).to_lowercase();
    if [
        "screenshot",
        "screen shot",
        "скрин",
        "niri",
        "grim",
        "spectacle",
    ]
    .iter()
    .any(|part| key.contains(part))
    {
        Some(format!(
            "{}|{}",
            n.app.to_lowercase(),
            n.summary.to_lowercase()
        ))
    } else {
        None
    }
}

fn compact_notification_rows(source: Vec<Notification>) -> Vec<NotificationRow> {
    let mut rows: Vec<NotificationRow> = Vec::with_capacity(source.len());
    for notification in source {
        let key = screenshot_group(&notification);
        if let Some(key) = key {
            if let Some(last) = rows.last_mut() {
                if screenshot_group(&last.notification).as_deref() == Some(key.as_str()) {
                    last.repeats += 1;
                    continue;
                }
            }
        }
        rows.push(NotificationRow {
            notification,
            repeats: 1,
        });
    }
    rows
}

/// A rough guess at the app family, so a plain notification still gets a
/// meaningful glyph instead of a generic bell.
pub(crate) fn notif_glyph(app: &str, icon: &str) -> &'static str {
    let key = format!("{} {}", app, icon).to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| key.contains(n));
    if has(&[
        "spotify",
        "music",
        "player",
        "vlc",
        "audacious",
        "rhythmbox",
    ]) {
        theme::icon::MUSIC
    } else if has(&["camera", "photo", "screenshot", "obs", "spectacle"]) {
        theme::icon::CAMERA
    } else if has(&[
        "code",
        "terminal",
        "konsole",
        "kitty",
        "alacritty",
        "vscode",
    ]) {
        theme::icon::CODE
    } else if has(&["steam", "game", "lutris", "heroic"]) {
        theme::icon::GAMEPAD
    } else if has(&["volume", "audio", "sound", "pulse", "pipewire"]) {
        theme::icon::SPEAKER
    } else if has(&["battery", "power", "charger", "upower"]) {
        theme::icon::BATTERY
    } else if has(&["download", "update", "sync", "backup"]) {
        theme::icon::ADDON
    } else if has(&[
        "mail", "message", "chat", "discord", "telegram", "signal", "element",
    ]) {
        theme::icon::PERSON
    } else {
        theme::icon::NOTIFICATIONS
    }
}

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

    if app.notif_hist && !app.notif_log_loaded && !app.notif_log_pending {
        app.load_notif_log();
    }

    ui.horizontal(|ui| {
        // Two views of the same store: what is live now, and what the
        // computer still remembers.
        let mut tab = |ui: &mut Ui, hist: bool, label: &str, count: usize| {
            let on = app.notif_hist == hist;
            let resp = ui.add(
                egui::Button::new(
                    RichText::new(format!("{label} {count}"))
                        .size(13.5)
                        .color(if on { Color32::WHITE } else { t.ink }),
                )
                .fill(if on { t.accent } else { Color32::TRANSPARENT })
                .stroke(Stroke::new(1.0, if on { t.accent } else { t.line_strong }))
                .corner_radius(CornerRadius::ZERO)
                .min_size(Vec2::new(0.0, 30.0)),
            );
            if resp.clicked() {
                app.notif_hist = hist;
            }
        };
        tab(
            ui,
            false,
            t!(app.lang, "сейчас", "live"),
            app.notifications.len(),
        );
        tab(
            ui,
            true,
            t!(app.lang, "история", "history"),
            app.notif_log.len(),
        );
        ui.add_space(8.0);
        if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
            app.last_notif = std::time::Instant::now() - std::time::Duration::from_secs(5);
            app.notif_log_loaded = false;
            if app.notif_hist {
                app.load_notif_log();
            }
        }
        if theme::ghost(ui, t!(app.lang, "проверить", "test"), t).clicked() {
            if let Some(c) = app.client() {
                app.spawn(move || {
                    Task::Msg(
                        c.test_notification()
                            .map(|_| {
                                t!(Lang::Ru, "тестовое уведомление", "test notification")
                                    .to_string()
                            })
                            .map_err(|e| e.to_string()),
                    )
                });
            }
        }
        if app.notif_unread > 0
            && theme::ghost(ui, t!(app.lang, "прочитать все", "read all"), t).clicked()
        {
            app.read_all_notifs();
        }
        if !app.notif_log.is_empty()
            && theme::ghost(ui, t!(app.lang, "очистить", "clear"), t).clicked()
        {
            if let Some(c) = app.client() {
                let lang = app.lang;
                app.spawn(move || {
                    Task::Msg(
                        c.clear_notifications()
                            .map(|_| t!(lang, "очищено", "cleared").to_string())
                            .map_err(|e| e.to_string()),
                    )
                });
            }
            app.notifications.clear();
            app.notif_log.clear();
            app.notif_art.clear();
            app.notif_read.clear();
            app.notif_leaving.clear();
            app.notif_drag = None;
            app.notif_last_id = 0;
            app.notif_unread = 0;
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
    ui.add_space(4.0);

    if app.notif_hist {
        let hint = t!(app.lang, "поиск по истории", "search the history");
        ui.add(
            TextEdit::singleline(&mut app.notif_query)
                .hint_text(hint)
                .desired_width(320.0),
        );
        if !app.notif_log_err.is_empty() {
            ui.label(
                RichText::new(util::truncate(&app.notif_log_err, 200))
                    .size(12.0)
                    .color(t.warn),
            );
        }
        ui.add_space(4.0);
    }

    // One compact row per notification: a small urgency marker on the left,
    // the picture, two lines of text and the age. No big paddings — a
    // screenful is a dozen of them.
    let source: Vec<Notification> = if app.notif_hist {
        let needle = app.notif_query.trim().to_lowercase();
        app.notif_log
            .iter()
            .filter(|n| {
                needle.is_empty()
                    || n.app.to_lowercase().contains(&needle)
                    || n.summary.to_lowercase().contains(&needle)
                    || n.body.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    } else {
        app.notifications.iter().rev().cloned().collect()
    };
    let source = compact_notification_rows(source);

    if source.is_empty() {
        theme::card(ui, t, |ui| {
            let (title, sub) = if app.notif_hist {
                (
                    t!(app.lang, "история пуста", "the history is empty"),
                    t!(
                        app.lang,
                        "компьютер пока ничего не запомнил — уведомления накапливаются, пока включено зеркалирование.",
                        "the computer has not kept anything yet — notifications pile up while mirroring is on."
                    ),
                )
            } else {
                (
                    t!(app.lang, "пока тихо", "all quiet"),
                    t!(
                        app.lang,
                        "включите зеркалирование в sdw-daemon на пк: notifications → mirror to deck.",
                        "enable mirroring in sdw-daemon on the PC: notifications → mirror to deck."
                    ),
                )
            };
            ui.label(RichText::new(title).size(17.0).strong().color(t.strong));
            ui.label(RichText::new(sub).size(13.0).color(t.muted));
        });
        return;
    }

    let mut dismiss: Option<u64> = None;
    let mut activate: Option<u64> = None;
    // No scrollbar: the floating bar egui draws by default is a stray hairline
    // over the right edge of the cards, and no other page shows one. The deck
    // is a touchscreen — the list follows the finger.
    egui::ScrollArea::vertical()
        .id_salt("notif-list")
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            for row in source {
                let n = row.notification;
                let color = notification_color(t, &n);
                let app_name =
                    notification_line(if n.app.is_empty() { "system" } else { &n.app }, 48);
                let summary = notification_line(&n.summary, 120);
                let body = notification_line(&n.body, 220);
                let art = app.notif_texture(ui.ctx(), n.id, &n.image);
                let (off, alpha) = app.notif_slide(n.id);
                let leaving = app.notif_leaving(n.id);
                // The history is what the PC kept; its age says it all, so the
                // unread marker belongs to the live list only.
                let unread = !app.notif_hist && !app.notif_seen(n.id);
                // A read card steps back, so the list visibly answers to
                // "read all" instead of only losing an 11 px dot.
                let alpha = alpha * if unread || app.notif_hist { 1.0 } else { 0.6 };
                let lang = app.lang;
                let mut close_rect: Option<Rect> = None;
                let card = notification_slot(ui, n.id, "deck-notif-slot", off, alpha, |ui| {
                    egui::Frame::new()
                        .fill(t.card)
                        .stroke(Stroke::new(1.0, t.line))
                        .corner_radius(CornerRadius::ZERO)
                        .inner_margin(Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            // Keep urgency as a tiny marker. A full-height
                            // yellow stripe made long Niri/screenshot
                            // notifications look infinite and turned a burst
                            // into a wall of colour.
                            let bar = ui.max_rect().left_top() + egui::vec2(0.0, 10.0);
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(bar, Vec2::splat(5.0)),
                                CornerRadius::ZERO,
                                color,
                            );
                            ui.horizontal(|ui| {
                                notification_thumb(ui, t, &n, art.as_ref(), color);
                                let close = ui.available_width() - 26.0;
                                ui.vertical(|ui| {
                                    ui.set_max_width(close.max(120.0));
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&app_name)
                                                .family(FontFamily::Monospace)
                                                .size(10.5)
                                                .color(color),
                                        );
                                        if unread {
                                            unread_marker(ui, t);
                                        }
                                        ui.label(
                                            RichText::new(age(n.ts, lang))
                                                .family(FontFamily::Monospace)
                                                .size(10.0)
                                                .color(t.faint),
                                        );
                                        if n.urgency >= 2 {
                                            theme::chip(ui, t!(lang, "важно", "urgent"), t.bad);
                                        }
                                        if row.repeats > 1 {
                                            ui.label(
                                                RichText::new(format!("×{}", row.repeats))
                                                    .family(FontFamily::Monospace)
                                                    .size(10.0)
                                                    .color(t.warn),
                                            );
                                        }
                                    });
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&summary)
                                                .size(14.0)
                                                .strong()
                                                .color(t.strong),
                                        )
                                        .truncate(),
                                    );
                                    if !body.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&body).size(12.0).color(t.muted),
                                            )
                                            .truncate(),
                                        );
                                    }
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let close = ui
                                        .add(
                                            egui::Button::new(theme::icon(
                                                theme::icon::CLOSE,
                                                15.0,
                                                t.muted,
                                            ))
                                            .fill(Color32::TRANSPARENT)
                                            .stroke(Stroke::NONE),
                                        )
                                        .on_hover_text(t!(lang, "скрыть", "dismiss"));
                                    close_rect = Some(close.rect);
                                    if close.clicked() {
                                        dismiss = Some(n.id);
                                    }
                                });
                            });
                        })
                        .response
                });
                if leaving {
                    // Already sliding out: it takes no taps and has no rect to
                    // start a second swipe on.
                    ui.add_space(4.0);
                    continue;
                }
                // While the control centre is up, its own shade owns the
                // screen: a swipe over a page card under the dim layer must
                // not reach that card.
                if !app.quick_open {
                    app.notif_rows.push((n.id, card.rect));
                }
                // The whole row is the button: a tap opens the notification
                // on the computer, which is what a person reaching for it
                // means. A sideways pull is taken by the raw input layer, so
                // the click can never fight the swipe.
                let hit = ui.interact(
                    card.rect,
                    egui::Id::new(("deck-notif", n.id)),
                    Sense::click(),
                );
                if hit.clicked() {
                    // The card's own click area sits over the close button, so
                    // egui hands the click here: the button keeps its meaning
                    // by where the finger landed.
                    let on_close = hit
                        .interact_pointer_pos()
                        .is_some_and(|pos| close_rect.is_some_and(|r| r.contains(pos)));
                    if on_close {
                        dismiss = Some(n.id);
                    } else {
                        activate = Some(n.id);
                    }
                }
                if hit.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                hit.on_hover_text(t!(
                    lang,
                    "нажмите, чтобы открыть на компьютере",
                    "tap to open it on the computer"
                ));
                ui.add_space(4.0);
            }
            // The floating dock hovers over the bottom of the page. Without
            // this the last row could not be scrolled out from under it.
            ui.add_space(96.0);
        });
    if let Some(id) = activate {
        app.activate_notif(id);
    }
    if let Some(id) = dismiss {
        app.dismiss_notif(id);
    }
}

// ---------------------------------------------------------------- media

fn media_time(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn media_button(ui: &mut Ui, t: &Tokens, glyph: &str, size: f32, active: bool) -> bool {
    let fill = if active { t.accent } else { t.well };
    let color = if active { Color32::WHITE } else { t.ink };
    ui.add(
        egui::Button::new(theme::icon(glyph, size * 0.44, color))
            .fill(fill)
            .stroke(Stroke::new(
                1.0,
                if active { t.accent } else { t.line_strong },
            ))
            .corner_radius(CornerRadius::ZERO)
            .min_size(Vec2::splat(size)),
    )
    .clicked()
}

fn media_cover(ui: &mut Ui, t: &Tokens, side: f32, art: Option<&egui::TextureHandle>, error: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::hover());
    match art {
        Some(tex) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, t.well);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, t.line),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                theme::icon::ALBUM,
                egui::FontId::new(side * 0.28, theme::icon_family()),
                t.faint,
            );
            if !error.is_empty() {
                ui.painter().text(
                    rect.center() + Vec2::new(0.0, side * 0.22),
                    Align2::CENTER_CENTER,
                    util::truncate(error, 28),
                    egui::FontId::new(11.0, FontFamily::Monospace),
                    t.bad,
                );
            }
        }
    }
}

pub fn media(app: &mut App, ui: &mut Ui, t: &Tokens) {
    page_title(
        ui,
        t,
        t!(app.lang, "медиа", "media"),
        t!(
            app.lang,
            "плеер, обложка и системный звук — всё с дека.",
            "player, cover art and system sound — all from the deck."
        ),
    );

    let st = app.media.state.clone();
    let connected = app.online();
    let art = app.media.art.clone();
    let art_error = app.media.art_error.clone();

    // status line
    ui.horizontal(|ui| {
        theme::dot(ui, if connected { t.good } else { t.bad }, true);
        ui.label(
            RichText::new(if !connected {
                t!(app.lang, "нет связи с компьютером", "no computer")
            } else if st.player.is_empty() {
                t!(app.lang, "плеер не найден", "no player")
            } else {
                &st.player
            })
            .size(14.0)
            .strong()
            .color(if connected { t.strong } else { t.bad }),
        );
        if st.available && !st.status.is_empty() {
            theme::chip(
                ui,
                &st.status.to_lowercase(),
                if st.playing() { t.good } else { t.faint },
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
                app.media.last_poll = std::time::Instant::now() - std::time::Duration::from_secs(5);
            }
        });
    });
    ui.add_space(6.0);

    // now playing
    theme::card(ui, t, |ui| {
        ui.horizontal_top(|ui| {
            let side = (ui.available_width() * 0.26).clamp(160.0, 250.0);
            media_cover(ui, t, side, art.as_ref(), &art_error);
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(theme::icon(theme::icon::MUSIC, 17.0, t.faint));
                    ui.label(theme::hud(
                        t!(app.lang, "сейчас играет", "now playing"),
                        t.faint,
                    ));
                });
                let title = if st.title.trim().is_empty() {
                    if st.available {
                        t!(app.lang, "ничего не играет", "nothing playing")
                    } else {
                        t!(
                            app.lang,
                            "запустите музыку на пк",
                            "start something on the PC"
                        )
                    }
                } else {
                    &st.title
                };
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size((side * 0.13).clamp(22.0, 34.0))
                            .strong()
                            .color(t.strong),
                    )
                    .truncate(),
                );
                if !st.artist.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&st.artist).size(18.0).color(t.ink))
                            .truncate(),
                    );
                }
                if !st.album.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&st.album).size(13.0).color(t.muted))
                            .truncate(),
                    );
                }
                ui.add_space(10.0);

                // position
                let length = if st.length > 1.0 { st.length } else { 1.0 };
                let length_f = length as f32;
                let mut pos_f = app.media.seeking.unwrap_or(st.position).clamp(0.0, length) as f32;
                ui.spacing_mut().interact_size.y = 30.0;
                let (_, released, dragged) = theme::slider_bar(
                    ui,
                    t,
                    ui.available_width(),
                    30.0,
                    &mut pos_f,
                    0.0..=length_f,
                    theme::auto_step(0.0, length_f),
                );
                let pos = pos_f as f64;
                if dragged {
                    app.media.seeking = Some(pos);
                }
                if released {
                    app.media_command("seek", pos);
                }
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(media_time(pos))
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.faint),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if st.length > 1.0 {
                                media_time(st.length)
                            } else {
                                "—".into()
                            })
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.faint),
                        );
                    });
                });

                ui.add_space(8.0);
                // Four equal squares: same side, same gap, centred in the row.
                let playing = st.playing();
                let avail = ui.available_width();
                let gap = 8.0;
                let side = ((avail - gap * 3.0) / 4.0).clamp(44.0, 68.0);
                let total = side * 4.0 + gap * 3.0;
                ui.horizontal(|ui| {
                    ui.add_space(((avail - total) * 0.5).max(0.0));
                    ui.spacing_mut().item_spacing.x = gap;
                    if media_button(ui, t, theme::icon::PREV, side, false) {
                        app.media_command("prev", 0.0);
                    }
                    if media_button(ui, t, theme::icon::PLAY_PAUSE, side, playing) {
                        app.media_command("play_pause", 0.0);
                    }
                    if media_button(ui, t, theme::icon::NEXT, side, false) {
                        app.media_command("next", 0.0);
                    }
                    if media_button(ui, t, theme::icon::STOP, side, false) {
                        app.media_command("stop", 0.0);
                    }
                });
                ui.label(
                    RichText::new(if st.error.is_empty() {
                        util::truncate(&st.player, 34)
                    } else {
                        util::truncate(&st.error, 44)
                    })
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(if st.error.is_empty() { t.faint } else { t.warn }),
                );
            });
        });
    });

    ui.add_space(10.0);

    // system sound
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::icon(
                if st.muted {
                    theme::icon::VOLUME_OFF
                } else {
                    theme::icon::VOLUME_UP
                },
                22.0,
                if st.muted { t.warn } else { t.accent },
            ));
            ui.label(
                RichText::new(t!(app.lang, "системный звук", "system sound"))
                    .size(16.0)
                    .strong()
                    .color(t.strong),
            );
            ui.label(
                RichText::new(if st.sink.is_empty() {
                    "default output".to_string()
                } else {
                    util::truncate(&st.sink, 52)
                })
                .family(FontFamily::Monospace)
                .size(11.0)
                .color(t.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if media_button(
                    ui,
                    t,
                    if st.muted {
                        theme::icon::VOLUME_OFF
                    } else {
                        theme::icon::VOLUME_UP
                    },
                    38.0,
                    st.muted,
                ) {
                    app.media_command("mute", 0.0);
                }
            });
        });
        ui.add_space(4.0);
        let mut volume_call: Option<f64> = None;
        let mut busy: Option<(u8, bool)> = None;
        let row = ui.available_width();
        theme::slider_row(
            ui,
            row,
            34.0,
            |ui| {
                let mut vol = st.volume.min(100) as i32;
                // The value and the two buttons sit to the right of the
                // slider; it must not take their room.
                let mut v = vol as f32;
                let (_, released, _) = theme::slider_bar(
                    ui,
                    t,
                    (row - 196.0).max(120.0),
                    34.0,
                    &mut v,
                    0.0..=100.0,
                    1.0,
                );
                vol = v.round() as i32;
                if released {
                    volume_call = Some(vol as f64);
                }
                ui.label(
                    RichText::new(format!("{vol}%"))
                        .family(FontFamily::Monospace)
                        .size(14.0)
                        .strong()
                        .color(t.strong),
                );
            },
            |ui| {
                if media_button(ui, t, theme::icon::VOLUME_UP, 38.0, false) {
                    busy = Some((2, true));
                }
                if media_button(ui, t, theme::icon::VOLUME_DOWN, 38.0, false) {
                    busy = Some((1, false));
                }
            },
        );
        if let Some(v) = volume_call {
            app.media_command("volume", v);
        }
        if let Some((_, up)) = busy {
            app.media_command(if up { "volume_up" } else { "volume_down" }, 0.0);
        }
        if st.sinks.len() > 1 {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(theme::icon(theme::icon::SPEAKER, 17.0, t.faint));
                ui.label(theme::hud(t!(app.lang, "выход", "output"), t.faint));
                egui::ComboBox::from_id_salt("media-sink")
                    .selected_text(util::truncate(&st.sink, 40))
                    .width((ui.available_width() - 90.0).clamp(160.0, 420.0))
                    .show_ui(ui, |ui| {
                        for sink in &st.sinks {
                            if ui
                                .selectable_label(sink == &st.sink, sink.as_str())
                                .clicked()
                            {
                                app.set_media_sink(sink);
                            }
                        }
                    });
            });
        }
    });

    if !st.players.is_empty() {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::hud(t!(app.lang, "плееры", "players"), t.faint));
            for p in &st.players {
                let selected = p == &st.player;
                ui.label(theme::icon(
                    theme::icon::SPEAKER,
                    16.0,
                    if selected { t.accent } else { t.faint },
                ));
                let resp = ui.add(
                    egui::Button::new(
                        RichText::new(util::truncate(p, 34))
                            .size(12.5)
                            .color(if selected { Color32::WHITE } else { t.ink }),
                    )
                    .fill(if selected { t.accent } else { t.well })
                    .stroke(Stroke::new(1.0, if selected { t.accent } else { t.line }))
                    .corner_radius(CornerRadius::ZERO)
                    .min_size(Vec2::new(0.0, 38.0)),
                );
                if resp.clicked() && !selected {
                    app.select_media_player(p);
                }
            }
        });
    }

    if !connected {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "подключите компьютер на странице «компьютеры», чтобы управлять его плеером и звуком.",
                    "connect a computer on the computers page to control its player and sound."
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
    }
}

// ---------------------------------------------------------------- home assistant

/// Built-in quick colours for lamps: warm white through the usual saturated
/// ones. Names are bilingual because the Deck is.
///
/// The four whites carry the kelvin they stand for. A lamp that can take a
/// temperature gets `color_temp_kelvin` instead of an RGB guess, which is what
/// makes "very warm" actually warm instead of a dim orange; the swatch keeps a
/// representative RGB so the row still reads as a palette. Saturated shades
/// stay RGB.
struct LightPreset {
    ru: &'static str,
    en: &'static str,
    rgb: [u8; 3],
    kelvin: Option<u32>,
}

const LIGHT_PRESETS: &[LightPreset] = &[
    LightPreset {
        ru: "очень тёплый",
        en: "very warm",
        rgb: [255, 146, 39],
        kelvin: Some(2200),
    },
    LightPreset {
        ru: "тёплый",
        en: "warm",
        rgb: [255, 167, 88],
        kelvin: Some(2700),
    },
    LightPreset {
        ru: "нейтральный",
        en: "neutral",
        rgb: [255, 206, 166],
        kelvin: Some(4000),
    },
    LightPreset {
        ru: "холодный",
        en: "cool",
        rgb: [214, 230, 255],
        kelvin: Some(6500),
    },
    LightPreset {
        ru: "красный",
        en: "red",
        rgb: [255, 70, 60],
        kelvin: None,
    },
    LightPreset {
        ru: "зелёный",
        en: "green",
        rgb: [80, 220, 120],
        kelvin: None,
    },
    LightPreset {
        ru: "синий",
        en: "blue",
        rgb: [70, 120, 255],
        kelvin: None,
    },
    LightPreset {
        ru: "фиолетовый",
        en: "violet",
        rgb: [170, 90, 255],
        kelvin: None,
    },
];

/// Two shades that a person would call the same, allowing for the colour
/// shift a bulb reports.
fn rgb_near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 14)
}

/// Whether the lamp is showing this preset. Kelvin wins when both sides have
/// it, with a generous window: a 2700 K bulb reads back as 2650 or 2800, and
/// the tick must stay on "warm" rather than hop between swatches.
fn preset_shown(p: &LightPreset, e: &HaEntity, current: Option<[u8; 3]>) -> bool {
    if let (Some(want), Some(have)) = (p.kelvin, e.color_temp_kelvin()) {
        if want.abs_diff(have) <= 200 {
            return true;
        }
    }
    current.is_some_and(|c| rgb_near(c, p.rgb))
}

/// The service data for a preset tap. A lamp that can take a temperature gets
/// one, clamped to its own range; everything else gets the RGB shade.
fn light_preset_call(e: &HaEntity, p: &LightPreset) -> serde_json::Value {
    match p.kelvin {
        Some(k) if e.supports_color_temp() => {
            let min = e.min_color_temp_kelvin().max(1000);
            let max = e.max_color_temp_kelvin().max(min + 100);
            json!({ "color_temp_kelvin": k.clamp(min, max) })
        }
        _ => json!({ "rgb_color": p.rgb }),
    }
}

/// One square colour button. A selected swatch gets a thicker ink border and a
/// tick drawn in the contrasting shade, so a tap is visible at arm's length.
fn light_swatch(
    ui: &mut Ui,
    t: &Tokens,
    rgb: [u8; 3],
    selected: bool,
    tip: impl Into<egui::WidgetText>,
) -> egui::Response {
    let fill = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let ink = if luma_of(rgb) > 145.0 {
        Color32::from_rgb(24, 24, 26)
    } else {
        Color32::WHITE
    };
    let text = if selected {
        theme::icon(theme::icon::CHECK, 20.0, ink)
    } else {
        RichText::new("").size(20.0)
    };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .stroke(Stroke::new(
                if selected { 2.5 } else { 1.0 },
                if selected { t.strong } else { t.line },
            ))
            .corner_radius(CornerRadius::ZERO)
            .min_size(Vec2::splat(34.0)),
    )
    .on_hover_text(tip)
}

fn light_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// Perceived brightness of a colour, used to pick a tick that stays readable.
fn luma_of(rgb: [u8; 3]) -> f32 {
    0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2])
}

fn ha_glyph(domain: &str) -> &'static str {
    match domain {
        "light" => theme::icon::BULB,
        "switch" | "input_boolean" | "siren" => theme::icon::POWER,
        "sensor" | "humidifier" | "water_heater" => theme::icon::SENSORS,
        "binary_sensor" => theme::icon::CHECK,
        "climate" => theme::icon::THERMOSTAT,
        "media_player" => theme::icon::PLAY,
        "cover" | "valve" => theme::icon::BLINDS,
        "lock" => theme::icon::LOCK,
        "scene" => theme::icon::SCENE,
        "script" | "event" => theme::icon::CODE,
        "camera" | "image" => theme::icon::CAMERA,
        "person" | "device_tracker" => theme::icon::PERSON,
        "number" | "input_number" | "select" | "input_select" => theme::icon::TUNE,
        "fan" => theme::icon::SPEAKER,
        "vacuum" => theme::icon::WIDGETS,
        "update" => theme::icon::ADDON,
        _ => theme::icon::WIDGETS,
    }
}

fn ha_state_text(e: &HaEntity, lang: Lang) -> String {
    sdw_core::ha::state_with_unit(e, lang)
}

/// The domain's own name without the unit; used where the unit has its own
/// slot (climate, media player).
fn ha_state_bare(e: &HaEntity, lang: Lang) -> String {
    sdw_core::ha::state_text(e, lang)
}

/// A square, hairline icon button used inside Home Assistant tiles.
fn ha_icon_button(ui: &mut Ui, t: &Tokens, glyph: &str, active: bool, tip: &str) -> bool {
    let color = if active { t.accent } else { t.muted };
    let resp = ui.add(
        egui::Button::new(theme::icon(glyph, 19.0, color))
            .fill(if active {
                t.accent.gamma_multiply(0.14)
            } else {
                t.well
            })
            .stroke(Stroke::new(
                1.0,
                if active {
                    t.accent.gamma_multiply(0.5)
                } else {
                    t.line
                },
            ))
            .corner_radius(CornerRadius::ZERO)
            .min_size(Vec2::splat(34.0)),
    );
    resp.on_hover_text(tip).clicked()
}

fn ha_text_button(ui: &mut Ui, t: &Tokens, text: &str) -> bool {
    ui.add(
        egui::Button::new(RichText::new(text).size(15.0).color(t.ink))
            .fill(t.well)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(CornerRadius::ZERO)
            .min_size(Vec2::splat(34.0)),
    )
    .clicked()
}

fn ha_toggle(e: &HaEntity, on: bool) -> HaCall {
    let (domain, service) = match e.domain() {
        "scene" | "script" => (e.domain().to_string(), "turn_on".to_string()),
        "button" => ("button".to_string(), "press".to_string()),
        _ => (
            "homeassistant".to_string(),
            if on { "turn_on" } else { "turn_off" }.to_string(),
        ),
    };
    HaCall {
        domain,
        service,
        target: json!({ "entity_id": e.entity_id }),
        data: serde_json::Value::Null,
    }
}

fn ha_service(e: &HaEntity, service: &str, data: serde_json::Value) -> HaCall {
    HaCall {
        domain: e.domain().to_string(),
        service: service.to_string(),
        target: json!({ "entity_id": e.entity_id }),
        data,
    }
}

#[derive(Clone)]
enum HaRoom {
    Favorites,
    /// A tab of the Home Assistant Lovelace dashboard.
    View(usize),
    Category(usize),
    Area(String),
    Unassigned,
    All,
}

struct HaAction {
    call: HaCall,
    entity: String,
    /// Local state to show until Home Assistant confirms the change.
    state: Option<String>,
}

/// Sensors worth a tile. Everything else (updates, counters, diagnostics)
/// would drown the page in 600 rows.
fn ha_visible(e: &HaEntity) -> bool {
    ha_visible_with(e, false)
}

/// Home Assistant installations grow hundreds of entities, many of them
/// unavailable, diagnostics or update notices. By default the page keeps what
/// a person can use and hides the dead weight; one switch brings it back.
fn ha_visible_with(e: &HaEntity, show_all: bool) -> bool {
    let offline = matches!(e.state.as_str(), "unavailable" | "unknown");
    if sdw_core::ha::is_controllable(e) {
        if e.domain() == "media_player" {
            return e.is_on() || e.media_title().is_some();
        }
        // An update is interesting only while there is something to install.
        if e.domain() == "update" {
            return show_all || e.state == "on";
        }
        return true;
    }
    if offline && !show_all {
        return false;
    }
    let device_class = e
        .attributes
        .get("device_class")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match e.domain() {
        // Any sensor with a unit or a plain number is worth a tile: this is
        // what makes integration sensors (Proxmox, NAS, routers) show up
        // without teaching the deck every integration by name.
        "sensor" => {
            !e.unit().is_empty() || e.state.parse::<f64>().is_ok() || !device_class.is_empty()
        }
        "binary_sensor" => show_all || !matches!(device_class, "update"),
        "weather" | "person" | "image" | "update" => true,
        "event" => show_all,
        _ => false,
    }
}

fn ha_power_button(ui: &mut Ui, t: &Tokens, on: bool, lang: Lang, width: f32) -> bool {
    let label = if on {
        t!(lang, "выключить", "turn off")
    } else {
        t!(lang, "включить", "turn on")
    };
    // A whole card is far too wide for one action: keep the control compact so
    // a two-column page does not turn into a wall of 600 px buttons.
    ui.add_sized(
        [width.clamp(120.0, 200.0), 38.0],
        egui::Button::new(RichText::new(label).size(14.5).strong().color(if on {
            t.accent
        } else {
            t.ink
        }))
        .fill(if on {
            t.accent.gamma_multiply(0.16)
        } else {
            t.well
        })
        .stroke(Stroke::new(
            1.0,
            if on {
                t.accent.gamma_multiply(0.55)
            } else {
                t.line_strong
            },
        ))
        .corner_radius(CornerRadius::ZERO),
    )
    .clicked()
}

fn ha_move_button(ui: &mut Ui, t: &Tokens, glyph: &str, tip: &str, enabled: bool) -> bool {
    let resp = ui.add_enabled(
        enabled,
        egui::Button::new(theme::icon(glyph, 16.0, t.muted))
            .fill(t.well)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(CornerRadius::ZERO)
            .min_size(Vec2::splat(30.0)),
    );
    resp.on_hover_text(tip).clicked()
}

/// Page switcher for rooms with more tiles than fit in one screen.
fn ha_pager(ui: &mut Ui, t: &Tokens, page: &mut usize, pages: usize, total: usize, lang: Lang) {
    if pages <= 1 {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if theme::nav_arrow(ui, t, theme::icon::ARROW_LEFT, *page > 0).clicked() {
            *page = page.saturating_sub(1);
        }
        ui.label(
            RichText::new(format!("{} / {}", *page + 1, pages))
                .family(FontFamily::Monospace)
                .size(12.0)
                .color(t.muted),
        );
        if theme::nav_arrow(ui, t, theme::icon::ARROW_RIGHT, *page + 1 < pages).clicked() {
            *page = (*page + 1).min(pages - 1);
        }
        ui.label(
            RichText::new(format!(
                "{} {}",
                total,
                if lang == Lang::Ru {
                    "плиток"
                } else if lang == Lang::Uk {
                    "плиток"
                } else {
                    "tiles"
                }
            ))
            .family(FontFamily::Monospace)
            .size(10.5)
            .color(t.faint),
        );
    });
    ui.add_space(2.0);
}

#[allow(clippy::too_many_arguments)]
fn ha_card(
    ui: &mut Ui,
    t: &Tokens,
    lang: Lang,
    e: &HaEntity,
    width: f32,
    favorites: &[String],
    categories: &[HaCategory],
    current_category: Option<usize>,
    actions: &mut Vec<HaAction>,
    // Entity id and the percentage of a slider that was just let go; kept
    // apart so the slider never snaps back before Home Assistant answers.
    bright: &mut Vec<(String, u8)>,
    star: &mut Option<String>,
    category_change: &mut Option<(String, Option<String>)>,
    move_entity: &mut Option<(usize, String, i32)>,
    // Colours the user saved on the Deck, plus a flag that says the list was
    // changed and has to be written to the config.
    light_colors: &mut Vec<[u8; 3]>,
    palette_dirty: &mut bool,
) {
    let on = e.is_on();
    let accent = if on { t.accent } else { t.faint };
    egui::Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(
            1.0,
            if on {
                accent.gamma_multiply(0.45)
            } else {
                t.line
            },
        ))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.horizontal(|ui| {
                ui.label(theme::icon(ha_glyph(e.domain()), 22.0, accent));
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 196.0).max(60.0));
                    ui.add(
                        egui::Label::new(
                            RichText::new(e.name()).size(15.0).strong().color(t.strong),
                        )
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!(
                                "{} · {}",
                                if e.area.is_empty() {
                                    t!(lang, "дом", "home")
                                } else {
                                    &e.area
                                },
                                ha_state_text(e, lang)
                            ))
                            .family(FontFamily::Monospace)
                            .size(10.0)
                            .color(t.faint),
                        )
                        .truncate(),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let fav = favorites.contains(&e.entity_id);
                    if ui
                        .add(
                            egui::Button::new(theme::icon(
                                theme::icon::STAR,
                                15.0,
                                if fav { t.warn } else { t.faint },
                            ))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        *star = Some(e.entity_id.clone());
                    }
                    // Move inside the current category.
                    if let Some(ci) = current_category {
                        let pos = categories
                            .get(ci)
                            .and_then(|c| c.entities.iter().position(|x| x == &e.entity_id));
                        if let Some(pos) = pos {
                            if ha_move_button(
                                ui,
                                t,
                                theme::icon::ARROW_DOWN,
                                t!(lang, "ниже", "move down"),
                                pos + 1 < categories[ci].entities.len(),
                            ) {
                                *move_entity = Some((ci, e.entity_id.clone(), 1));
                            }
                            if ha_move_button(
                                ui,
                                t,
                                theme::icon::ARROW_UP,
                                t!(lang, "выше", "move up"),
                                pos > 0,
                            ) {
                                *move_entity = Some((ci, e.entity_id.clone(), -1));
                            }
                        }
                    }
                    // Put the entity into a category.
                    ui.menu_button(theme::icon(theme::icon::DRAG, 15.0, t.faint), |ui| {
                        ui.label(theme::hud(t!(lang, "категория", "category"), t.faint));
                        for c in categories {
                            let inside = c.entities.iter().any(|x| x == &e.entity_id);
                            if ui.selectable_label(inside, c.name.as_str()).clicked() {
                                *category_change = Some((e.entity_id.clone(), Some(c.id.clone())));
                                ui.close();
                            }
                        }
                        if categories.is_empty() {
                            ui.label(
                                RichText::new(t!(
                                    lang,
                                    "создайте категорию выше",
                                    "create a category above"
                                ))
                                .size(11.5)
                                .color(t.muted),
                            );
                        }
                        if categories
                            .iter()
                            .any(|c| c.entities.iter().any(|x| x == &e.entity_id))
                            && ui
                                .button(t!(lang, "убрать из категории", "remove from category"))
                                .clicked()
                        {
                            *category_change = Some((e.entity_id.clone(), None));
                            ui.close();
                        }
                    });
                });
            });
            ui.add_space(6.0);

            match e.domain() {
                "light" => {
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::POWER,
                            on,
                            t!(lang, "вкл/выкл", "on/off"),
                        ) {
                            actions.push(HaAction {
                                call: ha_toggle(e, !on),
                                entity: e.entity_id.clone(),
                                state: Some(if on { "off" } else { "on" }.into()),
                            });
                        }
                        if e.supports_brightness() {
                            // Only the value Home Assistant reported is the
                            // truth: an unknown brightness is shown as "—",
                            // never as an invented 100 %.
                            let reported = e.brightness_pct();
                            let mut pct = i32::from(reported.unwrap_or(0));
                            // The slider's size is `spacing.slider_width`, and
                            // the percentage sits to its right: the two share
                            // the card's content width.
                            let slider_w = (width - 104.0).max(90.0);
                            let mut send: Option<u8> = None;
                            // The right group runs after the slider, so the
                            // live value and the drag flag travel in cells.
                            let shown = std::cell::Cell::new(pct);
                            let dragging = std::cell::Cell::new(false);
                            theme::slider_row(
                                ui,
                                width - 60.0,
                                34.0,
                                |ui| {
                                    let mut v = pct as f32;
                                    let (_, released, dragged) = theme::slider_bar(
                                        ui,
                                        t,
                                        slider_w,
                                        34.0,
                                        &mut v,
                                        1.0..=100.0,
                                        1.0,
                                    );
                                    pct = v.round() as i32;
                                    if released {
                                        send = Some(pct.clamp(1, 100) as u8);
                                    }
                                    shown.set(pct);
                                    dragging.set(dragged);
                                },
                                |ui| {
                                    // While the finger moves, follow it;
                                    // otherwise the number is exactly what
                                    // Home Assistant reports.
                                    let value = if dragging.get() {
                                        Some(shown.get())
                                    } else {
                                        reported.map(i32::from)
                                    };
                                    ui.label(
                                        RichText::new(match value {
                                            Some(p) => format!("{p}%"),
                                            None => "—".to_string(),
                                        })
                                        .family(FontFamily::Monospace)
                                        .size(12.0)
                                        .color(t.faint),
                                    );
                                },
                            );
                            if let Some(pct) = send {
                                // Remember the value locally, then send it:
                                // the slider must not snap back while HA is
                                // still catching up with the service call.
                                bright.push((e.entity_id.clone(), pct));
                                actions.push(HaAction {
                                    call: ha_service(
                                        e,
                                        "turn_on",
                                        json!({ "brightness_pct": pct }),
                                    ),
                                    entity: e.entity_id.clone(),
                                    state: Some("on".into()),
                                });
                            }
                        } else {
                            // An on/off bulb has nothing to slide.
                            ui.label(
                                RichText::new(ha_state_text(e, lang))
                                    .size(13.0)
                                    .color(t.muted),
                            );
                        }
                    });
                    if e.supports_color_temp() {
                        let min = e.min_color_temp_kelvin().max(1000);
                        let max = e.max_color_temp_kelvin().max(min + 100);
                        let mut k = e
                            .color_temp_kelvin()
                            .unwrap_or((min + max) / 2)
                            .clamp(min, max) as i32;
                        // 50 K steps: the whole range stays reachable while a
                        // fingertip can still land on a wanted value.
                        let step = if max - min > 2000 { 50 } else { 10 };
                        ui.horizontal(|ui| {
                            ui.label(theme::icon(theme::icon::THERMOSTAT, 17.0, t.faint));
                            ui.spacing_mut().interact_size.y = 30.0;
                            let mut kv = k as f32;
                            let (_, released, _) = theme::slider_bar(
                                ui,
                                t,
                                (width - 100.0).max(90.0),
                                30.0,
                                &mut kv,
                                min as f32..=max as f32,
                                step as f32,
                            );
                            k = kv.round() as i32;
                            if released {
                                actions.push(HaAction {
                                    call: ha_service(
                                        e,
                                        "turn_on",
                                        json!({ "color_temp_kelvin": k }),
                                    ),
                                    entity: e.entity_id.clone(),
                                    state: Some("on".into()),
                                });
                            }
                            ui.label(
                                RichText::new(format!("{k}K"))
                                    .family(FontFamily::Monospace)
                                    .size(11.0)
                                    .color(t.faint),
                            );
                        });
                    }
                    if e.supports_color() {
                        let current = e.rgb_color();
                        ui.horizontal_wrapped(|ui| {
                            // Roomier gaps: a fingertip is not a mouse cursor,
                            // and neighbouring shades must not be hit by mistake.
                            ui.spacing_mut().item_spacing.y = 8.0;
                            ui.label(theme::icon(theme::icon::LOOK, 17.0, t.faint));
                            // Built-in shades first.
                            for p in LIGHT_PRESETS {
                                let resp = light_swatch(
                                    ui,
                                    t,
                                    p.rgb,
                                    preset_shown(p, e, current),
                                    t!(lang, p.ru, p.en),
                                );
                                if resp.clicked() {
                                    actions.push(HaAction {
                                        call: ha_service(e, "turn_on", light_preset_call(e, p)),
                                        entity: e.entity_id.clone(),
                                        state: Some("on".into()),
                                    });
                                }
                            }
                            // The lamp's own colour, when no preset or saved
                            // swatch already shows it: the tick then means
                            // "this is what the lamp is doing right now".
                            if let Some(cur) = current {
                                let known =
                                    LIGHT_PRESETS.iter().any(|p| preset_shown(p, e, Some(cur)))
                                        || light_colors.iter().any(|c| rgb_near(*c, cur));
                                if !known {
                                    // A plain button, not the palette helper:
                                    // this shade must not be removable, it is
                                    // the lamp's own state.
                                    let resp = ui
                                        .add(
                                            egui::Button::new(theme::icon(
                                                theme::icon::CHECK,
                                                20.0,
                                                if luma_of(cur) > 145.0 {
                                                    Color32::from_rgb(24, 24, 26)
                                                } else {
                                                    Color32::WHITE
                                                },
                                            ))
                                            .fill(Color32::from_rgb(cur[0], cur[1], cur[2]))
                                            .stroke(Stroke::new(2.5, t.strong))
                                            .corner_radius(CornerRadius::ZERO)
                                            .min_size(Vec2::splat(34.0)),
                                        )
                                        .on_hover_text(format!(
                                            "{} · {}",
                                            light_hex(cur),
                                            t!(lang, "текущий цвет", "current colour")
                                        ));
                                    if resp.clicked() {
                                        actions.push(HaAction {
                                            call: ha_service(
                                                e,
                                                "turn_on",
                                                json!({ "rgb_color": cur }),
                                            ),
                                            entity: e.entity_id.clone(),
                                            state: Some("on".into()),
                                        });
                                    }
                                }
                            }
                            // Saved colours: a tap applies, a long touch (or
                            // a right click) removes. Removal is checked first
                            // because a long touch also reports a click.
                            let mut remove: Option<usize> = None;
                            for (i, rgb) in light_colors.iter().enumerate() {
                                let resp = light_swatch(
                                    ui,
                                    t,
                                    *rgb,
                                    current.is_some_and(|c| rgb_near(c, *rgb)),
                                    format!(
                                        "{} · {}",
                                        light_hex(*rgb),
                                        t!(lang, "удерживайте, чтобы убрать", "hold to remove")
                                    ),
                                );
                                if resp.long_touched() || resp.secondary_clicked() {
                                    remove = Some(i);
                                } else if resp.clicked() {
                                    actions.push(HaAction {
                                        call: ha_service(e, "turn_on", json!({ "rgb_color": rgb })),
                                        entity: e.entity_id.clone(),
                                        state: Some("on".into()),
                                    });
                                }
                            }
                            if let Some(i) = remove {
                                if i < light_colors.len() {
                                    light_colors.remove(i);
                                    *palette_dirty = true;
                                }
                            }
                        });
                        // Any colour, not only the palette. The draft lives in
                        // egui memory per entity, so opening the picker and
                        // choosing a shade survives frames; nothing is sent
                        // until "применить" is pressed.
                        let id = ui.make_persistent_id(("ha-light-colour", e.entity_id.as_str()));
                        let fallback = current.unwrap_or([255, 196, 120]);
                        let mut draft =
                            ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || fallback));
                        ui.horizontal(|ui| {
                            // The stock colour well is only 18 px tall, which
                            // is hard to hit on glass.
                            ui.spacing_mut().interact_size.y = 34.0;
                            ui.label(
                                RichText::new(t!(lang, "свой цвет", "custom colour"))
                                    .size(12.0)
                                    .color(t.muted),
                            );
                            egui::color_picker::color_edit_button_srgb(ui, &mut draft);
                            ui.data_mut(|d| d.insert_temp(id, draft));
                            ui.label(
                                RichText::new(light_hex(draft))
                                    .family(FontFamily::Monospace)
                                    .size(10.5)
                                    .color(t.faint),
                            );
                            if theme::ghost(ui, t!(lang, "применить", "apply"), t).clicked()
                            {
                                actions.push(HaAction {
                                    call: ha_service(e, "turn_on", json!({ "rgb_color": draft })),
                                    entity: e.entity_id.clone(),
                                    state: Some("on".into()),
                                });
                            }
                            // Keep this shade next to the built-ins: the Deck
                            // has no keyboard for a hex value later.
                            let exists = light_colors.contains(&draft);
                            let full = light_colors.len() >= 12;
                            let add = ui.add_enabled(
                                !exists && !full,
                                egui::Button::new(theme::icon(
                                    theme::icon::ADD,
                                    18.0,
                                    if exists || full { t.faint } else { t.ink },
                                ))
                                .fill(t.well)
                                .stroke(Stroke::new(1.0, t.line))
                                .corner_radius(CornerRadius::ZERO)
                                .min_size(Vec2::splat(34.0)),
                            );
                            let tip = if exists {
                                t!(
                                    lang,
                                    "этот цвет уже в палитре",
                                    "this colour is already saved"
                                )
                            } else if full {
                                t!(
                                    lang,
                                    "в палитре уже 12 цветов",
                                    "the palette already holds 12 colours"
                                )
                            } else {
                                t!(
                                    lang,
                                    "добавить цвет в палитру",
                                    "add this colour to the palette"
                                )
                            };
                            if add.on_hover_text(tip).clicked() {
                                light_colors.push(draft);
                                if light_colors.len() > 12 {
                                    let extra = light_colors.len() - 12;
                                    light_colors.drain(..extra);
                                }
                                *palette_dirty = true;
                            }
                        });
                    }
                }
                "cover" => {
                    let position = e.cover_position();
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::ARROW_UP,
                            false,
                            t!(lang, "открыть", "open"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "open_cover", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("open".into()),
                            });
                        }
                        if ha_icon_button(ui, t, theme::icon::STOP, false, t!(lang, "стоп", "stop"))
                        {
                            actions.push(HaAction {
                                call: ha_service(e, "stop_cover", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::ARROW_DOWN,
                            false,
                            t!(lang, "закрыть", "close"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "close_cover", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("closed".into()),
                            });
                        }
                        if let Some(current) = position {
                            let mut pos = i32::from(current);
                            // The slider keeps some room for the percentage
                            // label that follows it in this row.
                            let used = (width - 220.0).max(90.0);
                            ui.spacing_mut().interact_size.y = 30.0;
                            let mut pv = pos as f32;
                            let (_, released, _) = theme::slider_bar(
                                ui,
                                t,
                                used - 60.0,
                                30.0,
                                &mut pv,
                                0.0..=100.0,
                                1.0,
                            );
                            pos = pv.round() as i32;
                            if released {
                                actions.push(HaAction {
                                    call: ha_service(
                                        e,
                                        "set_cover_position",
                                        json!({ "position": pos }),
                                    ),
                                    entity: e.entity_id.clone(),
                                    state: Some(if pos > 0 { "open" } else { "closed" }.into()),
                                });
                            }
                            ui.label(
                                RichText::new(format!("{pos}%"))
                                    .family(FontFamily::Monospace)
                                    .size(11.0)
                                    .color(t.faint),
                            );
                        } else {
                            // A cover that reports no position is not "100 %
                            // open": show what it actually says.
                            ui.label(
                                RichText::new(ha_state_text(e, lang))
                                    .family(FontFamily::Monospace)
                                    .size(11.5)
                                    .color(t.faint),
                            );
                        }
                    });
                }
                "climate" => {
                    let current = e.current_temperature();
                    let mut target = e.target_temperature().or(current).unwrap_or(21.0);
                    ui.horizontal(|ui| {
                        if ha_text_button(ui, t, "−") {
                            target = (target - 0.5).max(5.0);
                            actions.push(HaAction {
                                call: ha_service(
                                    e,
                                    "set_temperature",
                                    json!({ "temperature": target }),
                                ),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        ui.label(
                            RichText::new(format!("{target:.1}°"))
                                .size(23.0)
                                .strong()
                                .color(t.strong),
                        );
                        if ha_text_button(ui, t, "+") {
                            target = (target + 0.5).min(35.0);
                            actions.push(HaAction {
                                call: ha_service(
                                    e,
                                    "set_temperature",
                                    json!({ "temperature": target }),
                                ),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if let Some(c) = current {
                            ui.label(
                                RichText::new(format!("{} {c:.1}°", t!(lang, "сейчас", "now")))
                                    .family(FontFamily::Monospace)
                                    .size(11.0)
                                    .color(t.faint),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ha_icon_button(
                                ui,
                                t,
                                theme::icon::POWER,
                                on,
                                t!(lang, "вкл/выкл", "on/off"),
                            ) {
                                actions.push(HaAction {
                                    call: ha_service(
                                        e,
                                        if on { "turn_off" } else { "turn_on" },
                                        serde_json::Value::Null,
                                    ),
                                    entity: e.entity_id.clone(),
                                    state: Some(if on { "off" } else { "on" }.into()),
                                });
                            }
                        });
                    });
                }
                "media_player" => {
                    let app_name = e.media_app();
                    ui.horizontal(|ui| {
                        ui.label(theme::icon(theme::icon::MUSIC, 17.0, t.faint));
                        ui.label(
                            RichText::new(if app_name.is_empty() {
                                t!(lang, "плеер", "player").to_string()
                            } else {
                                util::truncate(&app_name, 30)
                            })
                            .family(FontFamily::Monospace)
                            .size(10.5)
                            .color(t.faint),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ha_icon_button(
                                ui,
                                t,
                                theme::icon::POWER,
                                on,
                                t!(lang, "вкл/выкл", "on/off"),
                            ) {
                                actions.push(HaAction {
                                    call: ha_toggle(e, !on),
                                    entity: e.entity_id.clone(),
                                    state: Some(if on { "off" } else { "on" }.into()),
                                });
                            }
                        });
                    });
                    if let Some(title) = e.media_title() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(util::truncate(&title, 44))
                                    .size(12.5)
                                    .color(t.muted),
                            )
                            .truncate(),
                        );
                    }
                    if let Some(artist) = e.media_artist() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(util::truncate(&artist, 44))
                                    .size(11.0)
                                    .color(t.faint),
                            )
                            .truncate(),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::PREV,
                            false,
                            t!(lang, "назад", "previous"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(
                                    e,
                                    "media_previous_track",
                                    serde_json::Value::Null,
                                ),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::PLAY_PAUSE,
                            e.state == "playing",
                            t!(lang, "пауза", "play/pause"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "media_play_pause", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::NEXT,
                            false,
                            t!(lang, "дальше", "next"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "media_next_track", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if let Some(vol) = e.attributes.get("volume_level").and_then(|v| v.as_f64())
                        {
                            let mut pct = (vol * 100.0).round() as i32;
                            let mut v = pct as f32;
                            let (_, released, _) = theme::slider_bar(
                                ui,
                                t,
                                width - 80.0,
                                30.0,
                                &mut v,
                                0.0..=100.0,
                                1.0,
                            );
                            pct = v.round() as i32;
                            if released {
                                actions.push(HaAction {
                                    call: ha_service(
                                        e,
                                        "volume_set",
                                        json!({ "volume_level": pct as f64 / 100.0 }),
                                    ),
                                    entity: e.entity_id.clone(),
                                    state: None,
                                });
                            }
                        }
                    });
                }
                "lock" => {
                    let locked = e.state == "locked";
                    ui.horizontal(|ui| {
                        if ha_power_button(ui, t, locked, lang, width) {
                            actions.push(HaAction {
                                call: ha_service(
                                    e,
                                    if locked { "unlock" } else { "lock" },
                                    serde_json::Value::Null,
                                ),
                                entity: e.entity_id.clone(),
                                state: Some(if locked { "unlocked" } else { "locked" }.into()),
                            });
                        }
                        ui.label(
                            RichText::new(ha_state_text(e, lang))
                                .family(FontFamily::Monospace)
                                .size(11.5)
                                .color(t.faint),
                        );
                    });
                }
                "scene" | "script" | "button" => {
                    if ui
                        .add_sized(
                            [width.min(180.0).max(120.0), 38.0],
                            egui::Button::new(
                                RichText::new(t!(lang, "запустить", "run"))
                                    .size(14.0)
                                    .color(accent),
                            )
                            .fill(accent.gamma_multiply(0.12))
                            .stroke(Stroke::new(1.0, accent.gamma_multiply(0.4)))
                            .corner_radius(CornerRadius::ZERO),
                        )
                        .clicked()
                    {
                        actions.push(HaAction {
                            call: ha_toggle(e, true),
                            entity: e.entity_id.clone(),
                            state: None,
                        });
                    }
                }
                "fan" => {
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::POWER,
                            on,
                            t!(lang, "вкл/выкл", "on/off"),
                        ) {
                            actions.push(HaAction {
                                call: ha_toggle(e, !on),
                                entity: e.entity_id.clone(),
                                state: Some(if on { "off" } else { "on" }.into()),
                            });
                        }
                        let mut pct = e
                            .attributes
                            .get("percentage")
                            .and_then(|v| v.as_f64())
                            .map(|v| v.round() as i32)
                            .unwrap_or(if on { 100 } else { 0 });
                        ui.spacing_mut().interact_size.y = 34.0;
                        let mut v = pct as f32;
                        let (_, released, _) = theme::slider_bar(
                            ui,
                            t,
                            (width - 130.0).max(90.0),
                            34.0,
                            &mut v,
                            0.0..=100.0,
                            1.0,
                        );
                        pct = v.round() as i32;
                        if released {
                            actions.push(HaAction {
                                call: ha_service(e, "set_percentage", json!({ "percentage": pct })),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        ui.label(
                            RichText::new(format!("{pct}%"))
                                .family(FontFamily::Monospace)
                                .size(12.0)
                                .color(t.faint),
                        );
                    });
                }
                "number" | "input_number" => {
                    let attr = |key: &str, fallback: f32| {
                        e.attributes
                            .get(key)
                            .and_then(|v| v.as_f64())
                            .map(|v| v as f32)
                            .unwrap_or(fallback)
                    };
                    let min = attr("min", 0.0);
                    let max = attr("max", 100.0).max(min + 0.001);
                    let mut value = e.state.parse::<f32>().unwrap_or(min).clamp(min, max);
                    let (_, released) =
                        theme::slider_ex(ui, t, &e.name(), &mut value, min..=max, |v| {
                            if max <= 10.0 {
                                format!("{v:.1}")
                            } else {
                                format!("{v:.0}")
                            }
                        });
                    if released {
                        actions.push(HaAction {
                            call: ha_service(e, "set_value", json!({ "value": value })),
                            entity: e.entity_id.clone(),
                            state: None,
                        });
                    }
                }
                "select" | "input_select" => {
                    let options: Vec<String> = e
                        .attributes
                        .get("options")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(theme::icon(theme::icon::VIEW, 16.0, t.faint));
                        ui.menu_button(
                            RichText::new(ha_state_text(e, lang))
                                .size(14.0)
                                .color(t.ink),
                            |ui| {
                                for option in &options {
                                    if ui
                                        .selectable_label(&e.state == option, option.as_str())
                                        .clicked()
                                    {
                                        actions.push(HaAction {
                                            call: ha_service(
                                                e,
                                                "select_option",
                                                json!({ "option": option }),
                                            ),
                                            entity: e.entity_id.clone(),
                                            state: Some(option.clone()),
                                        });
                                        ui.close();
                                    }
                                }
                                if options.is_empty() {
                                    ui.label(
                                        RichText::new(t!(lang, "нет вариантов", "no options"))
                                            .size(11.5)
                                            .color(t.muted),
                                    );
                                }
                            },
                        );
                    });
                }
                "vacuum" => {
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::PLAY,
                            e.state == "cleaning",
                            t!(lang, "старт", "start"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "start", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("cleaning".into()),
                            });
                        }
                        if ha_icon_button(ui, t, theme::icon::STOP, false, t!(lang, "стоп", "stop"))
                        {
                            actions.push(HaAction {
                                call: ha_service(e, "stop", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("idle".into()),
                            });
                        }
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::HOME,
                            false,
                            t!(lang, "на базу", "dock"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "return_to_base", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("returning".into()),
                            });
                        }
                        ui.label(
                            RichText::new(ha_state_text(e, lang))
                                .family(FontFamily::Monospace)
                                .size(11.5)
                                .color(t.faint),
                        );
                    });
                }
                "valve" => {
                    ui.horizontal(|ui| {
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::ARROW_UP,
                            false,
                            t!(lang, "открыть", "open"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "open_valve", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("open".into()),
                            });
                        }
                        if ha_icon_button(ui, t, theme::icon::STOP, false, t!(lang, "стоп", "stop"))
                        {
                            actions.push(HaAction {
                                call: ha_service(e, "stop_valve", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                        if ha_icon_button(
                            ui,
                            t,
                            theme::icon::ARROW_DOWN,
                            false,
                            t!(lang, "закрыть", "close"),
                        ) {
                            actions.push(HaAction {
                                call: ha_service(e, "close_valve", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: Some("closed".into()),
                            });
                        }
                    });
                }
                "update" => {
                    let latest = e
                        .attributes
                        .get("latest_version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let state_text = ha_state_text(e, lang);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if latest.is_empty() {
                                state_text.clone()
                            } else {
                                format!("{state_text} → {latest}")
                            })
                            .family(FontFamily::Monospace)
                            .size(12.0)
                            .color(t.muted),
                        );
                        if e.state == "on"
                            && ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(t!(lang, "обновить", "install"))
                                            .size(13.0)
                                            .color(accent),
                                    )
                                    .fill(accent.gamma_multiply(0.12))
                                    .stroke(Stroke::new(1.0, accent.gamma_multiply(0.4)))
                                    .corner_radius(CornerRadius::ZERO)
                                    .min_size(Vec2::new(0.0, 30.0)),
                                )
                                .clicked()
                        {
                            actions.push(HaAction {
                                call: ha_service(e, "install", serde_json::Value::Null),
                                entity: e.entity_id.clone(),
                                state: None,
                            });
                        }
                    });
                }
                "sensor" | "binary_sensor" | "weather" | "person" | "device_tracker" | "event" => {
                    let text = ha_state_text(e, lang);
                    // Long values (an uptime, a formatted timestamp) shrink
                    // instead of running into the star and menu buttons.
                    let size = if text.chars().count() > 13 {
                        15.0
                    } else if text.chars().count() > 8 {
                        19.0
                    } else {
                        22.0
                    };
                    ui.add(
                        egui::Label::new(RichText::new(text).size(size).strong().color(if on {
                            t.accent
                        } else {
                            t.strong
                        }))
                        .truncate(),
                    );
                }
                _ => {
                    if sdw_core::ha::is_controllable(e) {
                        ui.horizontal(|ui| {
                            if ha_power_button(ui, t, on, lang, width) {
                                actions.push(HaAction {
                                    call: ha_toggle(e, !on),
                                    entity: e.entity_id.clone(),
                                    state: Some(if on { "off" } else { "on" }.into()),
                                });
                            }
                            ui.label(
                                RichText::new(ha_state_text(e, lang))
                                    .family(FontFamily::Monospace)
                                    .size(11.5)
                                    .color(t.faint),
                            );
                        });
                    } else {
                        ui.label(
                            RichText::new(ha_state_text(e, lang))
                                .size(13.5)
                                .color(t.muted),
                        );
                    }
                }
            }
        });
}

/// Draw a QR code in the Grid language: a white field, black square modules,
/// no rounding. The page it points to carries a random path, so a photo of the
/// screen is useless once the card is closed.
fn qr_code(ui: &mut Ui, text: &str, side: f32) {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return;
    };
    let n = code.width();
    let colors = code.to_colors();
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::WHITE);
    let pad = 8.0;
    let module = (side - pad * 2.0) / n as f32;
    let origin = rect.min + Vec2::splat(pad);
    for y in 0..n {
        for x in 0..n {
            if colors[y * n + x] == qrcode::Color::Dark {
                painter.rect_filled(
                    Rect::from_min_size(
                        origin + Vec2::new(x as f32 * module, y as f32 * module),
                        Vec2::splat(module),
                    ),
                    CornerRadius::ZERO,
                    Color32::BLACK,
                );
            }
        }
    }
}

/// The servers the deck knows: the computer's own connection and every Home
/// Assistant the deck keeps for itself, plus the form that adds or edits one.
fn ha_servers(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    let instances: Vec<(String, String, String)> = app
        .cfg
        .ha_instances
        .iter()
        .map(|i| (i.id.clone(), i.label(), i.url.clone()))
        .collect();
    let active = app.cfg.ha_active.clone();
    let computer = app
        .status
        .as_ref()
        .map(|s| (s.ha.configured, s.ha.url.clone()))
        .unwrap_or((false, String::new()));
    let mut select: Option<String> = None;
    let mut edit: Option<String> = None;
    let mut remove: Option<String> = None;
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t!(lang, "серверы", "servers"))
                    .size(16.0)
                    .strong()
                    .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    lang,
                    "компьютер или свои серверы; выбранный кормит страницу.",
                    "the computer or your own servers; the chosen one feeds the page."
                ))
                .size(12.0)
                .color(t.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if !app.ha.form_open
                    && theme::ghost(ui, t!(lang, "добавить сервер", "add a server"), t).clicked()
                {
                    app.new_ha_instance();
                }
            });
        });
        ui.add_space(6.0);
        // The computer's own connection is a row like the rest, without a
        // token of its own: the daemon holds it.
        ui.horizontal(|ui| {
            let chosen = active.trim().is_empty();
            theme::dot(ui, if chosen { t.good } else { t.faint }, true);
            if ui
                .selectable_label(
                    chosen,
                    RichText::new(t!(lang, "компьютер", "the computer")).size(13.5),
                )
                .clicked()
            {
                select = Some(String::new());
            }
            let note = if computer.0 {
                util::truncate(&computer.1, 46)
            } else {
                t!(
                    lang,
                    "home assistant на компьютере не настроен",
                    "no home assistant on the computer"
                )
                .to_string()
            };
            ui.label(
                RichText::new(note)
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.faint),
            );
        });
        for (id, label, url) in &instances {
            ui.horizontal(|ui| {
                let chosen = &active == id;
                theme::dot(ui, if chosen { t.good } else { t.faint }, true);
                if ui
                    .selectable_label(chosen, RichText::new(label).size(13.5))
                    .clicked()
                {
                    select = Some(id.clone());
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(util::truncate(url, 40))
                            .family(FontFamily::Monospace)
                            .size(10.5)
                            .color(t.faint),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::ghost(ui, t!(lang, "удалить", "delete"), t).clicked() {
                        remove = Some(id.clone());
                    }
                    if theme::ghost(ui, t!(lang, "изменить", "change"), t).clicked() {
                        edit = Some(id.clone());
                    }
                });
            });
        }
        ui.add_space(6.0);
        theme::separator(ui, t);
        ui.add_space(4.0);
        if app.ha.form_open || instances.is_empty() {
            ha_form(app, ui, t);
        }
    });
    if let Some(id) = select {
        app.select_ha(&id);
    }
    if let Some(id) = edit {
        app.edit_ha_instance(&id);
    }
    if let Some(id) = remove {
        app.remove_ha_instance(&id);
    }
}

/// The form that connects one server: its name, its address and a long-lived
/// token, or the temporary page that takes them from a phone.
fn ha_form(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let lang = app.lang;
    let editing = app.ha.edit_id.is_some();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(if editing {
                t!(lang, "изменить сервер", "edit the server")
            } else if app.cfg.ha_instances.is_empty() {
                t!(lang, "подключить home assistant", "connect home assistant")
            } else {
                t!(lang, "добавить сервер", "add a server")
            })
            .size(18.0)
            .strong()
            .color(t.strong),
        );
        if app.ha.form_open || editing {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
                    app.cancel_ha_form();
                }
            });
        }
    });
    // While the phone page is open the form steps back: the token is being
    // typed somewhere else, and two ways in at once would only confuse.
    if app.ha_link.is_some() {
        ui.label(
            RichText::new(t!(
                lang,
                "откройте страницу на телефоне или компьютере и вставьте токен — дека подхватит его сама.",
                "open the page on your phone or computer and paste the token — the deck takes it from there."
            ))
            .size(13.0)
            .color(t.muted),
        );
        let url = app
            .ha_link
            .as_ref()
            .map(|link| link.url.clone())
            .unwrap_or_default();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            qr_code(ui, &url, 168.0);
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.label(theme::hud(t!(lang, "адрес", "address"), t.faint));
                ui.add(
                    egui::Label::new(
                        RichText::new(&url)
                            .family(FontFamily::Monospace)
                            .size(13.0)
                            .color(t.strong),
                    )
                    .truncate(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(t!(lang, "ждём токен", "waiting for the token"))
                            .size(12.5)
                            .color(t.muted),
                    );
                });
                ui.add_space(8.0);
                if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
                    app.cancel_ha_link();
                }
            });
        });
        return;
    }
    ui.label(
        RichText::new(t!(
            lang,
            "адрес и долгоживущий токен из профиля home assistant.",
            "the address and a long-lived token from your home assistant profile."
        ))
        .size(13.0)
        .color(t.muted),
    );
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut app.ha.instance_name)
                .hint_text(t!(lang, "имя, например «дом»", "a name, e.g. “home”"))
                .desired_width(240.0),
        );
        ui.label(
            RichText::new(t!(lang, "необязательно", "optional"))
                .size(11.0)
                .color(t.faint),
        );
    });
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
                .hint_text(t!(lang, "токен", "token"))
                .desired_width(380.0),
        );
        if ui
            .selectable_label(app.ha.token_show, t!(lang, "показать", "show"))
            .clicked()
        {
            app.ha.token_show = !app.ha.token_show;
        }
    });
    if !app.ha.error.is_empty() {
        ui.label(RichText::new(&app.ha.error).size(12.5).color(t.bad));
    }
    let mut connect = false;
    ui.horizontal(|ui| {
        if theme::primary(ui, t!(lang, "подключить", "connect"), t).clicked() {
            connect = true;
        }
        if theme::ghost(ui, t!(lang, "ввести с телефона", "enter from a phone"), t).clicked()
        {
            app.start_ha_link();
        }
    });
    if connect {
        let name = app.ha.instance_name.clone();
        let url = app.ha.url.trim().to_string();
        let token = app.ha.token.trim().to_string();
        if url.is_empty() || token.is_empty() {
            app.ha.error = t!(
                lang,
                "нужны адрес и токен",
                "the address and the token are needed"
            )
            .to_string();
        } else {
            // The deck keeps the connection itself: no computer needed, and
            // the token never leaves this device.
            app.save_ha_instance(&name, &url, &token);
        }
    }
}

pub fn home_assistant(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let ha = app.ha_status();
    let favorites = ha.favorites.clone();
    let categories = app.ha.categories.clone();
    page_title(
        ui,
        t,
        t!(app.lang, "home assistant", "home assistant"),
        t!(
            app.lang,
            "комнаты, свет, шторы и климат — плитками, без браузера.",
            "rooms, lights, covers and climate — as tiles, no browser."
        ),
    );

    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            theme::dot(ui, if ha.connected { t.good } else { t.bad }, true);
            ui.label(
                RichText::new(if ha.connected {
                    t!(app.lang, "подключено", "connected")
                } else if ha.configured {
                    t!(app.lang, "настроено, нет связи", "configured, offline")
                } else {
                    t!(app.lang, "не настроено", "not configured")
                })
                .size(16.0)
                .strong()
                .color(if ha.connected { t.good } else { t.strong }),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if app.ha.servers_open {
                    // The list has its own "done"; the page header keeps one
                    // place to leave it.
                    if theme::ghost(ui, t!(app.lang, "готово", "done"), t).clicked() {
                        app.ha.servers_open = false;
                    }
                } else if theme::ghost(ui, t!(app.lang, "серверы", "servers"), t).clicked() {
                    app.ha.servers_open = true;
                }
                if ha.configured
                    && theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked()
                {
                    app.ha.last_fetch =
                        std::time::Instant::now() - std::time::Duration::from_secs(60);
                }
            });
        });
        if ha.configured {
            // The summary gets its own line: squeezing it beside the status
            // and the refresh button is what made the header overlap. The name
            // of the chosen server is there when it is one of the deck's own.
            let source = app.ha_active_label();
            let head = match source {
                Some(name) => format!(
                    "{} · {}",
                    util::truncate(&name, 24),
                    util::truncate(&ha.url, 32)
                ),
                None => format!(
                    "{} · {}",
                    t!(app.lang, "компьютер", "the computer"),
                    util::truncate(&ha.url, 40)
                ),
            };
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{} · {} {} · {} {}",
                        head,
                        ha.entities,
                        t!(app.lang, "сущностей", "entities"),
                        ha_rooms_count(app),
                        t!(app.lang, "комнат", "rooms")
                    ))
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.muted),
                )
                .truncate(),
            );
        }
        if ha.configured && !ha.error.is_empty() && !ha.connected {
            ui.label(
                RichText::new(util::truncate(&ha.error, 90))
                    .size(11.5)
                    .color(t.bad),
            );
        }
    });

    if app.ha.servers_open {
        ui.add_space(8.0);
        ha_servers(app, ui, t);
    }

    if !ha.configured {
        if app.ha.servers_open {
            // The server card already carries the form (or the list of
            // servers); nothing else to connect here.
            return;
        }
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ha_form(app, ui, t);
        });
        return;
    }

    if app.ha.entities.is_empty() {
        ui.add_space(8.0);
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

    // ---------------------------------------------------------------- categories

    if app.ha.manage {
        ui.add_space(8.0);
        let mut list = app.ha.categories.clone();
        let mut new_name = app.ha.new_category.clone();
        let mut changed = false;
        theme::card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(app.lang, "категории", "categories"))
                        .size(16.0)
                        .strong()
                        .color(t.strong),
                );
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "порядок вкладок и порядок плиток внутри них.",
                        "tab order and tile order inside each one."
                    ))
                    .size(12.0)
                    .color(t.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::ghost(ui, t!(app.lang, "готово", "done"), t).clicked() {
                        app.ha.manage = false;
                    }
                });
            });
            ui.add_space(6.0);
            let mut remove: Option<usize> = None;
            let mut swap: Option<(usize, usize)> = None;
            let count = list.len();
            for (i, c) in list.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(TextEdit::singleline(&mut c.name).desired_width(220.0))
                        .lost_focus()
                    {
                        changed = true;
                    }
                    ui.label(
                        RichText::new(format!("{}", c.entities.len()))
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.faint),
                    );
                    if ha_move_button(
                        ui,
                        t,
                        theme::icon::ARROW_LEFT,
                        t!(app.lang, "левее", "move left"),
                        i > 0,
                    ) {
                        swap = Some((i, i - 1));
                    }
                    if ha_move_button(
                        ui,
                        t,
                        theme::icon::ARROW_RIGHT,
                        t!(app.lang, "правее", "move right"),
                        i + 1 < count,
                    ) {
                        swap = Some((i, i + 1));
                    }
                    if ui
                        .add(
                            egui::Button::new(theme::icon(theme::icon::CLOSE, 16.0, t.bad))
                                .fill(t.well)
                                .stroke(Stroke::new(1.0, t.line))
                                .corner_radius(CornerRadius::ZERO)
                                .min_size(Vec2::splat(30.0)),
                        )
                        .clicked()
                    {
                        remove = Some(i);
                    }
                });
            }
            if let Some((a, b)) = swap {
                if a < list.len() && b < list.len() {
                    list.swap(a, b);
                    changed = true;
                }
            }
            if let Some(i) = remove {
                if i < list.len() {
                    list.remove(i);
                    changed = true;
                }
            }
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut new_name)
                        .hint_text(t!(app.lang, "новая категория", "new category"))
                        .desired_width(240.0),
                );
                if theme::ghost(ui, t!(app.lang, "добавить", "add"), t).clicked()
                    && !new_name.trim().is_empty()
                {
                    list.push(HaCategory::new(&new_name));
                    new_name.clear();
                    changed = true;
                }
            });
        });
        app.ha.new_category = new_name;
        if changed {
            app.ha.categories = list;
            app.save_ha_categories();
        }
    }

    // ---------------------------------------------------------------- presets

    if app.ha.presets_open {
        ui.add_space(8.0);
        let mut list = app.ha.presets.clone();
        let mut name = app.ha.preset_name.clone();
        let mut apply: Option<usize> = None;
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let mut recapture: Option<usize> = None;
        let mut changed = false;
        let mut save_current = false;
        let count = list.len();
        let manage = app.ha.manage;
        theme::card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(t!(app.lang, "пресеты", "presets"))
                        .size(16.0)
                        .strong()
                        .color(t.strong),
                );
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "яркость, температура и цвет всех включённых ламп одним нажатием.",
                        "brightness, temperature and colour of every lit lamp in one tap."
                    ))
                    .size(12.0)
                    .color(t.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::ghost(ui, t!(app.lang, "готово", "done"), t).clicked() {
                        app.ha.presets_open = false;
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut name)
                        .hint_text(t!(app.lang, "имя нового пресета", "new preset name"))
                        .desired_width(260.0),
                );
                if theme::ghost(
                    ui,
                    t!(app.lang, "сохранить текущий свет", "capture current lights"),
                    t,
                )
                .on_hover_text(t!(
                    app.lang,
                    "снимок яркости и цвета всех включённых ламп",
                    "snapshot of brightness and colour of every lamp that is on"
                ))
                .clicked()
                {
                    save_current = true;
                }
            });
            if list.is_empty() {
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "пока нет пресетов. включите лампы, введите имя и нажмите «сохранить текущий свет».",
                        "no presets yet. turn lamps on, type a name and hit “capture current lights”."
                    ))
                    .size(12.0)
                    .color(t.muted),
                );
            }
            for (i, p) in list.iter_mut().enumerate() {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let label = format!("{} · {}", p.name, p.actions.len());
                    let resp = ui.add(
                        egui::Button::new(RichText::new(label).size(14.0).color(t.ink))
                            .fill(t.well)
                            .stroke(Stroke::new(1.0, t.line_strong))
                            .corner_radius(CornerRadius::ZERO)
                            .min_size(Vec2::new(0.0, 42.0)),
                    );
                    if resp
                        .on_hover_text(t!(app.lang, "применить пресет", "apply preset"))
                        .clicked()
                    {
                        apply = Some(i);
                    }
                    if manage {
                        if ui
                            .add(TextEdit::singleline(&mut p.name).desired_width(180.0))
                            .lost_focus()
                        {
                            changed = true;
                        }
                        if ha_move_button(
                            ui,
                            t,
                            theme::icon::ARROW_UP,
                            t!(app.lang, "выше", "move up"),
                            i > 0,
                        ) {
                            swap = Some((i, i - 1));
                        }
                        if ha_move_button(
                            ui,
                            t,
                            theme::icon::ARROW_DOWN,
                            t!(app.lang, "ниже", "move down"),
                            i + 1 < count,
                        ) {
                            swap = Some((i, i + 1));
                        }
                        if ha_move_button(
                            ui,
                            t,
                            theme::icon::CHECK,
                            t!(
                                app.lang,
                                "переснять с текущего света",
                                "re-capture current lights"
                            ),
                            true,
                        ) {
                            recapture = Some(i);
                        }
                        if ui
                            .add(
                                egui::Button::new(theme::icon(theme::icon::CLOSE, 16.0, t.bad))
                                    .fill(t.well)
                                    .stroke(Stroke::new(1.0, t.line))
                                    .corner_radius(CornerRadius::ZERO)
                                    .min_size(Vec2::splat(30.0)),
                            )
                            .on_hover_text(t!(app.lang, "удалить пресет", "delete preset"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    }
                });
            }
        });
        app.ha.preset_name = name;
        if save_current {
            let new_name = app.ha.preset_name.clone();
            app.save_preset_from_lights(&new_name);
        }
        if let Some((a, b)) = swap {
            if a < list.len() && b < list.len() {
                list.swap(a, b);
                changed = true;
            }
        }
        if let Some(i) = remove {
            if i < list.len() {
                list.remove(i);
                changed = true;
            }
        }
        if let Some(i) = recapture {
            let snapshot = sdw_core::ha::capture_light_preset(&app.ha.entities);
            if !snapshot.is_empty() {
                if let Some(p) = list.get_mut(i) {
                    p.actions = snapshot;
                    changed = true;
                }
            }
        }
        if let Some(i) = apply {
            if let Some(p) = app.ha.presets.get(i).cloned() {
                app.apply_preset(&p);
            }
        }
        if changed {
            app.ha.presets = list;
            app.save_ha_presets();
        }
    }

    // ---------------------------------------------------------------- rooms

    let show_all = app.ha.show_offline;
    let mut area_counts: Vec<(String, usize)> = Vec::new();
    for area in &app.ha.areas {
        let count = app
            .ha
            .entities
            .iter()
            .filter(|e| &e.area == area && ha_visible_with(e, show_all))
            .count();
        area_counts.push((area.clone(), count));
    }
    for e in &app.ha.entities {
        if e.area.is_empty() {
            continue;
        }
        if !area_counts.iter().any(|(a, _)| a == &e.area) {
            let count = app
                .ha
                .entities
                .iter()
                .filter(|x| x.area == e.area && ha_visible_with(x, show_all))
                .count();
            area_counts.push((e.area.clone(), count));
        }
    }
    area_counts.retain(|(_, n)| *n > 0);

    let mut rooms: Vec<(HaRoom, String, usize)> = Vec::new();
    let all_count = app
        .ha
        .entities
        .iter()
        .filter(|e| ha_visible_with(e, show_all))
        .count();
    rooms.push((
        HaRoom::All,
        t!(app.lang, "весь дом", "whole home").to_string(),
        all_count,
    ));
    // The dashboard's own tabs come first: this is the page the user built in
    // Home Assistant, not a second opinion invented by the daemon.
    for (i, view) in app.ha.views.iter().enumerate() {
        let count = view
            .entities
            .iter()
            .filter(|id| {
                app.ha
                    .entities
                    .iter()
                    .find(|e| &e.entity_id == *id)
                    .map(|e| ha_visible_with(e, show_all))
                    .unwrap_or(false)
            })
            .count();
        rooms.push((HaRoom::View(i), view.title.clone(), count));
    }
    if !favorites.is_empty() {
        rooms.push((
            HaRoom::Favorites,
            format!("★ {}", t!(app.lang, "избранное", "favorites")),
            favorites.len(),
        ));
    }
    for (i, c) in app.ha.categories.iter().enumerate() {
        rooms.push((HaRoom::Category(i), c.name.clone(), c.entities.len()));
    }
    for (area, count) in &area_counts {
        rooms.push((HaRoom::Area(area.clone()), area.clone(), *count));
    }
    let has_unassigned = app
        .ha
        .entities
        .iter()
        .any(|e| e.area.is_empty() && ha_visible_with(e, show_all));
    if !area_counts.is_empty() && has_unassigned {
        rooms.push((
            HaRoom::Unassigned,
            t!(app.lang, "без комнаты", "no room").to_string(),
            app.ha
                .entities
                .iter()
                .filter(|e| e.area.is_empty() && ha_visible_with(e, show_all))
                .count(),
        ));
    }
    if app.ha.room >= rooms.len() {
        app.ha.room = 0;
    }
    let room = rooms[app.ha.room].0.clone();
    let needle = app.ha.search.trim().to_lowercase();
    let searching = !needle.is_empty();

    // ---------------------------------------------------------------- toolbar

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if !searching {
            let n = rooms.len();
            let prev = if app.ha.room == 0 {
                n - 1
            } else {
                app.ha.room - 1
            };
            if theme::nav_arrow(ui, t, theme::icon::ARROW_LEFT, n > 1).clicked() && n > 1 {
                app.ha.room = prev;
            }
            egui::ComboBox::from_id_salt("ha-room")
                .selected_text(
                    RichText::new(rooms[app.ha.room].1.clone())
                        .size(16.0)
                        .strong(),
                )
                .width(280.0)
                .show_ui(ui, |ui| {
                    for (i, (_, label, count)) in rooms.iter().enumerate() {
                        ui.selectable_value(&mut app.ha.room, i, format!("{label} · {count}"));
                    }
                });
            if theme::nav_arrow(ui, t, theme::icon::ARROW_RIGHT, n > 1).clicked() && n > 1 {
                app.ha.room = (app.ha.room + 1) % n;
            }
            ui.label(
                RichText::new(format!("{} / {}", app.ha.room + 1, rooms.len()))
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.faint),
            );
            ui.add_space(8.0);
        }
        ui.add(
            TextEdit::singleline(&mut app.ha.search)
                .hint_text(t!(app.lang, "поиск по имени", "search by name"))
                .desired_width(220.0),
        );
        if searching && theme::ghost(ui, t!(app.lang, "сбросить", "clear"), t).clicked() {
            app.ha.search.clear();
        }
    });
    // The action row is separate: the search field and three labels do not fit
    // beside the room switcher on the Deck and used to overlap it.
    ui.horizontal_wrapped(|ui| {
        if theme::ghost(
            ui,
            if app.ha.presets_open {
                t!(app.lang, "скрыть пресеты", "hide presets")
            } else {
                t!(app.lang, "пресеты", "presets")
            },
            t,
        )
        .clicked()
        {
            app.ha.presets_open = !app.ha.presets_open;
        }
        if theme::ghost(
            ui,
            if app.ha.manage {
                t!(app.lang, "скрыть категории", "hide categories")
            } else {
                t!(app.lang, "категории", "categories")
            },
            t,
        )
        .clicked()
        {
            app.ha.manage = !app.ha.manage;
        }
        if theme::ghost(
            ui,
            if app.ha.show_offline {
                t!(app.lang, "скрыть недоступные", "hide unavailable")
            } else {
                t!(app.lang, "недоступные", "unavailable")
            },
            t,
        )
        .clicked()
        {
            app.ha.show_offline = !app.ha.show_offline;
            app.ha.page = 0;
        }
    });
    ui.add_space(6.0);

    // Connection state belongs on the page, not in a hidden tooltip: data
    // that stopped updating must never look live.
    if app.ha.loaded && (!app.ha.connected || app.ha.age_ms > 20_000) {
        let stale = app.ha.connected;
        let message = if stale {
            format!(
                "{} {}",
                t!(app.lang, "данные устарели:", "data is stale:"),
                sdw_core::ha::human_duration(app.ha.age_ms as f64 / 1000.0, app.lang)
            )
        } else if app.ha.stale_error.is_empty() {
            t!(
                app.lang,
                "нет связи с home assistant",
                "no connection to home assistant"
            )
            .to_string()
        } else {
            util::truncate(&app.ha.stale_error, 140)
        };
        theme::card(ui, t, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::icon(theme::icon::SENSORS, 17.0, t.warn));
                ui.label(RichText::new(message).size(12.5).color(if stale {
                    t.warn
                } else {
                    t.bad
                }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::ghost(ui, t!(app.lang, "обновить", "retry"), t).clicked() {
                        app.ha.last_fetch =
                            std::time::Instant::now() - std::time::Duration::from_secs(30);
                        app.load_ha();
                    }
                });
            });
        });
        ui.add_space(6.0);
    }

    // ---------------------------------------------------------------- entities

    let current_category = match &room {
        HaRoom::Category(i) => Some(*i),
        _ => None,
    };
    let optimistic = app.ha.optimistic.clone();
    let mut entities: Vec<HaEntity> = if searching {
        app.ha
            .entities
            .iter()
            .filter(|e| {
                let name = e.name().to_lowercase();
                name.contains(&needle) || e.entity_id.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    } else {
        match &room {
            HaRoom::Favorites => app
                .ha
                .entities
                .iter()
                .filter(|e| favorites.contains(&e.entity_id))
                .cloned()
                .collect(),
            HaRoom::Category(i) => {
                let order = app
                    .ha
                    .categories
                    .get(*i)
                    .map(|c| c.entities.clone())
                    .unwrap_or_default();
                let mut out = Vec::new();
                for id in order {
                    if let Some(e) = app.ha.entities.iter().find(|e| e.entity_id == id) {
                        out.push(e.clone());
                    }
                }
                out
            }
            HaRoom::View(i) => {
                let order = app
                    .ha
                    .views
                    .get(*i)
                    .map(|v| v.entities.clone())
                    .unwrap_or_default();
                order
                    .iter()
                    .filter_map(|id| app.ha.entities.iter().find(|e| &e.entity_id == id))
                    .filter(|e| ha_visible_with(e, show_all))
                    .cloned()
                    .collect()
            }
            HaRoom::Area(a) => app
                .ha
                .entities
                .iter()
                .filter(|e| &e.area == a && ha_visible_with(e, show_all))
                .cloned()
                .collect(),
            HaRoom::Unassigned => app
                .ha
                .entities
                .iter()
                .filter(|e| e.area.is_empty() && ha_visible_with(e, show_all))
                .cloned()
                .collect(),
            HaRoom::All => app
                .ha
                .entities
                .iter()
                .filter(|e| ha_visible_with(e, show_all))
                .cloned()
                .collect(),
        }
    };
    for e in &mut entities {
        if let Some((state, at)) = optimistic.get(&e.entity_id) {
            if at.elapsed() < std::time::Duration::from_secs(6) {
                e.state = state.clone();
            }
        }
        // A slider just released: show its value until Home Assistant
        // reports the same brightness, otherwise it jumps back and forth.
        if let Some((pct, at)) = app.ha.optimistic_pct.get(&e.entity_id) {
            if at.elapsed() < std::time::Duration::from_secs(8) {
                let raw = ((u16::from(*pct) * 255 + 50) / 100).min(255) as u64;
                if let Some(map) = e.attributes.as_object_mut() {
                    map.insert("brightness".into(), serde_json::json!(raw));
                }
            }
        }
        // A just-pressed swatch keeps its tick until the lamp answers.
        if let Some(c) = app.ha.optimistic_rgb.get(&e.entity_id) {
            if c.at.elapsed() < std::time::Duration::from_secs(8) {
                if let Some(map) = e.attributes.as_object_mut() {
                    map.insert("rgb_color".into(), serde_json::json!(c.rgb));
                    map.insert("color_mode".into(), serde_json::json!("hs"));
                    if c.kelvin {
                        map.remove("color_temp_kelvin");
                    }
                }
            }
        }
    }
    let priority = |d: &str| match d {
        "light" => 0,
        "cover" => 1,
        "climate" => 2,
        "media_player" => 3,
        "lock" => 4,
        "switch" | "input_boolean" => 5,
        "fan" => 6,
        "scene" | "script" | "button" => 7,
        "sensor" | "binary_sensor" | "weather" | "person" | "device_tracker" => 20,
        _ => 10,
    };
    if !matches!(room, HaRoom::Category(_)) {
        entities.sort_by_key(|e| (priority(e.domain()), e.name().to_lowercase()));
    }

    if entities.is_empty() {
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(if searching {
                    t!(app.lang, "ничего не найдено", "nothing found")
                } else if matches!(room, HaRoom::Category(_)) {
                    t!(
                        app.lang,
                        "категория пуста. откройте «весь дом» и добавьте плитки через меню на карточке.",
                        "this category is empty. open “whole home” and add tiles from the card menu."
                    )
                } else {
                    t!(app.lang, "в этой комнате пока нет устройств", "no devices in this room yet")
                })
                .size(13.5)
                .color(t.muted),
            );
        });
        return;
    }

    // A whole home can hold hundreds of tiles. Rendering all of them at once
    // costs frames and scrolls worse than a fixed page, so show one page and
    // reset it whenever the room or the search changes.
    if app.ha.page_room != app.ha.room || app.ha.page_needle != app.ha.search {
        app.ha.page = 0;
        app.ha.page_room = app.ha.room;
        app.ha.page_needle = app.ha.search.clone();
    }
    let total_found = entities.len();
    let page_size = 48usize;
    let pages = total_found.div_ceil(page_size).max(1);
    if app.ha.page >= pages {
        app.ha.page = pages - 1;
    }
    let page = app.ha.page;
    let from = page * page_size;
    let to = (from + page_size).min(total_found);
    entities = entities[from..to].to_vec();

    let mut actions: Vec<HaAction> = Vec::new();
    let mut bright: Vec<(String, u8)> = Vec::new();
    let mut star: Option<String> = None;
    let mut category_change: Option<(String, Option<String>)> = None;
    let mut move_entity: Option<(usize, String, i32)> = None;
    let lang = app.lang;
    // The palette is written back once the whole page has been laid out.
    let light_colors = &mut app.cfg.light_colors;
    let mut palette_dirty = false;

    ha_pager(ui, t, &mut app.ha.page, pages, total_found, app.lang);

    // Group tiles by domain so the page reads in sections.
    let mut groups: Vec<(&str, Vec<&HaEntity>)> = Vec::new();
    for e in &entities {
        let d = e.domain();
        if let Some((_, list)) = groups.iter_mut().find(|(g, _)| *g == d) {
            list.push(e);
        } else {
            groups.push((d, vec![e]));
        }
    }
    groups.sort_by_key(|(d, _)| priority(d));
    let width = ui.available_width();
    // As many columns as fit without squeezing a card below a comfortable
    // reading width; the Deck lands on two, a small window on one.
    let cols = (((width + 14.0) / 520.0).floor() as usize).clamp(1, 3);

    for (domain, list) in groups {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(sdw_core::ha::domain_label(domain, app.lang).to_lowercase())
                    .family(FontFamily::Monospace)
                    .size(11.0)
                    .extra_letter_spacing(1.2)
                    .color(t.faint),
            );
            ui.label(
                RichText::new(format!("{}", list.len()))
                    .family(FontFamily::Monospace)
                    .size(10.0)
                    .color(t.faint),
            );
        });
        for row in list.chunks(cols) {
            ui.columns(cols, |columns| {
                for (i, e) in row.iter().enumerate() {
                    let col = &mut columns[i];
                    // The card frame has 14 px margins on both sides; pass the
                    // exact content width so its children can never overflow.
                    let inner = (col.available_width() - 28.0).max(120.0);
                    col.push_id(e.entity_id.as_str(), |ui| {
                        ha_card(
                            ui,
                            t,
                            lang,
                            e,
                            inner,
                            &favorites,
                            &categories,
                            current_category,
                            &mut actions,
                            &mut bright,
                            &mut star,
                            &mut category_change,
                            &mut move_entity,
                            light_colors,
                            &mut palette_dirty,
                        );
                    });
                }
            });
            ui.add_space(8.0);
        }
    }

    for (id, pct) in bright {
        app.ha
            .optimistic_pct
            .insert(id, (pct, std::time::Instant::now()));
    }
    if palette_dirty {
        // The palette is small, so a deferred save is enough; `save_soon`
        // coalesces a whole session of swatch edits into one write.
        app.save_soon();
    }
    let acted = !actions.is_empty();
    for a in actions {
        if let Some(state) = a.state {
            app.ha
                .optimistic
                .insert(a.entity.clone(), (state, std::time::Instant::now()));
        }
        // Remember a pressed colour so its swatch shows the tick at once, and
        // forget an older one when the lamp is switched back to white.
        let rgb = a
            .call
            .data
            .get("rgb_color")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                if arr.len() < 3 {
                    return None;
                }
                let c = |i: usize| arr[i].as_u64().map(|n| n.min(255) as u8).unwrap_or(0);
                Some([c(0), c(1), c(2)])
            });
        // A white preset is sent as a temperature: its swatch still has to
        // tick at once, so borrow the RGB the preset stands for. The window is
        // wider than the matching one because the value may have been clamped
        // to the lamp's own range (a 6500 K preset on a 6000 K bulb).
        let kelvin = a
            .call
            .data
            .get("color_temp_kelvin")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let kelvin_rgb = kelvin.and_then(|k| {
            LIGHT_PRESETS
                .iter()
                .find(|p| p.kelvin.is_some_and(|pk| pk.abs_diff(k) <= 600))
                .map(|p| p.rgb)
        });
        if let Some(rgb) = rgb.or(kelvin_rgb) {
            app.ha.optimistic_rgb.insert(
                a.entity.clone(),
                crate::app::OptimisticColor {
                    rgb,
                    kelvin: kelvin.is_some(),
                    at: std::time::Instant::now(),
                },
            );
        } else if kelvin.is_some() {
            // The temperature slider: it has no swatch of its own, and the
            // lamp answers in a moment anyway.
            app.ha.optimistic_rgb.remove(&a.entity);
        }
        app.ha_call(a.call);
    }
    if acted {
        // Ask for the real state shortly after the service call lands.
        app.ha.last_fetch = std::time::Instant::now() - std::time::Duration::from_secs(4);
    }

    if let Some(id) = star {
        let mut favorites = favorites.clone();
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

    if let Some((entity, target)) = category_change {
        for c in &mut app.ha.categories {
            c.entities.retain(|x| x != &entity);
        }
        let mut cat_name = t!(app.lang, "без категории", "no category").to_string();
        if let Some(id) = target {
            if let Some(c) = app.ha.categories.iter_mut().find(|c| c.id == id) {
                c.entities.push(entity.clone());
                cat_name = c.name.clone();
            }
        }
        app.save_ha_categories();
        app.toast(
            format!("{} → {}", util::truncate(&entity, 32), cat_name),
            true,
        );
    }

    if let Some((ci, entity, delta)) = move_entity {
        if let Some(c) = app.ha.categories.get_mut(ci) {
            if let Some(pos) = c.entities.iter().position(|x| x == &entity) {
                let next = (pos as i32 + delta).clamp(0, c.entities.len() as i32 - 1) as usize;
                if next != pos {
                    c.entities.swap(pos, next);
                    app.save_ha_categories();
                }
            }
        }
    }
}

fn ha_rooms_count(app: &App) -> usize {
    let mut n = 1; // whole home
    if app
        .status
        .as_ref()
        .map(|s| !s.ha.favorites.is_empty())
        .unwrap_or(false)
    {
        n += 1;
    }
    n += app.ha.categories.len();
    let mut areas: Vec<&str> = app
        .ha
        .entities
        .iter()
        .map(|e| e.area.as_str())
        .filter(|a| !a.is_empty())
        .collect();
    areas.sort_unstable();
    areas.dedup();
    n + areas.len()
}

// ---------------------------------------------------------------- settings

fn updates_card(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let info = app.update.info.clone();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t!(app.lang, "обновления", "updates"))
                    .size(16.0)
                    .strong()
                    .color(t.strong),
            );
            theme::chip(ui, &format!("v{}", VERSION), t.faint);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "проверить", "check"), t).clicked() {
                    app.check_updates();
                }
            });
        });
        let mut text = app.cfg.update_remote.clone();
        let response = ui.add(
            TextEdit::singleline(&mut text)
                .hint_text("https://github.com/owner/repo")
                .desired_width(f32::INFINITY),
        );
        if response.changed() {
            app.cfg.update_remote = text;
            // The old answer belongs to the old source.
            app.update.info = None;
            app.update.error.clear();
        }
        if response.lost_focus() {
            app.save();
            if sdw_core::update::Remote::parse(&app.cfg.update_remote).is_ok() {
                app.check_updates();
            }
        }
        match sdw_core::update::Remote::parse(&app.cfg.update_remote) {
            Ok(remote) => {
                ui.label(
                    RichText::new(remote.label())
                        .family(FontFamily::Monospace)
                        .size(10.5)
                        .color(t.faint),
                );
                if app.cfg.update_remote.trim().is_empty() {
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "пусто — встроенный источник",
                            "empty keeps the built-in source"
                        ))
                        .size(11.0)
                        .color(t.faint),
                    );
                }
            }
            Err(e) => {
                ui.label(
                    RichText::new(format!(
                        "{}: {e}",
                        t!(app.lang, "адрес не подходит", "the address does not fit")
                    ))
                    .size(11.5)
                    .color(t.bad),
                );
            }
        }
        if app.update.checking {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new(t!(app.lang, "проверяю релизы…", "checking releases…"))
                        .size(12.5)
                        .color(t.muted),
                );
            });
        }
        if !app.update.error.is_empty() {
            ui.label(RichText::new(&app.update.error).size(12.0).color(t.bad));
        }
        if let Some(info) = info {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if info.newer {
                    theme::chip(ui, &format!("{} → {}", info.current, info.latest), t.good);
                } else {
                    theme::chip(ui, t!(app.lang, "последняя версия", "up to date"), t.good);
                }
                ui.label(
                    RichText::new(info.published.chars().take(10).collect::<String>())
                        .family(FontFamily::Monospace)
                        .size(10.5)
                        .color(t.faint),
                );
            });
            if info.newer {
                if !info.notes.is_empty() {
                    ui.label(
                        RichText::new(util::truncate(&info.notes, 420))
                            .size(11.5)
                            .color(t.muted),
                    );
                }
                if app.update.downloading {
                    let done = app
                        .update
                        .progress
                        .load(std::sync::atomic::Ordering::Relaxed);
                    let total = app.update.total;
                    app.update
                        .view
                        .sample(done, total, Instant::now(), &mut app.update.sampled);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(t!(app.lang, "иду за обновлением", "downloading"))
                            .size(14.0)
                            .strong()
                            .color(t.ink),
                    );
                    let label = theme::progress_label(app.lang, &app.update.view);
                    theme::progress(ui, t, done, total, &label);
                    // The bar can only move while the page is being drawn.
                    ui.ctx().request_repaint_after(Duration::from_millis(180));
                } else {
                    ui.horizontal(|ui| {
                        if theme::primary(
                            ui,
                            t!(app.lang, "скачать обновление", "download update"),
                            t,
                        )
                        .clicked()
                        {
                            app.download_update();
                        }
                        if theme::ghost(ui, t!(app.lang, "страница релиза", "release page"), t)
                            .clicked()
                        {
                            let _ = util::open_url(&info.page);
                        }
                    });
                }
            }
            if !app.update.path.is_empty() {
                ui.add_space(6.0);
                theme::separator(ui, t);
                if app.update.installed {
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "обновление установлено — остался перезапуск",
                            "the update is installed — only a restart is left"
                        ))
                        .size(13.5)
                        .color(t.good),
                    );
                    ui.add_space(4.0);
                    if theme::primary(ui, t!(app.lang, "перезапустить", "restart"), t).clicked()
                    {
                        app.restart_app();
                    }
                } else {
                    ui.label(
                        RichText::new(t!(
                            app.lang,
                            "обновление скачано и готово к установке",
                            "the update is downloaded and ready to install"
                        ))
                        .size(13.5)
                        .color(t.good),
                    );
                    ui.label(
                        RichText::new(&app.update.path)
                            .family(FontFamily::Monospace)
                            .size(10.5)
                            .color(t.faint),
                    );
                    ui.horizontal(|ui| {
                        if theme::primary(ui, t!(app.lang, "установить", "install"), t).clicked()
                        {
                            let path = app.update.path.clone();
                            app.spawn(move || {
                                Task::Installed(sdw_core::update::install(std::path::Path::new(
                                    &path,
                                )))
                            });
                        }
                        if theme::ghost(ui, t!(app.lang, "открыть папку", "open folder"), t)
                            .clicked()
                        {
                            let _ = util::open_url(
                                &sdw_core::update::updates_dir().display().to_string(),
                            );
                        }
                    });
                }
            }
        }
    });
}

/// The programs the app starts from outside: the sound player, media keys,
/// typing and screenshots. A missing one is installed here with the
/// administrator password, because a Steam Deck has no terminal in hand.
fn components_card(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let list = app.components.clone();
    let missing = list.iter().filter(|c| !c.present).count();
    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(t!(app.lang, "компоненты деки", "deck components"))
                    .size(16.0)
                    .strong()
                    .color(t.strong),
            );
            theme::chip(
                ui,
                &format!("{}/{}", list.len() - missing, list.len()),
                if missing == 0 { t.good } else { t.warn },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(
                    ui,
                    if app.components_show {
                        t!(app.lang, "свернуть", "hide")
                    } else {
                        t!(app.lang, "подробнее", "details")
                    },
                    t,
                )
                .clicked()
                {
                    app.components_show = !app.components_show;
                }
                if theme::ghost(ui, t!(app.lang, "проверить", "check"), t).clicked() {
                    app.refresh_components();
                }
            });
        });
        ui.label(
            RichText::new(t!(
                app.lang,
                "программы, которые дека запускает снаружи: звук, медиа, ввод и снимки.",
                "programs the deck starts from outside: sound, media, typing and screenshots."
            ))
            .size(12.0)
            .color(t.muted),
        );
        if !app.components_show {
            if missing > 0 {
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "части не хватает — раскройте список, чтобы поставить.",
                        "some are missing — open the list to install them."
                    ))
                    .size(12.0)
                    .color(t.warn),
                );
            }
            return;
        }
        ui.add_space(4.0);
        for c in &list {
            ui.horizontal(|ui| {
                theme::dot(ui, if c.present { t.good } else { t.warn }, true);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(crate::components::title(app.lang, c.id))
                                .size(13.5)
                                .strong()
                                .color(t.strong),
                        );
                        if c.present && !c.found.is_empty() {
                            ui.label(
                                RichText::new(&c.found)
                                    .family(FontFamily::Monospace)
                                    .size(10.5)
                                    .color(t.faint),
                            );
                        }
                    });
                    ui.label(
                        RichText::new(crate::components::purpose(app.lang, c.id))
                            .size(11.5)
                            .color(t.muted),
                    );
                });
            });
        }
        if missing > 0 {
            ui.add_space(6.0);
            theme::separator(ui, t);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut app.components_pass)
                        .password(true)
                        .hint_text(t!(
                            app.lang,
                            "пароль администратора",
                            "administrator password"
                        ))
                        .desired_width(260.0),
                );
                if app.components_busy {
                    ui.spinner();
                    ui.label(
                        RichText::new(t!(app.lang, "ставлю…", "installing…"))
                            .size(12.0)
                            .color(t.muted),
                    );
                } else if theme::primary(
                    ui,
                    t!(
                        app.lang,
                        "установить недостающее",
                        "install the missing ones"
                    ),
                    t,
                )
                .clicked()
                {
                    app.install_components();
                }
            });
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "на SteamOS root откроется на время установки и закроется сам.",
                    "on SteamOS the root is opened for the install and closed again by itself."
                ))
                .size(11.0)
                .color(t.faint),
            );
        }
        if !app.components_message.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(util::truncate(&app.components_message, 420))
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.faint),
            );
        }
    });
}

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
            ui.label(
                RichText::new(t!(app.lang, "имя дека", "deck name"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add(TextEdit::singleline(&mut app.settings_name).desired_width(280.0));
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
        ui.label(theme::hud(
            t!(app.lang, "язык и тема", "language and theme"),
            t.faint,
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(t!(app.lang, "язык", "language"))
                    .size(13.0)
                    .color(t.muted),
            );
            for lang in Lang::ALL {
                if ui
                    .selectable_label(app.lang == lang, lang.label())
                    .clicked()
                {
                    app.set_lang(lang);
                }
            }
            ui.add_space(14.0);
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
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 104.0).max(240.0);
            if theme::slider_with_width(
                ui,
                t,
                width,
                t!(app.lang, "масштаб ui", "ui scale"),
                &mut app.cfg.ui_scale,
                0.75..=crate::app::max_ui_scale(),
                |v| format!("{:.0}%", v * 100.0),
            ) {
                app.save_soon();
            }
            if theme::ghost(ui, "1×", t).clicked() {
                app.cfg.ui_scale = 1.0;
                app.save();
            }
        });
        ui.add_space(2.0);
        if theme::slider(
            ui,
            t,
            t!(app.lang, "рисунок", "drawing"),
            &mut app.deck_view.cfg.scale,
            0.6..=1.4,
            |v| format!("{:.0}%", v * 100.0),
        ) {
            app.save_soon();
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            theme::switch(ui, &mut app.cfg.show_clock, t);
            ui.label(
                RichText::new(t!(app.lang, "часы в шапке", "clock in the header"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add_space(14.0);
            let mut gestures = app.cfg.global_gestures;
            theme::switch(ui, &mut gestures, t);
            if gestures != app.cfg.global_gestures {
                app.cfg.global_gestures = gestures;
                app.save();
            }
            ui.label(
                RichText::new(t!(app.lang, "свайпы страниц", "page swipes"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add_space(14.0);
            let mut grid = app.cfg.style.grid;
            if theme::switch(ui, &mut grid, t).changed() {
                app.cfg.style.grid = grid;
                app.apply_style(ui.ctx());
            }
            ui.label(
                RichText::new(t!(app.lang, "сетка", "grid"))
                    .size(13.0)
                    .color(t.muted),
            );
        });
        ui.add_space(6.0);
        if theme::slider(
            ui,
            t,
            t!(app.lang, "размер стрелок", "arrow size"),
            &mut app.cfg.style.arrow_size,
            28.0..=96.0,
            |v| format!("{v:.0} px"),
        ) {
            app.apply_style(ui.ctx());
            app.save_soon();
        }
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
        ui.label(theme::hud(
            t!(app.lang, "языковые пакеты", "language packs"),
            t.faint,
        ));
        let packs = sdw_core::i18n::pack_list();
        let active = sdw_core::i18n::active_pack();
        let dir = sdw_core::i18n::pack_dir();
        ui.horizontal_wrapped(|ui| {
            if theme::ghost(ui, t!(app.lang, "обновить", "reload"), t).clicked() {
                app.reload_lang_packs();
            }
            if theme::ghost(ui, t!(app.lang, "шаблон", "template"), t).clicked() {
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join("template.toml");
                match std::fs::write(&path, sdw_core::i18n::pack_template()) {
                    Ok(()) => app.toast(
                        format!(
                            "{} {}",
                            t!(app.lang, "шаблон записан:", "template written:"),
                            path.display()
                        ),
                        true,
                    ),
                    Err(e) => app.toast(
                        format!(
                            "{}: {e}",
                            t!(
                                app.lang,
                                "не удалось записать шаблон",
                                "could not write the template"
                            )
                        ),
                        false,
                    ),
                }
            }
            ui.label(
                RichText::new(dir.display().to_string())
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.faint),
            );
        });
        if packs.is_empty() {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "положите .toml в эту папку и нажмите «обновить» — шаблон уже содержит все строки",
                    "put a .toml into this folder and press \"reload\" — the template already has every line"
                ))
                .size(11.5)
                .color(t.muted),
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                for pack in &packs {
                    let selected = active.as_deref() == Some(pack.code.as_str());
                    let label = if pack.author.is_empty() {
                        pack.name.clone()
                    } else {
                        format!("{} · {}", pack.name, pack.author)
                    };
                    if ui
                        .selectable_label(selected, label)
                        .on_hover_text(format!(
                            "{} — {} {}",
                            pack.code,
                            pack.lines,
                            t!(app.lang, "строк", "lines")
                        ))
                        .clicked()
                    {
                        app.set_lang_pack(&pack.code);
                    }
                }
            });
            if active.is_some()
                && theme::ghost(ui, t!(app.lang, "выключить пакет", "turn the pack off"), t)
                    .clicked()
            {
                app.set_lang(Lang::En);
            }
        }
        for error in sdw_core::i18n::pack_errors() {
            ui.label(RichText::new(error).size(11.5).color(t.warn));
        }
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(
            t!(app.lang, "геймпад и вид", "gamepad and view"),
            t.faint,
        ));
        if theme::slider(
            ui,
            t,
            t!(app.lang, "масштаб рисунка", "drawing scale"),
            &mut app.deck_view.cfg.scale,
            0.6..=1.4,
            |v| format!("{:.0}%", v * 100.0),
        ) {
            app.save_soon();
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(14.0);
            theme::switch(ui, &mut app.deck_view.cfg.labels, t);
            ui.label(
                RichText::new(t!(app.lang, "подписи", "labels"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add_space(14.0);
            theme::switch(ui, &mut app.deck_view.cfg.glow, t);
            ui.label(
                RichText::new(t!(app.lang, "свечение", "glow"))
                    .size(13.0)
                    .color(t.muted),
            );
            ui.add_space(14.0);
            theme::switch(ui, &mut app.deck_view.cfg.sticks, t);
            ui.label(
                RichText::new(t!(app.lang, "стики", "sticks"))
                    .size(13.0)
                    .color(t.muted),
            );
        });
        ui.horizontal(|ui| {
            if theme::ghost(ui, t!(app.lang, "сбросить вид", "reset view"), t).clicked()
            {
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
        for e in &errors {
            ui.label(
                RichText::new(crate::input::error_text(app.lang, e))
                    .size(12.5)
                    .color(t.bad),
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
        ui.label(theme::hud(
            t!(app.lang, "уведомления", "notifications"),
            t.faint,
        ));
        ui.horizontal_wrapped(|ui| {
            let mut popup = app.cfg.notify_popup;
            theme::switch(ui, &mut popup, t);
            if popup != app.cfg.notify_popup {
                app.cfg.notify_popup = popup;
                if !popup {
                    app.popups.clear();
                }
                app.save();
            }
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "плавные всплывающие окна",
                    "smooth popup windows"
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
        ui.horizontal_wrapped(|ui| {
            let mut over = app.cfg.notify_fullscreen;
            theme::switch(ui, &mut over, t);
            if over != app.cfg.notify_fullscreen {
                app.cfg.notify_fullscreen = over;
                app.save();
            }
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "показывать и в полном экране",
                    "show them in fullscreen too"
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
        ui.label(
            RichText::new(t!(
                app.lang,
                "новое уведомление с пк появляется плавной карточкой поверх интерфейса; на компьютере оно остаётся системным.",
                "a new notification from the pc slides in as a card over the interface; on the computer it stays a system one."
            ))
            .size(11.5)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "звук с пк", "pc audio"), t.faint));
        let mut vol = app.cfg.audio.volume as f32;
        if theme::slider(
            ui,
            t,
            t!(app.lang, "громкость", "volume"),
            &mut vol,
            0.0..=100.0,
            |v| format!("{v:.0}%"),
        ) {
            app.cfg.audio.volume = vol.round().clamp(0.0, 100.0) as u8;
            app.save_soon();
        }
        crate::audio_ui::confirmations_ui(ui, app, t);
        ui.label(
            RichText::new(t!(
                app.lang,
                "сколько раз спросить «вы точно хотите перевести звук»",
                "how many times to ask “are you sure about moving the sound”"
            ))
            .size(11.5)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(
            t!(app.lang, "трансляция на пк", "share with the pc"),
            t.faint,
        ));
        ui.horizontal_wrapped(|ui| {
            let mut on = app.cfg.mirror_deck;
            theme::switch(ui, &mut on, t);
            if on != app.cfg.mirror_deck {
                app.cfg.mirror_deck = on;
                app.save();
            }
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "показывать экран деки на пк",
                    "show the deck screen on the pc"
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
        ui.label(
            RichText::new(t!(
                app.lang,
                "кадры идут только пока на пк открыта страница «стим дек» и включено наблюдение; в простое дека не передаёт ни картинки, ни звука.",
                "frames go only while the pc's “steam deck” page is open and watching; an idle deck sends neither a picture nor sound."
            ))
            .size(11.5)
            .color(t.faint),
        );
        ui.horizontal_wrapped(|ui| {
            let mut fps = app.cfg.mirror_fps;
            if theme::slider_u32(
                ui,
                t,
                t!(app.lang, "кадров в секунду", "frames per second"),
                &mut fps,
                1..=sdw_core::model::DECK_SHARE_FPS_MAX,
            ) {
                app.cfg.mirror_fps = fps.min(sdw_core::model::DECK_SHARE_FPS_MAX);
                app.save_soon();
            }
            ui.add_space(12.0);
            let mut quality = app.cfg.mirror_quality as u32;
            if theme::slider_u32(
                ui,
                t,
                t!(app.lang, "качество", "quality"),
                &mut quality,
                30..=95,
            ) {
                app.cfg.mirror_quality = quality.min(95) as u8;
                app.save_soon();
            }
        });
        ui.horizontal_wrapped(|ui| {
            let mut sound = app.cfg.mirror_sound;
            theme::switch(ui, &mut sound, t);
            if sound != app.cfg.mirror_sound {
                app.cfg.mirror_sound = sound;
                app.save();
            }
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "звук деки вместе с экраном",
                    "the deck's sound along with the picture"
                ))
                .size(13.0)
                .color(t.muted),
            );
        });
        ui.label(
            RichText::new(t!(
                app.lang,
                "звук играет на компьютере, пока открыта страница «стим дек»; запись идёт только тогда же.",
                "the sound plays on the computer while the “steam deck” page is open; recording runs only then too."
            ))
            .size(11.5)
            .color(t.faint),
        );
        if !app.share_audio_err.is_empty() {
            ui.label(
                RichText::new(util::truncate(&app.share_audio_err, 160))
                    .size(11.5)
                    .color(t.warn),
            );
        }
        if app.share.sent > 0 {
            ui.label(
                RichText::new(format!(
                    "{} {} · {}×{}",
                    app.share.sent,
                    t!(app.lang, "кадров отправлено", "frames sent"),
                    app.share.size.0,
                    app.share.size.1
                ))
                .family(FontFamily::Monospace)
                .size(10.5)
                .color(t.faint),
            );
        }
        if !app.share.error.is_empty() {
            // A pc that is off is a normal state, not a failure: amber, not red.
            ui.label(RichText::new(&app.share.error).size(11.5).color(t.warn));
        }
    });

    ui.add_space(8.0);
    updates_card(app, ui, t);

    ui.add_space(8.0);
    components_card(app, ui, t);

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

fn palette_grid(ui: &mut Ui, t: &Tokens, style: &mut ThemeCfg, prefix: &str, lang: Lang) -> bool {
    let p = |n: &str| {
        if prefix.is_empty() {
            n.to_string()
        } else {
            format!("{prefix}-{n}")
        }
    };
    let mut dirty = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
        for (key, ru, en) in [
            ("bg", "фон", "background"),
            ("bg2", "фон 2", "background 2"),
            ("card", "карточки", "cards"),
            ("well", "колодцы", "wells"),
            ("ink", "текст", "text"),
            ("strong", "яркий текст", "strong text"),
            ("muted", "вторичный", "secondary"),
            ("faint", "тихий", "faint"),
            ("line", "линии", "lines"),
            ("line-strong", "контуры", "outlines"),
            ("accent", "акцент", "accent"),
            ("accent-soft", "акцент мягкий", "accent soft"),
            ("good", "успех", "success"),
            ("warn", "внимание", "warning"),
            ("bad", "ошибка", "error"),
        ] {
            let name = p(key);
            let raw = theme::css_var(&style.css, &name)
                .or_else(|| theme::css_var(&style.css, &format!("light-{name}")));
            let mut c = raw.as_deref().and_then(theme::parse_color).unwrap_or(t.ink);
            let inner = ui.allocate_ui_with_layout(
                Vec2::new(104.0, 64.0),
                Layout::top_down(Align::Min),
                |ui| theme::color_chip(ui, t, &mut c, t!(lang, ru, en)),
            );
            if inner.inner {
                theme::css_set(&mut style.css, &name, &hex_of(c));
                dirty = true;
            }
            inner.response.on_hover_text(hex_of(c));
        }
    });
    dirty
}

pub fn addons(app: &mut App, ui: &mut Ui, t: &Tokens) {
    crate::addons_ui::ui(app, ui, t);
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
        let presets = theme::all_presets();
        let current_bg = theme::css_var(&style.css, "bg")
            .as_deref()
            .and_then(theme::parse_color);
        ui.horizontal_wrapped(|ui| {
            for (key, name) in &presets {
                let active = current_bg.is_some()
                    && current_bg
                        == theme::preset_css_owned(key)
                            .as_deref()
                            .and_then(|css| theme::css_var(css, "bg"))
                            .as_deref()
                            .and_then(theme::parse_color);
                let resp = ui.add(
                    egui::Button::new(RichText::new(name).size(13.5).color(if active {
                        t.strong
                    } else {
                        t.muted
                    }))
                    .fill(if active { t.accent_soft } else { t.well })
                    .stroke(Stroke::new(1.0, if active { t.accent } else { t.line }))
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
        });
        if theme::slider(
            ui,
            t,
            t!(app.lang, "масштаб ui", "ui scale"),
            &mut app.cfg.ui_scale,
            0.75..=crate::app::max_ui_scale(),
            |v| format!("{:.0}%", v * 100.0),
        ) {
            app.save_soon();
        }
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "палитра", "palette"), t.faint));
        ui.add_space(2.0);
        theme::small(ui, t!(app.lang, "тёмная", "dark"), t.faint);
        style_dirty |= palette_grid(ui, t, &mut style, "", app.lang);
        ui.add_space(8.0);
        theme::small(ui, t!(app.lang, "светлая", "light"), t.faint);
        style_dirty |= palette_grid(ui, t, &mut style, "light", app.lang);
    });

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(
            t!(app.lang, "сетка и плотность", "grid and density"),
            t.faint,
        ));
        ui.horizontal_wrapped(|ui| {
            for (value, ru, en) in [
                (&mut style.grid, "сетка", "grid"),
                (&mut style.compact, "компактно", "compact"),
                (&mut style.animations, "анимации", "animations"),
            ] {
                let before = *value;
                theme::switch(ui, value, t);
                if *value != before {
                    style_dirty = true;
                }
                ui.label(
                    RichText::new(t!(app.lang, ru, en))
                        .size(12.5)
                        .color(t.muted),
                );
                ui.add_space(12.0);
            }
        });
        ui.add_space(2.0);
        if theme::slider_step(
            ui,
            t,
            t!(app.lang, "шаг сетки", "grid step"),
            &mut style.grid_step,
            16.0..=64.0,
            1.0,
            |v| format!("{v:.0} px"),
        ) {
            theme::css_set(
                &mut style.css,
                "grid-step",
                &format!("{}", style.grid_step as u32),
            );
            style_dirty = true;
        }
        if theme::slider_step(
            ui,
            t,
            t!(app.lang, "яркость сетки", "grid alpha"),
            &mut style.grid_alpha,
            0.0..=40.0,
            1.0,
            |v| format!("{v:.0}"),
        ) {
            theme::css_set(
                &mut style.css,
                "grid-alpha",
                &format!("{}", style.grid_alpha.round() as u32),
            );
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "плотность", "density"),
            &mut style.density,
            0.65..=1.9,
            |v| format!("{v:.2}×"),
        ) {
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "шрифт", "font scale"),
            &mut style.font_scale,
            0.75..=1.8,
            |v| format!("{:.0}%", v * 100.0),
        ) {
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "высота плиток", "tile height"),
            &mut style.tile_height,
            120.0..=300.0,
            |v| format!("{v:.0}"),
        ) {
            style_dirty = true;
        }
        let mut cols = style.tile_columns;
        if theme::slider_u32(ui, t, t!(app.lang, "колонки", "columns"), &mut cols, 1..=4) {
            style.tile_columns = cols;
            style_dirty = true;
        }
        if crate::app::deck_mode() {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "на деке всегда три колонки",
                    "the deck always uses three columns"
                ))
                .size(11.5)
                .color(t.faint),
            );
        }
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
                ui.label(
                    RichText::new(t!(app.lang, ru, en))
                        .size(13.0)
                        .color(t.muted),
                );
                ui.add_space(14.0);
            }
            ui.add_space(8.0);
        });
        if theme::slider(
            ui,
            t,
            t!(app.lang, "размер стрелок", "arrow size"),
            &mut style.arrow_size,
            28.0..=96.0,
            |v| format!("{v:.0} px"),
        ) {
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "масштаб шапки", "top bar scale"),
            &mut style.nav_scale,
            0.8..=1.6,
            |v| format!("{:.0}%", v * 100.0),
        ) {
            style_dirty = true;
        }
        ui.add_space(6.0);
        ui.label(theme::hud(t!(app.lang, "навбар", "navbar"), t.faint));
        let mut pages: Vec<Page> = if style.pages.is_empty() {
            Page::ORDER.to_vec()
        } else {
            style.pages.iter().map(|k| Page::from_key(k)).collect()
        };
        let mut seen: Vec<Page> = Vec::new();
        pages.retain(|page| {
            if seen.contains(page) {
                false
            } else {
                seen.push(*page);
                true
            }
        });
        let original = pages.clone();
        for page in Page::ORDER {
            ui.horizontal(|ui| {
                let mut shown = pages.contains(&page);
                let was = shown;
                theme::switch(ui, &mut shown, t);
                if shown != was {
                    if shown {
                        pages.push(page);
                    } else {
                        pages.retain(|p| *p != page);
                    }
                }
                ui.label(theme::icon(page.icon(), 14.0, t.faint));
                ui.label(
                    RichText::new(page.label(app.lang))
                        .size(12.5)
                        .color(t.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let idx = pages.iter().position(|p| *p == page);
                    let can = idx.is_some();
                    if theme::icon_button(ui, theme::icon::ARROW_DOWN, t, 26.0, false)
                        .on_hover_text(t!(app.lang, "ниже", "move down"))
                        .clicked()
                    {
                        if let Some(i) = idx {
                            if i + 1 < pages.len() {
                                pages.swap(i, i + 1);
                            }
                        }
                    }
                    if theme::icon_button(ui, theme::icon::ARROW_UP, t, 26.0, false)
                        .on_hover_text(t!(app.lang, "выше", "move up"))
                        .clicked()
                    {
                        if let Some(i) = idx {
                            if i > 0 {
                                pages.swap(i, i - 1);
                            }
                        }
                    }
                    if !can {
                        // Keep the row the same height whether or not the page
                        // is in the dock.
                        ui.add_space(2.0);
                    }
                });
            });
        }
        if pages.is_empty() {
            pages.push(Page::Home);
        }
        if pages != original {
            style.pages = pages.iter().map(|p| p.key().to_string()).collect();
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "иконки навбара", "navbar icons"),
            &mut style.dock_icon,
            10.0..=30.0,
            |v| format!("{v:.0} px"),
        ) {
            style_dirty = true;
        }
        ui.horizontal(|ui| {
            let mut top = style.dock_top;
            theme::switch(ui, &mut top, t);
            if top != style.dock_top {
                style.dock_top = top;
                style_dirty = true;
            }
            ui.label(
                RichText::new(t!(app.lang, "навбар сверху", "navbar on top"))
                    .size(12.5)
                    .color(t.muted),
            );
        });
        ui.add_space(4.0);
        ui.label(theme::hud(
            t!(app.lang, "плитки на главной", "home tiles"),
            t.faint,
        ));
        let tiles = [
            ("gamepad", "геймпад", "gamepad"),
            ("screen", "экран", "screen"),
            ("notifications", "уведомления", "notifications"),
            ("timers", "таймеры", "timers"),
            ("ha", "home assistant", "home assistant"),
            ("binds", "бинды", "binds"),
            ("control", "пульт", "control"),
            ("bluetooth", "bluetooth", "bluetooth"),
            ("computers", "компьютеры", "computers"),
        ];
        ui.horizontal_wrapped(|ui| {
            for (key, ru, en) in tiles {
                let mut shown =
                    style.home_tiles.is_empty() || style.home_tiles.iter().any(|k| k == key);
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
                ui.label(
                    RichText::new(t!(app.lang, ru, en))
                        .size(12.5)
                        .color(t.muted),
                );
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

    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(
            t!(app.lang, "свои паки оформления", "custom theme packs"),
            t.faint,
        ));
        ui.label(
            RichText::new(format!(
                "{} {}",
                t!(app.lang, "папка:", "folder:"),
                theme::custom_dir().display()
            ))
            .family(FontFamily::Monospace)
            .size(11.0)
            .color(t.faint),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut app.theme_pack_name)
                    .hint_text(t!(app.lang, "имя пака", "pack name"))
                    .desired_width(220.0),
            );
            if theme::ghost(
                ui,
                t!(app.lang, "сохранить текущий css", "save current css"),
                t,
            )
            .clicked()
            {
                match theme::save_custom_pack(&app.theme_pack_name, &style.css) {
                    Ok(path) => {
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("my-theme")
                            .to_string();
                        let key = format!("custom:{stem}");
                        theme::apply_preset(&mut style, &key);
                        app.style_draft = style.css.clone();
                        app.theme_pack_name.clear();
                        style_dirty = true;
                        app.toast(
                            format!(
                                "{} {}",
                                t!(app.lang, "пак сохранён:", "pack saved:"),
                                path.display()
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(format!("{}: {e}", t!(app.lang, "пак", "pack")), false),
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut app.theme_import)
                    .hint_text(t!(app.lang, "путь к .css файлу", "path to a .css file"))
                    .desired_width(360.0),
            );
            if theme::ghost(ui, t!(app.lang, "импортировать", "import"), t).clicked() {
                let raw = app.theme_import.trim().to_string();
                let expanded = if let Some(rest) = raw.strip_prefix("~/") {
                    dirs::home_dir()
                        .map(|h| h.join(rest))
                        .unwrap_or_else(|| std::path::PathBuf::from(&raw))
                } else {
                    std::path::PathBuf::from(&raw)
                };
                match std::fs::read_to_string(&expanded) {
                    Ok(css) => {
                        let name = app
                            .theme_pack_name
                            .trim()
                            .to_string()
                            .is_empty()
                            .then(|| {
                                expanded
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("imported")
                                    .to_string()
                            })
                            .unwrap_or_else(|| app.theme_pack_name.clone());
                        match theme::save_custom_pack(&name, &css) {
                            Ok(path) => {
                                let stem = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("imported")
                                    .to_string();
                                theme::apply_preset(&mut style, &format!("custom:{stem}"));
                                app.style_draft = style.css.clone();
                                app.theme_import.clear();
                                style_dirty = true;
                                app.toast(
                                    format!(
                                        "{} {}",
                                        t!(app.lang, "пак добавлен:", "pack added:"),
                                        path.display()
                                    ),
                                    true,
                                );
                            }
                            Err(e) => {
                                app.toast(format!("{}: {e}", t!(app.lang, "пак", "pack")), false)
                            }
                        }
                    }
                    Err(e) => app.toast(format!("{}: {e}", t!(app.lang, "файл", "file")), false),
                }
            }
        });
        let custom = theme::custom_presets();
        if custom.is_empty() {
            theme::small(
                ui,
                t!(
                    app.lang,
                    "пока нет своих паков. сохраните текущий css или импортируйте файл — он появится в списке пресетов.",
                    "no custom packs yet. save the current css or import a file and it appears in the presets list."
                ),
                t.muted,
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                for (key, _) in &custom {
                    let name = key.strip_prefix("custom:").unwrap_or(key);
                    if theme::ghost(ui, name, t).clicked() {
                        theme::apply_preset(&mut style, key);
                        app.style_draft = style.css.clone();
                        style_dirty = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(theme::icon(theme::icon::CLOSE, 14.0, t.faint))
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::NONE)
                                .corner_radius(0.0)
                                .min_size(Vec2::new(24.0, 24.0)),
                        )
                        .on_hover_text(t!(app.lang, "удалить пак", "delete pack"))
                        .clicked()
                    {
                        let _ =
                            std::fs::remove_file(theme::custom_dir().join(format!("{name}.css")));
                    }
                }
            });
        }
    });

    // ---------------------------------------------------------------- sound
    let mut snd = app.cfg.sound.clone();
    let mut snd_dirty = false;
    ui.add_space(8.0);
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "звуки", "sounds"), t.faint));
        // Everything wraps: the Deck has one width and the interface scale can
        // grow, so no single row is allowed to run past the card.
        ui.horizontal_wrapped(|ui| {
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
            ui.label(
                RichText::new(t!(app.lang, "набор", "pack"))
                    .size(13.0)
                    .color(t.muted),
            );
            for (key, label) in [
                ("grid", "Grid"),
                ("pop", "Pop"),
                ("soft", "Soft"),
                ("chip", "Chip"),
            ] {
                if ui.selectable_label(snd.pack == key, label).clicked() && snd.pack != key {
                    snd.pack = key.to_string();
                    snd_dirty = true;
                    crate::sound::play(&snd, crate::sound::Cue::Click);
                }
            }
        });
        // The slider needs the whole row: it draws its own label and value.
        let mut vol = snd.volume as f32;
        if theme::slider(
            ui,
            t,
            t!(app.lang, "громкость", "volume"),
            &mut vol,
            0.0..=100.0,
            |v| format!("{v:.0}%"),
        ) {
            snd.volume = vol.round().clamp(0.0, 100.0) as u8;
            snd_dirty = true;
        }
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
                            // A fixed label column keeps every field aligned,
                            // and the path field takes whatever is left.
                            ui.add_sized(
                                [116.0, 24.0],
                                egui::Label::new(
                                    RichText::new(t!(app.lang, ru, en))
                                        .size(12.5)
                                        .color(t.muted),
                                )
                                .truncate(),
                            );
                            ui.add_space(4.0);
                            let room = (ui.available_width() - 52.0).max(80.0);
                            changed = ui
                                .add(
                                    TextEdit::singleline(buf)
                                        .hint_text("/home/deck/sounds/click.wav")
                                        .desired_width(room),
                                )
                                .changed();
                            test = ui
                                .add(
                                    egui::Button::new(RichText::new("▶").size(14.0).color(t.ink))
                                        .fill(t.card)
                                        .stroke(Stroke::new(1.0, t.line_strong))
                                        .corner_radius(0.0)
                                        .min_size(egui::vec2(40.0, 28.0)),
                                )
                                .clicked();
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

    crate::viz_stage::controls(app, ui, t);

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

#[cfg(test)]
mod light_tests {
    use super::*;

    fn notification(app: &str, summary: &str, body: &str) -> Notification {
        Notification {
            app: app.into(),
            summary: summary.into(),
            body: body.into(),
            ..Notification::default()
        }
    }

    #[test]
    fn notification_line_flattens_and_bounds_external_text() {
        assert_eq!(
            notification_line("  first\n\n second\tthird  ", 20),
            "first second third"
        );
        assert_eq!(notification_line("a very long path", 8), "a very …");
    }

    #[test]
    fn screenshot_notifications_collapse_only_consecutive_matching_rows() {
        let rows = compact_notification_rows(vec![
            notification("niri", "Screenshot saved", "/tmp/a.png"),
            notification("niri", "Screenshot saved", "/tmp/b.png"),
            notification("discord", "message", "same shape is not a screenshot"),
            notification("niri", "Screenshot saved", "/tmp/c.png"),
        ]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].repeats, 2);
        assert_eq!(rows[1].repeats, 1);
        assert_eq!(rows[2].repeats, 1);
    }

    #[test]
    fn ordinary_notifications_are_never_collapsed() {
        let rows = compact_notification_rows(vec![
            notification("mail", "New message", "hello"),
            notification("mail", "New message", "hello"),
        ]);
        assert_eq!(rows.len(), 2);
    }

    fn lamp(modes: &[&str], extra: serde_json::Value) -> HaEntity {
        let mut attrs = serde_json::json!({ "supported_color_modes": modes });
        if let (Some(map), Some(add)) = (attrs.as_object_mut(), extra.as_object()) {
            for (k, v) in add {
                map.insert(k.clone(), v.clone());
            }
        }
        HaEntity {
            entity_id: "light.lamp".into(),
            state: "on".into(),
            attributes: attrs,
            last_changed: String::new(),
            last_updated: String::new(),
            area: String::new(),
        }
    }

    fn preset(en: &str) -> &'static LightPreset {
        LIGHT_PRESETS
            .iter()
            .find(|p| p.en == en)
            .expect("preset exists")
    }

    #[test]
    fn a_warm_preset_is_sent_as_a_temperature() {
        let e = lamp(&["color_temp", "hs"], json!({}));
        let call = light_preset_call(&e, preset("very warm"));
        assert_eq!(call, json!({ "color_temp_kelvin": 2200 }));
    }

    #[test]
    fn a_temperature_is_clamped_to_the_lamp() {
        let e = lamp(&["color_temp"], json!({ "max_color_temp_kelvin": 6000 }));
        let call = light_preset_call(&e, preset("cool"));
        assert_eq!(call, json!({ "color_temp_kelvin": 6000 }));
    }

    #[test]
    fn an_rgb_only_lamp_gets_the_shade() {
        let e = lamp(&["hs"], json!({ "color_temp_kelvin": 2292 }));
        let call = light_preset_call(&e, preset("very warm"));
        assert_eq!(call, json!({ "rgb_color": [255, 146, 39] }));
    }

    #[test]
    fn a_saturated_preset_is_always_rgb() {
        let e = lamp(&["color_temp", "hs"], json!({}));
        let call = light_preset_call(&e, preset("blue"));
        assert_eq!(call, json!({ "rgb_color": [70, 120, 255] }));
    }

    #[test]
    fn the_tick_follows_a_slightly_off_temperature() {
        // 2292 K is what the bulb really reports for "very warm".
        let e = lamp(&["color_temp", "hs"], json!({ "color_temp_kelvin": 2292 }));
        assert!(preset_shown(preset("very warm"), &e, None));
        assert!(!preset_shown(preset("warm"), &e, None));
        assert!(!preset_shown(preset("neutral"), &e, None));
    }

    #[test]
    fn the_tick_uses_the_shade_when_the_temperature_is_gone() {
        // After a tap the optimistic colour hides the old kelvin reading, so
        // only the RGB is left: the deviation a bulb reports must still tick.
        let e = lamp(&["color_temp", "hs"], json!({}));
        assert!(preset_shown(preset("very warm"), &e, Some([255, 150, 49])));
        assert!(preset_shown(preset("blue"), &e, Some([68, 122, 250])));
        assert!(!preset_shown(preset("warm"), &e, Some([255, 150, 49])));
    }
}
