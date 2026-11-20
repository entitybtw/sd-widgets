//! the addons page: import, enable, configure and remove packages.
//!
//! A package can add a theme, a clock preset, home tiles, sounds and daemon
//! actions. Everything is declarative JSON, so a user can ship an addon as a
//! folder, a single manifest or a zip without rebuilding either binary.

use crate::app::{App, Task};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, Align, Color32, FontFamily, Layout, RichText, Stroke, TextEdit, Ui, Vec2,
};
use sdw_core::addons::{self, Addon, AddonField};
use sdw_core::model::Action;
use sdw_core::{t, util};
use serde_json::Value;

pub fn ui(app: &mut App, ui: &mut Ui, t: &Tokens) {
    ui.label(
        RichText::new(t!(app.lang, "аддоны", "addons"))
            .size(28.0)
            .strong()
            .color(t.strong),
    );
    ui.add(
        egui::Label::new(
            RichText::new(t!(
                app.lang,
                "пакеты расширяют деку и демон: темы, часы, плитки, звуки и действия на компьютере.",
                "packages extend the deck and the daemon: themes, clocks, tiles, sounds and actions on the computer."
            ))
            .size(13.5)
            .color(t.muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);

    import_card(app, ui, t);
    ui.add_space(8.0);
    store_card(app, ui, t);
    ui.add_space(8.0);

    let installed = addons::list();
    if installed.is_empty() {
        theme::card(ui, t, |ui| {
            theme::small(
                ui,
                t!(
                    app.lang,
                    "пока нет аддонов. импортируйте папку, addon.json, .css или .sdwaddon.",
                    "no addons yet. import a folder, addon.json, .css or .sdwaddon."
                ),
                t.muted,
            );
            let path = addons::root();
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    t!(app.lang, "папка:", "folder:"),
                    path.display()
                ))
                .family(FontFamily::Monospace)
                .size(11.0)
                .color(t.faint),
            );
        });
        return;
    }

    for addon in installed {
        addon_card(app, ui, t, &addon);
        ui.add_space(8.0);
    }
}

fn import_card(app: &mut App, ui: &mut Ui, t: &Tokens) {
    theme::card(ui, t, |ui| {
        ui.label(theme::hud(t!(app.lang, "импорт", "import"), t.faint));
        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut app.addon_import)
                    .hint_text(t!(
                        app.lang,
                        "путь к папке, addon.json, .css или .sdwaddon",
                        "a folder, addon.json, .css or .sdwaddon"
                    ))
                    .desired_width(420.0),
            );
            if theme::primary(ui, t!(app.lang, "импортировать", "import"), t).clicked()
            {
                let raw = expand(&app.addon_import);
                match addons::install(&raw) {
                    Ok(addon) => {
                        app.cfg.addons.set(addon.id(), true);
                        defaults(app, &addon);
                        app.save();
                        app.addon_import.clear();
                        app.toast(
                            format!(
                                "{} {}",
                                t!(app.lang, "аддон установлен:", "addon installed:"),
                                addon.manifest.name
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(format!("{}: {e}", t!(app.lang, "аддон", "addon")), false),
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if theme::ghost(ui, t!(app.lang, "пример", "example"), t).clicked() {
                let dir = addons::root().join("example");
                let _ = std::fs::create_dir_all(&dir);
                let text =
                    serde_json::to_string_pretty(&addons::example_manifest()).unwrap_or_default();
                let path = dir.join(addons::MANIFEST);
                match std::fs::write(&path, text) {
                    Ok(()) => app.toast(
                        format!("{} {}", t!(app.lang, "пример:", "example:"), path.display()),
                        true,
                    ),
                    Err(e) => app.toast(e.to_string(), false),
                }
            }
            if theme::ghost(ui, t!(app.lang, "сохранить вид", "save theme"), t).clicked()
            {
                match addons::create_theme(
                    &format!("{}-{}", app.cfg.device_name, now_tag()),
                    &app.cfg.style.css,
                ) {
                    Ok(addon) => {
                        app.cfg.addons.set(addon.id(), true);
                        app.save();
                        app.toast(
                            t!(
                                app.lang,
                                "тема сохранена как аддон",
                                "theme saved as an addon"
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(e, false),
                }
            }
            if theme::ghost(ui, t!(app.lang, "сохранить часы", "save clock"), t).clicked()
            {
                match addons::create_clock(&format!("clock-{}", now_tag()), &app.cfg.clock) {
                    Ok(addon) => {
                        app.cfg.addons.set(addon.id(), true);
                        app.save();
                        app.toast(
                            t!(
                                app.lang,
                                "часы сохранены как аддон",
                                "clock saved as an addon"
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(e, false),
                }
            }
        });
        ui.label(
            RichText::new(format!(
                "{} {}",
                t!(app.lang, "папка:", "folder:"),
                addons::root().display()
            ))
            .family(FontFamily::Monospace)
            .size(10.5)
            .color(t.faint),
        );
    });
}

/// The store: repositories the user added, and the packages they list.
///
/// A base URL is added here; the catalogue is fetched in the background and a
/// package installs with one press on the deck and on the computer behind it.
fn store_card(app: &mut App, ui: &mut Ui, t: &Tokens) {
    // Refresh at most every ten minutes, and as soon as the list is empty.
    let stale = app
        .addon_catalog_at
        .map(|at| at.elapsed() > std::time::Duration::from_secs(600))
        .unwrap_or(true);
    if stale && !app.cfg.addons.repos.is_empty() {
        app.load_addon_catalog();
    }

    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::hud(t!(app.lang, "магазин", "store"), t.faint));
            if app.addon_catalog_pending {
                ui.spinner();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::ghost(ui, t!(app.lang, "обновить", "refresh"), t).clicked() {
                    app.addon_catalog_at = None;
                    app.load_addon_catalog();
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut app.addon_repo_input)
                    .hint_text(t!(
                        app.lang,
                        "ссылка на магазин или на файл .sdwaddon",
                        "a store URL or a .sdwaddon file"
                    ))
                    .desired_width(430.0),
            );
            let typed = app.addon_repo_input.trim().to_string();
            let is_file = typed.ends_with(".sdwaddon")
                || typed.ends_with(".zip")
                || typed.ends_with(".json")
                || typed.ends_with(".css");
            if theme::primary(
                ui,
                if is_file {
                    t!(app.lang, "установить файл", "install file")
                } else {
                    t!(app.lang, "добавить магазин", "add store")
                },
                t,
            )
            .clicked()
            {
                if !(typed.starts_with("http://") || typed.starts_with("https://")) {
                    app.toast(
                        t!(
                            app.lang,
                            "нужна ссылка http:// или https://",
                            "a http:// or https:// link is required"
                        ),
                        false,
                    );
                } else if is_file {
                    app.install_catalog_entry(addons::CatalogEntry {
                        id: String::new(),
                        name: String::new(),
                        url: typed.clone(),
                        ..Default::default()
                    });
                    app.addon_repo_input.clear();
                } else if app.cfg.addons.add_repo(&typed) {
                    app.save();
                    app.addon_repo_input.clear();
                    app.addon_catalog_at = None;
                    app.load_addon_catalog();
                } else {
                    app.toast(
                        t!(app.lang, "магазин уже добавлен", "store already added"),
                        true,
                    );
                }
            }
        });

        if !app.cfg.addons.repos.is_empty() {
            ui.add_space(4.0);
            let mut drop: Option<String> = None;
            for repo in app.cfg.addons.repos.clone() {
                ui.horizontal(|ui| {
                    ui.label(theme::icon(theme::icon::STORE, 15.0, t.faint));
                    ui.label(
                        RichText::new(&repo)
                            .family(FontFamily::Monospace)
                            .size(11.0)
                            .color(t.muted),
                    );
                    if theme::nav_arrow(ui, t, theme::icon::CLOSE, true).clicked() {
                        drop = Some(repo.clone());
                    }
                });
            }
            if let Some(repo) = drop {
                app.cfg.addons.remove_repo(&repo);
                app.save();
                app.addon_catalog.retain(|e| !e.url.starts_with(&repo));
                app.addon_catalog_at = None;
            }
        }

        if !app.addon_catalog_err.is_empty() {
            ui.label(
                RichText::new(util::truncate(&app.addon_catalog_err, 220))
                    .size(11.5)
                    .color(t.warn),
            );
        }

        if app.addon_catalog.is_empty() {
            if app.cfg.addons.repos.is_empty() {
                theme::small(
                    ui,
                    t!(
                        app.lang,
                        "добавьте ссылку на магазин — он покажет свои пакеты здесь; ссылка на .sdwaddon ставится сразу.",
                        "add a store link and its packages appear here; a .sdwaddon link installs right away."
                    ),
                    t.muted,
                );
            }
            return;
        }

        let mut wanted: Option<addons::CatalogEntry> = None;
        ui.add_space(2.0);
        // Three columns: a Deck screen holds two readable ones and never
        // needs a horizontal scroll.
        let cols = if ui.available_width() >= 900.0 { 3 } else { 2 };
        let width = ((ui.available_width() - 8.0 * (cols as f32 - 1.0)) / cols as f32).max(160.0);
        let entries = app.addon_catalog.clone();
        for row in entries.chunks(cols) {
            ui.horizontal_top(|ui| {
                for entry in row {
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            if catalog_tile(app, ui, t, entry) {
                                wanted = Some(entry.clone());
                            }
                        },
                    );
                }
            });
            ui.add_space(6.0);
        }
        if let Some(entry) = wanted {
            app.install_catalog_entry(entry);
        }
    });
}

/// One catalogue tile. Returns true when "install" was pressed.
fn catalog_tile(app: &mut App, ui: &mut Ui, t: &Tokens, entry: &addons::CatalogEntry) -> bool {
    let installed = addons::find(&entry.id).is_some();
    let busy = app.addon_installing.as_deref() == Some(entry.id.as_str());
    let mut install = false;
    ui.set_width(ui.available_width());
    egui::Frame::new()
        .fill(t.well)
        .stroke(Stroke::new(
            1.0,
            if installed {
                t.good.gamma_multiply(0.5)
            } else {
                t.line
            },
        ))
        .corner_radius(0.0)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::icon(theme::icon::ADDON, 18.0, t.accent));
                ui.add(
                    egui::Label::new(
                        RichText::new(&entry.name)
                            .size(14.5)
                            .strong()
                            .color(t.strong),
                    )
                    .truncate(),
                );
            });
            let tagline = if entry.tagline.is_empty() {
                &entry.description
            } else {
                &entry.tagline
            };
            if !tagline.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(util::truncate(tagline, 110))
                            .size(12.0)
                            .color(t.muted),
                    )
                    .wrap(),
                );
            }
            let meta = [entry.version.as_str(), entry.author.as_str()]
                .into_iter()
                .filter(|x| !x.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            ui.horizontal(|ui| {
                if !meta.is_empty() {
                    ui.label(
                        RichText::new(meta)
                            .family(FontFamily::Monospace)
                            .size(10.0)
                            .color(t.faint),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if busy {
                        ui.spinner();
                    } else if installed {
                        ui.label(
                            RichText::new(t!(app.lang, "установлен", "installed"))
                                .size(11.5)
                                .color(t.good),
                        );
                    } else if theme::ghost(ui, t!(app.lang, "установить", "install"), t).clicked()
                    {
                        install = true;
                    }
                });
            });
        });
    install
}

fn addon_card(app: &mut App, ui: &mut Ui, t: &Tokens, addon: &Addon) {
    let m = &addon.manifest;
    let id = m.id.clone();
    let enabled = app.cfg.addons.is_on(&id);
    let theme_css = addon.theme_css();
    let clock_cfg = addon.clock();
    let mut delete = false;
    let mut want_on = enabled;

    theme::card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(&m.name).size(19.0).strong().color(t.strong));
                let meta = [m.version.as_str(), m.author.as_str(), m.scope.as_str()]
                    .into_iter()
                    .filter(|x| !x.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                if !meta.is_empty() {
                    ui.label(
                        RichText::new(meta)
                            .family(FontFamily::Monospace)
                            .size(10.5)
                            .color(t.faint),
                    );
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(t!(app.lang, "удалить", "remove"))
                                .size(12.5)
                                .color(t.bad),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, t.bad.gamma_multiply(0.4)))
                        .corner_radius(0.0)
                        .min_size(Vec2::new(0.0, 36.0)),
                    )
                    .clicked()
                {
                    delete = true;
                }
                theme::switch(ui, &mut want_on, t);
                ui.label(
                    RichText::new(if enabled {
                        t!(app.lang, "включён", "on")
                    } else {
                        t!(app.lang, "выключен", "off")
                    })
                    .size(12.0)
                    .color(if enabled { t.good } else { t.faint }),
                );
            });
        });
        if !m.description.is_empty() {
            theme::small(ui, &m.description, t.muted);
        }
        if !m.homepage.is_empty() {
            ui.label(
                RichText::new(&m.homepage)
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.accent),
            );
        }

        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if theme_css.is_some() {
                if theme::ghost(ui, t!(app.lang, "применить тему", "apply theme"), t).clicked()
                {
                    theme::apply_preset(&mut app.cfg.style, &format!("addon:{id}"));
                    app.style_draft = app.cfg.style.css.clone();
                    app.apply_style(ui.ctx());
                    app.toast(t!(app.lang, "тема применена", "theme applied"), true);
                }
            }
            if clock_cfg.is_some() {
                if theme::ghost(ui, t!(app.lang, "применить часы", "apply clock"), t).clicked()
                {
                    app.cfg.clock = clock_cfg.clone().unwrap_or_default();
                    app.save();
                    app.toast(t!(app.lang, "часы применены", "clock applied"), true);
                }
            }
            for action in &m.daemon.actions {
                let label = if action.label.is_empty() {
                    action.id.clone()
                } else {
                    action.label.clone()
                };
                if theme::ghost(ui, &label, t).clicked() {
                    let cmd = format!("addon:{id}:{}", action.id);
                    let action = Action::Remote {
                        host: None,
                        command: cmd,
                    };
                    app.execute_action(&action);
                }
            }
            for (k, v) in [
                ("theme", m.deck.theme.as_str()),
                ("clock", m.deck.clock.as_str()),
            ] {
                if !v.trim().is_empty() {
                    theme::chip(ui, k, t.accent);
                }
            }
            if !m.deck.tiles.is_empty() {
                theme::chip(ui, &format!("{} tiles", m.deck.tiles.len()), t.accent);
            }
            if !m.deck.sounds.is_empty() {
                theme::chip(ui, &format!("{} sounds", m.deck.sounds.len()), t.accent);
            }
        });

        if !m.settings.is_empty() {
            ui.add_space(4.0);
            ui.label(theme::hud(
                t!(app.lang, "настройки аддона", "addon settings"),
                t.faint,
            ));
            for field in &m.settings {
                field_ui(app, ui, t, &id, field);
            }
        }
    });

    if want_on != enabled {
        app.cfg.addons.set(&id, want_on);
        if want_on {
            defaults(app, addon);
        }
        app.save();
    }
    if delete {
        let _ = addons::remove(&id);
        app.cfg.addons.set(&id, false);
        app.cfg.addons.values.remove(&id);
        app.save();
        app.toast(t!(app.lang, "аддон удалён", "addon removed"), true);
    }
}

fn field_ui(app: &mut App, ui: &mut Ui, t: &Tokens, id: &str, field: &AddonField) {
    let value = app.cfg.addons.value(id, &field.key);
    let value = if value.is_null() {
        field.default.clone()
    } else {
        value
    };
    let label = if field.label.is_empty() {
        field.key.clone()
    } else {
        field.label.clone()
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&label).size(12.5).color(t.muted));
        match field.kind.as_str() {
            "bool" => {
                let mut on = value.as_bool().unwrap_or(false);
                if theme::switch(ui, &mut on, t).changed() {
                    set(app, id, &field.key, Value::Bool(on));
                }
            }
            "number" => {
                let mut n = value.as_f64().unwrap_or(0.0);
                let mut drag = egui::DragValue::new(&mut n);
                if let Some(min) = field.min {
                    drag = drag.range(min..=field.max.unwrap_or(f64::MAX));
                }
                if ui.add(drag).changed() {
                    set(app, id, &field.key, serde_json::json!(n));
                }
            }
            "color" => {
                let mut raw = value.as_str().unwrap_or("#3fd2ff").to_string();
                let mut c = theme::parse_color(&raw).unwrap_or(t.accent);
                if ui.color_edit_button_srgba(&mut c).changed() {
                    raw = format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                    set(app, id, &field.key, Value::String(raw));
                }
            }
            "choice" => {
                let current = value.as_str().unwrap_or("").to_string();
                for opt in &field.options {
                    if ui
                        .selectable_label(&current == opt, RichText::new(opt).size(12.0))
                        .clicked()
                    {
                        set(app, id, &field.key, Value::String(opt.clone()));
                    }
                }
            }
            _ => {
                let mut text = value.as_str().unwrap_or("").to_string();
                if ui
                    .add(TextEdit::singleline(&mut text).desired_width(240.0))
                    .changed()
                {
                    set(app, id, &field.key, Value::String(text));
                }
            }
        }
        if !field.hint.is_empty() {
            ui.label(RichText::new(&field.hint).size(11.0).color(t.faint));
        }
    });
}

fn set(app: &mut App, id: &str, key: &str, value: Value) {
    app.cfg.addons.set_value(id, key, value);
    app.save();
}

pub fn defaults(app: &mut App, addon: &Addon) {
    for field in &addon.manifest.settings {
        if app.cfg.addons.value(addon.id(), &field.key).is_null() {
            app.cfg
                .addons
                .set_value(addon.id(), &field.key, field.default.clone());
        }
    }
    // A sound pack replaces the per-cue files, once.
    for (cue, _) in &addon.manifest.deck.sounds {
        if let Some(path) = addon.sound_file(cue) {
            let path = path.display().to_string();
            let s = &mut app.cfg.sound;
            match cue.as_str() {
                "click" => s.click = path,
                "navigate" => s.navigate = path,
                "open" => s.open = path,
                "back" => s.back = path,
                "notify" => s.notify = path,
                "error" => s.error = path,
                "startup" => s.startup = path,
                _ => {}
            }
        }
    }
}

fn expand(raw: &str) -> std::path::PathBuf {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| std::path::PathBuf::from(raw));
    }
    std::path::PathBuf::from(raw)
}

fn now_tag() -> String {
    let now = theme::local_now();
    format!("{:02}-{:02}-{:02}", now.hour, now.minute, now.second)
}

// Keep the Task import used when the page grows background work.
#[allow(dead_code)]
fn _task(_: Task) {}
