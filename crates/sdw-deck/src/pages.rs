//! every page of the deck app.
//!
//! the home page is a grid-style grid of big touch tiles; the gamepad page is the
//! drawn steam deck; the rest are focused panels that stay readable at 1280×800.

use crate::app::{notification_color, App, Page, Task};
use crate::binds::BindEditor;
use crate::deck_view::{self, DeckInput, DeckViewState};
use crate::theme::{self, Tile, Tokens};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, Layout, Margin, RichText, Sense,
    Stroke, TextEdit, Ui, Vec2,
};
use sdw_core::model::*;
use sdw_core::util;
use sdw_core::{t, Lang};
use serde_json::json;
use std::time::Duration;

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
                ui.label(RichText::new(e).size(12.0).color(t.bad));
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

    let online = app.status.is_some();
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
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(lang, "питание", "power"), t.faint));
            if !online {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "нет связи с компьютером",
                            "the computer is offline"
                        ))
                        .size(12.0)
                        .color(t.warn),
                    );
                });
            }
        });
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
                let width = ((ui.available_width() - 8.0 * (count - 1.0)) / count).max(104.0);
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
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(lang, "приложения", "applications"), t.faint));
            if app.control_apps_pending {
                ui.spinner();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(lang, "обновить", "refresh"), t).clicked() {
                    refresh = true;
                }
            });
        });
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
                                        ui.label(theme::icon(glyph, 18.0, t.accent));
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
                        ui.label(RichText::new(bind.action.label()).size(12.5).color(t.muted));
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
            let r = ui
                .add(egui::Slider::new(&mut app.deck_view.cfg.scale, 0.6..=1.4).show_value(false));
            if r.drag_stopped() || r.lost_focus() {
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

fn backend_list() -> [MonitorBackend; 7] {
    [
        MonitorBackend::Auto,
        MonitorBackend::Hyprland,
        MonitorBackend::Vkms,
        MonitorBackend::Krfb,
        MonitorBackend::Mirror,
        MonitorBackend::X11,
        MonitorBackend::Gdigrab,
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

    theme::small(
        ui,
        t!(
            app.lang,
            "по умолчанию: расширить, авто-бэкенд и 1280×800 — лучший вариант для деки. выход создаётся одной кнопкой.",
            "defaults: extend, auto backend and 1280×800 — the best fit for the deck. one press creates the output."
        ),
        t.faint,
    );
    ui.add_space(8.0);

    let area = ui.available_rect_before_wrap();
    let stream_h = (area.width() / 1.6).clamp(240.0, 520.0);
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
        ui.horizontal(|ui| {
            theme::switch(ui, &mut app.stream.input, t);
            ui.label(
                RichText::new(t!(app.lang, "мышь и клавиатура", "mouse and keyboard"))
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

/// A small Grid tile with the notification picture, or a domain icon when the
/// daemon had no image to send.
fn notification_thumb(
    ui: &mut Ui,
    t: &Tokens,
    n: &Notification,
    art: Option<&egui::TextureHandle>,
    color: Color32,
) {
    let side = 48.0;
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

    ui.horizontal(|ui| {
        if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
            app.last_notif = std::time::Instant::now() - std::time::Duration::from_secs(5);
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
                RichText::new(t!(app.lang, "пока тихо", "all quiet"))
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
        let art = app.notif_texture(ui.ctx(), n.id, &n.image);
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    notification_thumb(ui, t, &n, art.as_ref(), color);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(if n.app.is_empty() { "system" } else { &n.app })
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
                                egui::Label::new(RichText::new(&n.body).size(13.0).color(t.muted))
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
    let connected = app.status.is_some();
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
                let mut pos = app.media.seeking.unwrap_or(st.position).clamp(0.0, length);
                ui.spacing_mut().interact_size.y = 30.0;
                let r = ui.add_sized(
                    [ui.available_width(), 30.0],
                    egui::Slider::new(&mut pos, 0.0..=length)
                        .show_value(false)
                        .trailing_fill(true),
                );
                if r.dragged() {
                    app.media.seeking = Some(pos);
                }
                if r.drag_stopped() {
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
        ui.horizontal(|ui| {
            let mut vol = st.volume.min(100) as i32;
            ui.spacing_mut().interact_size.y = 34.0;
            let r = ui.add_sized(
                [(ui.available_width() - 150.0).max(120.0), 34.0],
                egui::Slider::new(&mut vol, 0..=100)
                    .show_value(false)
                    .trailing_fill(true),
            );
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                app.media_command("volume", vol as f64);
            }
            ui.label(
                RichText::new(format!("{vol}%"))
                    .family(FontFamily::Monospace)
                    .size(14.0)
                    .strong()
                    .color(t.strong),
            );
            if media_button(ui, t, theme::icon::VOLUME_DOWN, 38.0, false) {
                app.media_command("volume_down", 0.0);
            }
            if media_button(ui, t, theme::icon::VOLUME_UP, 38.0, false) {
                app.media_command("volume_up", 0.0);
            }
        });
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

/// Seven quick colours for RGB lamps: warm white, neutral, cool, then the
/// usual saturated ones. `light.turn_on` takes the array as-is.
const LIGHT_COLORS: &[(&str, [u8; 3])] = &[
    ("тёплый", [255, 180, 110]),
    ("нейтральный", [255, 235, 210]),
    ("холодный", [205, 225, 255]),
    ("красный", [255, 70, 60]),
    ("зелёный", [80, 220, 120]),
    ("синий", [70, 120, 255]),
    ("фиолетовый", [170, 90, 255]),
];

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
    ui.add_sized(
        [width.max(120.0), 46.0],
        egui::Button::new(RichText::new(label).size(15.5).strong().color(if on {
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
                            let reported = e.brightness_pct().unwrap_or(if on { 100 } else { 0 });
                            let mut pct = i32::from(reported.max(if on { 1 } else { 0 }));
                            let slider_w = (width - 96.0).max(90.0);
                            ui.spacing_mut().interact_size.y = 34.0;
                            let r = ui.add_sized(
                                [slider_w, 34.0],
                                egui::Slider::new(&mut pct, 1..=100)
                                    .step_by(1.0)
                                    .show_value(false)
                                    .trailing_fill(true),
                            );
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                                let pct = pct.clamp(1, 100) as u8;
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
                            ui.label(
                                RichText::new(format!(
                                    "{}%",
                                    if on || reported > 0 { pct } else { 0 }
                                ))
                                .family(FontFamily::Monospace)
                                .size(12.0)
                                .color(t.faint),
                            );
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
                            let r = ui.add_sized(
                                [(width - 100.0).max(90.0), 30.0],
                                egui::Slider::new(&mut k, min as i32..=max as i32)
                                    .step_by(step as f64)
                                    .show_value(false)
                                    .trailing_fill(true),
                            );
                            if r.drag_stopped() || (r.changed() && !r.dragged()) {
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
                        ui.horizontal_wrapped(|ui| {
                            ui.label(theme::icon(theme::icon::LOOK, 17.0, t.faint));
                            for (name, rgb) in LIGHT_COLORS {
                                let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                let selected = e.rgb_color() == Some(*rgb);
                                let resp = ui.add(
                                    egui::Button::new("")
                                        .fill(c)
                                        .stroke(Stroke::new(
                                            if selected { 2.0 } else { 1.0 },
                                            if selected { t.strong } else { t.line },
                                        ))
                                        .corner_radius(CornerRadius::ZERO)
                                        .min_size(Vec2::splat(26.0)),
                                );
                                if resp.on_hover_text(*name).clicked() {
                                    actions.push(HaAction {
                                        call: ha_service(e, "turn_on", json!({ "rgb_color": rgb })),
                                        entity: e.entity_id.clone(),
                                        state: Some("on".into()),
                                    });
                                }
                            }
                        });
                        // Any colour, not only the eight presets. The draft
                        // lives in egui memory per entity, so opening the
                        // picker and choosing a shade survives frames; nothing
                        // is sent until "применить" is pressed.
                        let id = ui.make_persistent_id(("ha-light-colour", e.entity_id.as_str()));
                        let fallback = e.rgb_color().unwrap_or([255, 196, 120]);
                        let mut draft =
                            ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || fallback));
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(t!(lang, "свой цвет", "custom colour"))
                                    .size(12.0)
                                    .color(t.muted),
                            );
                            egui::color_picker::color_edit_button_srgb(ui, &mut draft);
                            ui.data_mut(|d| d.insert_temp(id, draft));
                            ui.label(
                                RichText::new(format!(
                                    "#{:02X}{:02X}{:02X}",
                                    draft[0], draft[1], draft[2]
                                ))
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
                        });
                    }
                }
                "cover" => {
                    let mut pos = e
                        .cover_position()
                        .unwrap_or(if e.is_open() { 100 } else { 0 })
                        as i32;
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
                        ui.spacing_mut().interact_size.y = 30.0;
                        let r = ui.add_sized(
                            [(width - 220.0).max(90.0), 30.0],
                            egui::Slider::new(&mut pos, 0..=100)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if r.drag_stopped() {
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
                            let r = ui.add(
                                egui::Slider::new(&mut pct, 0..=100)
                                    .show_value(false)
                                    .trailing_fill(true),
                            );
                            if r.drag_stopped() {
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
                }
                "scene" | "script" | "button" => {
                    if ui
                        .add_sized(
                            [width, 44.0],
                            egui::Button::new(
                                RichText::new(t!(lang, "запустить", "run"))
                                    .size(14.5)
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
                        let r = ui.add_sized(
                            [(width - 130.0).max(90.0), 34.0],
                            egui::Slider::new(&mut pct, 0..=100)
                                .show_value(false)
                                .trailing_fill(true),
                        );
                        if r.drag_stopped() {
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
                        if ha_power_button(ui, t, on, lang, width) {
                            actions.push(HaAction {
                                call: ha_toggle(e, !on),
                                entity: e.entity_id.clone(),
                                state: Some(if on { "off" } else { "on" }.into()),
                            });
                        }
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

pub fn home_assistant(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let ha = app
        .status
        .as_ref()
        .map(|s| s.ha.clone())
        .unwrap_or_default();
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
            if ha.configured {
                ui.label(
                    RichText::new(format!(
                        "{} · {} {} · {} {}",
                        util::truncate(&ha.url, 40),
                        ha.entities,
                        t!(app.lang, "сущностей", "entities"),
                        ha_rooms_count(app),
                        t!(app.lang, "комнат", "rooms")
                    ))
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.muted),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ha.configured
                    && theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked()
                {
                    app.ha.last_fetch =
                        std::time::Instant::now() - std::time::Duration::from_secs(60);
                }
            });
        });
        if ha.configured && !ha.error.is_empty() && !ha.connected {
            ui.label(
                RichText::new(util::truncate(&ha.error, 90))
                    .size(11.5)
                    .color(t.bad),
            );
        }
    });

    if !ha.configured {
        ui.add_space(8.0);
        theme::card(ui, t, |ui| {
            ui.label(
                RichText::new(t!(
                    app.lang,
                    "подключить home assistant",
                    "connect home assistant"
                ))
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
                app.last_status = std::time::Instant::now() - std::time::Duration::from_secs(5);
            }
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
        } else {
            ui.label(
                RichText::new(format!(
                    "{}: {}",
                    t!(app.lang, "поиск", "search"),
                    util::truncate(&app.ha.search, 40)
                ))
                .size(14.0)
                .color(t.muted),
            );
            if theme::ghost(ui, t!(app.lang, "сбросить", "clear"), t).clicked() {
                app.ha.search.clear();
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                TextEdit::singleline(&mut app.ha.search)
                    .hint_text(t!(app.lang, "поиск по имени", "search by name"))
                    .desired_width(220.0),
            );
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
    // Two columns only where a card is still wide enough to be usable. On the
    // Deck everything stays in one full-width column so nothing can spill out.
    let cols = if width >= 1180.0 { 2 } else { 1 };

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
                            app.lang,
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
                        );
                    });
                }
            });
        }
    }

    for (id, pct) in bright {
        app.ha
            .optimistic_pct
            .insert(id, (pct, std::time::Instant::now()));
    }
    let acted = !actions.is_empty();
    for a in actions {
        if let Some(state) = a.state {
            app.ha
                .optimistic
                .insert(a.entity, (state, std::time::Instant::now()));
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
        ui.label(
            RichText::new("git.entitybtw.ru · entitybtw/sd-widgets")
                .family(FontFamily::Monospace)
                .size(10.5)
                .color(t.faint),
        );
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
                ui.horizontal(|ui| {
                    if app.update.downloading {
                        let done = app
                            .update
                            .progress
                            .load(std::sync::atomic::Ordering::Relaxed);
                        let total = app.update.total.max(1);
                        ui.add(
                            egui::ProgressBar::new((done as f32 / total as f32).clamp(0.0, 1.0))
                                .desired_width(240.0)
                                .text(format!("{}%", done * 100 / total)),
                        );
                    } else if theme::primary(
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
            if !app.update.path.is_empty() {
                ui.label(
                    RichText::new(&app.update.path)
                        .family(FontFamily::Monospace)
                        .size(10.5)
                        .color(t.muted),
                );
                ui.horizontal(|ui| {
                    if theme::ghost(ui, t!(app.lang, "установить", "install"), t).clicked()
                    {
                        let path = app.update.path.clone();
                        app.spawn(move || {
                            Task::Installed(sdw_core::update::install(std::path::Path::new(&path)))
                        });
                    }
                    if theme::ghost(ui, t!(app.lang, "открыть папку", "open folder"), t).clicked()
                    {
                        let _ =
                            util::open_url(&sdw_core::update::updates_dir().display().to_string());
                    }
                });
            }
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
            for lang in [Lang::Ru, Lang::En] {
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
        ui.horizontal_wrapped(|ui| {
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
            ui.horizontal(|ui| {
                if theme::ghost(ui, "1×", t).clicked() {
                    app.cfg.ui_scale = 1.0;
                    app.save();
                }
            });
            ui.add_space(4.0);
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
        });
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
        ui.horizontal_wrapped(|ui| {
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
                "сколько раз спросить «вы точно хотите перевести звук»",
                "how many times to ask “are you sure about moving the sound”"
            ))
            .size(11.5)
            .color(t.faint),
        );
    });

    ui.add_space(8.0);
    updates_card(app, ui, t);

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
        if theme::slider(
            ui,
            t,
            t!(app.lang, "шаг сетки", "grid step"),
            &mut style.grid_step,
            16.0..=64.0,
            |v| format!("{v:.0} px"),
        ) {
            theme::css_set(
                &mut style.css,
                "grid-step",
                &format!("{}", style.grid_step as u32),
            );
            style_dirty = true;
        }
        if theme::slider(
            ui,
            t,
            t!(app.lang, "яркость сетки", "grid alpha"),
            &mut style.grid_alpha,
            0.0..=40.0,
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
            ("ha", "home assistant", "home assistant"),
            ("binds", "бинды", "binds"),
            ("control", "пульт", "control"),
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
