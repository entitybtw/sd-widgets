//! the bind list and its editor.
//!
//! a bind is a trigger plus an action. triggers are captured live from the deck:
//! press a physical button or perform a trackpad gesture, and it is stored.

use crate::app::{App, Page};
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, Layout, Margin, RichText, Stroke, TextEdit, Ui,
    Vec2,
};
use sdw_core::model::{random_id, Action, Bind, GestureKind, GestureSpec, SwipeDir, Trigger};
use sdw_core::{t, Lang};

/// What kind of action a bind carries. Kept next to the editor because it is
/// the first choice there; the editor itself lives in the app state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    None,
    Shell,
    Launch,
    VolumeStep,
    VolumeSet,
    Mute,
    BrightnessStep,
    BrightnessSet,
    Media,
    Key,
    Page,
    Remote,
    Ha,
    Url,
    Http,
}

impl ActionKind {
    pub const ALL: [ActionKind; 15] = [
        ActionKind::None,
        ActionKind::Shell,
        ActionKind::Launch,
        ActionKind::VolumeStep,
        ActionKind::VolumeSet,
        ActionKind::Mute,
        ActionKind::BrightnessStep,
        ActionKind::BrightnessSet,
        ActionKind::Media,
        ActionKind::Key,
        ActionKind::Page,
        ActionKind::Remote,
        ActionKind::Ha,
        ActionKind::Url,
        ActionKind::Http,
    ];

    pub fn of(action: &Action) -> Self {
        match action {
            Action::None => Self::None,
            Action::Shell { .. } => Self::Shell,
            Action::Launch { .. } => Self::Launch,
            Action::Volume { .. } => Self::VolumeStep,
            Action::VolumeSet { .. } => Self::VolumeSet,
            Action::Mute => Self::Mute,
            Action::Brightness { .. } => Self::BrightnessStep,
            Action::BrightnessSet { .. } => Self::BrightnessSet,
            Action::Media { .. } => Self::Media,
            Action::Key { .. } => Self::Key,
            Action::Page { .. } => Self::Page,
            Action::Remote { .. } => Self::Remote,
            Action::Ha { .. } => Self::Ha,
            Action::Url { .. } => Self::Url,
            Action::Http { .. } => Self::Http,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Self::None => t!(lang, "ничего", "nothing"),
            Self::Shell => t!(lang, "команда на деке", "shell command on the deck"),
            Self::Launch => t!(lang, "приложение на компьютере", "app on the computer"),
            Self::VolumeStep => t!(lang, "громкость ±", "volume ±"),
            Self::VolumeSet => t!(lang, "громкость", "volume"),
            Self::Mute => t!(lang, "выкл. звук", "mute"),
            Self::BrightnessStep => t!(lang, "яркость ±", "brightness ±"),
            Self::BrightnessSet => t!(lang, "яркость", "brightness"),
            Self::Media => t!(lang, "медиа", "media"),
            Self::Key => t!(lang, "клавиши", "keys"),
            Self::Page => t!(lang, "страница", "page"),
            Self::Remote => t!(lang, "компьютер", "computer"),
            Self::Ha => t!(lang, "home assistant", "home assistant"),
            Self::Url => t!(lang, "url", "url"),
            Self::Http => t!(lang, "http-запрос", "http request"),
        }
    }

    pub fn default_action(self) -> Action {
        match self {
            Self::None => Action::None,
            Self::Shell => Action::Shell {
                command: "notify-send sd-widgets hello".into(),
            },
            Self::Launch => Action::Launch {
                target: "steam".into(),
            },
            Self::VolumeStep => Action::Volume { delta: 5 },
            Self::VolumeSet => Action::VolumeSet { percent: 50 },
            Self::Mute => Action::Mute,
            Self::BrightnessStep => Action::Brightness { delta: 5 },
            Self::BrightnessSet => Action::BrightnessSet { percent: 50 },
            Self::Media => Action::Media {
                command: "play_pause".into(),
            },
            Self::Key => Action::Key {
                combo: "ctrl+alt+t".into(),
            },
            Self::Page => Action::Page {
                page: "home".into(),
            },
            Self::Remote => Action::Remote {
                host: None,
                command: "media play_pause".into(),
            },
            Self::Ha => Action::Ha {
                host: None,
                domain: "light".into(),
                service: "toggle".into(),
                entity: String::new(),
                data: serde_json::Value::Null,
            },
            Self::Url => Action::Url {
                url: "https://".into(),
            },
            Self::Http => Action::Http {
                method: "GET".into(),
                url: "https://".into(),
                headers: Default::default(),
                body: String::new(),
            },
        }
    }
}

pub struct BindEditor {
    pub id: Option<String>,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
    pub combo_codes: String,
    pub combo_label: String,
    pub manual_code: String,
    pub manual_label: String,
}

impl BindEditor {
    pub fn new() -> Self {
        Self {
            id: None,
            name: String::new(),
            enabled: true,
            trigger: Trigger::Gesture {
                spec: GestureSpec {
                    kind: GestureKind::Swipe,
                    fingers: 3,
                    dir: Some(SwipeDir::Left),
                },
            },
            action: Action::None,
            combo_codes: String::new(),
            combo_label: String::new(),
            manual_code: String::new(),
            manual_label: String::new(),
        }
    }

    pub fn for_trigger(name: impl Into<String>, trigger: Trigger) -> Self {
        let mut editor = Self::new();
        editor.name = name.into();
        editor.set_trigger(trigger);
        editor
    }

    pub fn from_bind(bind: &Bind) -> Self {
        let mut editor = Self::new();
        editor.id = Some(bind.id.clone());
        editor.name = bind.name.clone();
        editor.enabled = bind.enabled;
        editor.action = bind.action.clone();
        editor.set_trigger(bind.trigger.clone());
        editor
    }

    fn set_trigger(&mut self, trigger: Trigger) {
        match &trigger {
            Trigger::Combo { codes, label } => {
                self.combo_codes = codes.join(", ");
                self.combo_label = label.clone();
            }
            Trigger::Button { code, label, .. } => {
                self.manual_code = code.clone();
                self.manual_label = label.clone();
            }
            _ => {}
        }
        self.trigger = trigger;
    }

    fn trigger_for_save(&self) -> Trigger {
        match &self.trigger {
            Trigger::Combo { .. } => {
                let codes: Vec<String> = self
                    .combo_codes
                    .split([',', '+', ' '])
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                Trigger::Combo {
                    codes,
                    label: if self.combo_label.trim().is_empty() {
                        self.combo_codes.clone()
                    } else {
                        self.combo_label.clone()
                    },
                }
            }
            other => other.clone(),
        }
    }
}

enum Outcome {
    None,
    Save,
    Delete,
    Cancel,
}

pub fn capture_button(app: &mut App, code: u16, label: &str, device: &str) {
    app.binds.capture = false;
    let Some(mut editor) = app.binds.editor.take() else {
        return;
    };
    editor.set_trigger(Trigger::Button {
        device: device.to_string(),
        code: code.to_string(),
        label: label.to_string(),
    });
    if editor.name.trim().is_empty() {
        editor.name = label.to_string();
    }
    app.binds.editor = Some(editor);
    app.toast(
        format!(
            "{}: {label} ({code})",
            t!(app.lang, "кнопка записана", "button captured")
        ),
        true,
    );
}

pub fn capture_gesture(app: &mut App, spec: &GestureSpec) {
    app.binds.capture = false;
    let Some(mut editor) = app.binds.editor.take() else {
        return;
    };
    editor.set_trigger(Trigger::Gesture { spec: *spec });
    if editor.name.trim().is_empty() {
        editor.name = spec.label_lang(app.lang);
    }
    app.binds.editor = Some(editor);
    app.toast(
        format!(
            "{}: {}",
            t!(app.lang, "жест записан", "gesture captured"),
            spec.label_lang(app.lang)
        ),
        true,
    );
}

fn title(ui: &mut Ui, t: &Tokens, text: &str, sub: &str) {
    ui.label(RichText::new(text).size(26.0).strong().color(t.strong));
    if !sub.is_empty() {
        ui.label(RichText::new(sub).size(13.5).color(t.muted));
    }
    ui.add_space(2.0);
}

// ---------------------------------------------------------------- list

pub fn list_ui(app: &mut App, ui: &mut Ui, t: &Tokens) {
    // Headless checks can jump straight into an editor: `SDW_BIND=<id>:launch`
    // or `SDW_BIND=<id>:shell`. It changes nothing on a normal run.
    if let Some((id, kind)) = app.binds.open.clone() {
        app.binds.open = None;
        if let Some(bind) = app.cfg.binds.iter().find(|b| b.id == id).cloned() {
            let mut editor = BindEditor::from_bind(&bind);
            editor.action = kind.default_action();
            app.binds.editor = Some(editor);
            app.set_bind_capture(false);
        }
    }
    title(
        ui,
        t,
        t!(app.lang, "бинды", "binds"),
        t!(
            app.lang,
            "кнопки и жесты, которые что-то делают. работают поверх любой игры.",
            "buttons and gestures that do something. they work over any game."
        ),
    );

    ui.horizontal(|ui| {
        if theme::primary(ui, t!(app.lang, "+ новый бинд", "+ new bind"), t).clicked() {
            app.binds.editor = Some(BindEditor::new());
            app.set_bind_capture(true);
        }
        if theme::ghost(ui, t!(app.lang, "набор по умолчанию", "default set"), t).clicked()
        {
            seed_defaults(app);
        }
        let mut global = app.cfg.global_gestures;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::switch(ui, &mut global, t);
            ui.label(
                RichText::new(t!(app.lang, "свайпы страниц", "page swipes"))
                    .size(13.5)
                    .color(t.muted),
            );
        });
        if global != app.cfg.global_gestures {
            app.cfg.global_gestures = global;
            app.save();
        }
    });
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let hint = t!(app.lang, "поиск", "search");
        ui.add(
            TextEdit::singleline(&mut app.binds.filter)
                .hint_text(hint)
                .desired_width(280.0),
        );
        ui.label(
            RichText::new(format!(
                "{} {}",
                app.cfg.binds.len(),
                t!(app.lang, "биндов", "binds")
            ))
            .family(FontFamily::Monospace)
            .size(12.0)
            .color(t.faint),
        );
    });
    ui.add_space(4.0);

    let needle = app.binds.filter.trim().to_lowercase();
    let binds = app.cfg.binds.clone();
    let mut toggle: Option<(usize, bool)> = None;
    let mut edit: Option<usize> = None;
    let mut delete: Option<usize> = None;

    if binds.is_empty() {
        egui::Frame::new()
            .fill(t.card)
            .stroke(t.hairline())
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::same(22))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "пока нет ни одного бинда",
                        "there are no binds yet"
                    ))
                    .size(17.0)
                    .color(t.strong),
                );
                ui.label(
                    RichText::new(t!(
                        app.lang,
                        "нажмите «новый бинд», затем нажмите кнопку на деке — она запишется сама.",
                        "press “new bind”, then press a button on the deck — it is recorded automatically."
                    ))
                    .size(14.0)
                    .color(t.muted),
                );
            });
        return;
    }

    for (i, bind) in binds.iter().enumerate() {
        if !needle.is_empty() {
            let hay = format!(
                "{} {} {}",
                bind.name,
                bind.trigger.label_lang(app.lang),
                bind.action.label_lang(app.lang)
            )
            .to_lowercase();
            if !hay.contains(&needle) {
                continue;
            }
        }
        let mut enabled = bind.enabled;
        egui::Frame::new()
            .fill(t.card)
            .stroke(Stroke::new(
                1.0,
                if enabled {
                    t.line
                } else {
                    t.line.gamma_multiply(0.4)
                },
            ))
            .corner_radius(CornerRadius::ZERO)
            .inner_margin(Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(if bind.name.trim().is_empty() {
                                    bind.trigger.label_lang(app.lang)
                                } else {
                                    bind.name.clone()
                                })
                                .size(16.5)
                                .strong()
                                .color(if enabled {
                                    t.strong
                                } else {
                                    t.muted
                                }),
                            )
                            .wrap(),
                        );
                        ui.label(
                            RichText::new(bind.trigger.label_lang(app.lang))
                                .family(FontFamily::Monospace)
                                .size(12.0)
                                .color(t.accent),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::ghost(ui, t!(app.lang, "изменить", "edit"), t).clicked() {
                            edit = Some(i);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(t!(app.lang, "удалить", "delete"))
                                        .size(13.0)
                                        .color(t.bad),
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, t.bad.gamma_multiply(0.45)))
                                .corner_radius(CornerRadius::ZERO),
                            )
                            .clicked()
                        {
                            delete = Some(i);
                        }
                        if theme::switch(ui, &mut enabled, t).changed() {
                            toggle = Some((i, enabled));
                        }
                        ui.label(
                            RichText::new(bind.action.label_lang(app.lang))
                                .size(13.5)
                                .color(t.muted),
                        );
                    });
                });
            });
        ui.add_space(6.0);
    }

    if let Some((i, value)) = toggle {
        if let Some(bind) = app.cfg.binds.get_mut(i) {
            bind.enabled = value;
        }
        app.save();
    }
    if let Some(i) = edit {
        if let Some(bind) = app.cfg.binds.get(i) {
            app.binds.editor = Some(BindEditor::from_bind(bind));
        }
    }
    if let Some(i) = delete {
        if i < app.cfg.binds.len() {
            let bind = app.cfg.binds.remove(i);
            app.save();
            app.toast(
                format!(
                    "{}: {}",
                    t!(app.lang, "бинт удалён", "bind deleted"),
                    bind.name
                ),
                true,
            );
        }
    }
}

fn seed_defaults(app: &mut App) {
    let lang = app.lang;
    let defaults = vec![
        (
            t!(lang, "звук: выключить", "sound: mute"),
            Trigger::Button {
                device: "steam deck".into(),
                code: crate::input::BTN_L4.to_string(),
                label: "L4".into(),
            },
            Action::Mute,
        ),
        (
            t!(lang, "громкость −", "volume −"),
            Trigger::Button {
                device: "steam deck".into(),
                code: crate::input::BTN_L5.to_string(),
                label: "L5".into(),
            },
            Action::Volume { delta: -5 },
        ),
        (
            t!(lang, "громкость +", "volume +"),
            Trigger::Button {
                device: "steam deck".into(),
                code: crate::input::BTN_R4.to_string(),
                label: "R4".into(),
            },
            Action::Volume { delta: 5 },
        ),
        (
            t!(lang, "яркость −", "brightness −"),
            Trigger::Button {
                device: "steam deck".into(),
                code: crate::input::BTN_R5.to_string(),
                label: "R5".into(),
            },
            Action::Brightness { delta: -5 },
        ),
        (
            t!(
                lang,
                "три пальца вправо: компьютеры",
                "three fingers right: computers"
            ),
            Trigger::Gesture {
                spec: GestureSpec {
                    kind: GestureKind::Swipe,
                    fingers: 3,
                    dir: Some(SwipeDir::Right),
                },
            },
            Action::Page {
                page: "computers".into(),
            },
        ),
        (
            t!(
                lang,
                "четыре пальца вниз: уведомления",
                "four fingers down: notifications"
            ),
            Trigger::Gesture {
                spec: GestureSpec {
                    kind: GestureKind::Swipe,
                    fingers: 4,
                    dir: Some(SwipeDir::Down),
                },
            },
            Action::Page {
                page: "notifications".into(),
            },
        ),
    ];
    let mut added = 0;
    for (name, trigger, action) in defaults {
        let clash = app
            .cfg
            .binds
            .iter()
            .any(|b| b.trigger == trigger || b.name == name);
        if !clash {
            app.cfg.binds.push(Bind::new(name, trigger, action));
            added += 1;
        }
    }
    app.save();
    app.toast(
        if added == 0 {
            t!(app.lang, "набор уже добавлен", "the set is already there").to_string()
        } else {
            format!(
                "{}: {added}",
                t!(app.lang, "добавлено биндов", "binds added")
            )
        },
        true,
    );
}

// ---------------------------------------------------------------- editor

pub fn editor_ui(app: &mut App, ui: &mut Ui, t: &Tokens) {
    let Some(mut editor) = app.binds.editor.take() else {
        return;
    };
    let mut capture = app.binds.capture;
    let mut outcome = Outcome::None;
    let lang = app.lang;
    let host_names: Vec<String> = app.cfg.hosts.iter().map(|h| h.name.clone()).collect();
    let pages = [
        "home",
        "computers",
        "gamepad",
        "screen",
        "notifications",
        "ha",
        "binds",
        "settings",
    ];

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(if editor.id.is_some() {
                    t!(lang, "изменить бинд", "edit bind")
                } else {
                    t!(lang, "новый бинд", "new bind")
                })
                .size(26.0)
                .strong()
                .color(t.strong),
            );
            ui.label(
                RichText::new(t!(
                    lang,
                    "кнопка или жест слева, действие справа",
                    "a button or gesture on the left, an action on the right"
                ))
                .size(13.5)
                .color(t.muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::ghost(ui, t!(lang, "назад", "back"), t).clicked() {
                outcome = Outcome::Cancel;
            }
            if theme::primary(ui, t!(lang, "сохранить", "save"), t).clicked() {
                outcome = Outcome::Save;
            }
        });
    });
    ui.add_space(8.0);

    ui.columns(2, |cols| {
        // ------------------------------------------------ trigger
        let ui = &mut cols[0];
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(t!(lang, "когда", "trigger"), t.faint));
            ui.add_space(4.0);
            ui.add(
                TextEdit::singleline(&mut editor.name)
                    .hint_text(t!(lang, "название бинда", "bind name"))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let is_button = matches!(editor.trigger, Trigger::Button { .. });
                let is_gesture = matches!(editor.trigger, Trigger::Gesture { .. });
                let is_combo = matches!(editor.trigger, Trigger::Combo { .. });
                if ui
                    .selectable_label(is_button, t!(lang, "кнопка", "button"))
                    .clicked()
                {
                    editor.set_trigger(Trigger::Button {
                        device: "steam deck".into(),
                        code: crate::input::BTN_L4.to_string(),
                        label: "L4".into(),
                    });
                }
                if ui
                    .selectable_label(is_gesture, t!(lang, "жест", "gesture"))
                    .clicked()
                {
                    editor.set_trigger(Trigger::Gesture {
                        spec: GestureSpec::default(),
                    });
                }
                if ui
                    .selectable_label(is_combo, t!(lang, "комбо", "combo"))
                    .clicked()
                {
                    editor.combo_codes.clear();
                    editor.combo_label.clear();
                    editor.set_trigger(Trigger::Combo {
                        codes: Vec::new(),
                        label: String::new(),
                    });
                }
            });
            ui.add_space(4.0);

            match &mut editor.trigger {
                Trigger::Button {
                    device,
                    code,
                    label,
                } => {
                    ui.label(
                        RichText::new(format!("{} · {code}", label))
                            .family(FontFamily::Monospace)
                            .size(13.0)
                            .color(t.accent),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!(lang, "код", "code")).size(12.5).color(t.muted));
                        ui.add(TextEdit::singleline(code).desired_width(90.0));
                        ui.label(RichText::new(t!(lang, "имя", "name")).size(12.5).color(t.muted));
                        ui.add(TextEdit::singleline(label).desired_width(120.0));
                    });
                    ui.label(
                        RichText::new(format!(
                            "{}: {device}",
                            t!(lang, "устройство", "device")
                        ))
                        .size(11.5)
                        .color(t.faint),
                    );
                }
                Trigger::Gesture { spec } => {
                    let mut kind = spec.kind;
                    let mut fingers = spec.fingers;
                    let mut dir = spec.dir.unwrap_or(SwipeDir::Left);
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("bind-gesture-kind")
                            .selected_text(gesture_kind_label(kind, lang))
                            .show_ui(ui, |ui| {
                                for k in [
                                    GestureKind::Swipe,
                                    GestureKind::Tap,
                                    GestureKind::LongPress,
                                    GestureKind::PinchIn,
                                    GestureKind::PinchOut,
                                ] {
                                    if ui
                                        .selectable_label(kind == k, gesture_kind_label(k, lang))
                                        .clicked()
                                    {
                                        kind = k;
                                    }
                                }
                            });
                        ui.label(
                            RichText::new(t!(lang, "пальцев", "fingers"))
                                .size(12.0)
                                .color(t.muted),
                        );
                        for n in 1..=4u8 {
                            if ui
                                .selectable_label(fingers == n, n.to_string())
                                .clicked()
                            {
                                fingers = n;
                            }
                        }
                    });
                    if kind == GestureKind::Swipe {
                        ui.horizontal(|ui| {
                            for d in [
                                SwipeDir::Left,
                                SwipeDir::Right,
                                SwipeDir::Up,
                                SwipeDir::Down,
                            ] {
                                if ui
                                    .selectable_label(dir == d, swipe_arrow(d))
                                    .on_hover_text(d.label_lang(lang))
                                    .clicked()
                                {
                                    dir = d;
                                }
                            }
                        });
                    }
                    if kind != spec.kind
                        || fingers != spec.fingers
                        || (kind == GestureKind::Swipe && Some(dir) != spec.dir)
                    {
                        *spec = GestureSpec {
                            kind,
                            fingers,
                            dir: (kind == GestureKind::Swipe).then_some(dir),
                        };
                    }
                    ui.label(
                        RichText::new(spec.label_lang(lang))
                            .family(FontFamily::Monospace)
                            .size(13.0)
                            .color(t.accent),
                    );
                }
                Trigger::Combo { codes, label } => {
                    ui.add(
                        TextEdit::singleline(&mut editor.combo_codes)
                            .hint_text(t!(
                                lang,
                                "коды через запятую, например 46, 48",
                                "comma separated codes, for example 46, 48"
                            ))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add(
                        TextEdit::singleline(&mut editor.combo_label)
                            .hint_text(t!(lang, "подпись", "label"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.label(
                        RichText::new(codes.join(" + "))
                            .family(FontFamily::Monospace)
                            .size(12.0)
                            .color(t.faint),
                    );
                    if label.is_empty() {
                        *label = editor.combo_label.clone();
                    }
                }
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if capture {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "слушаю деку… нажмите кнопку или сделайте жест",
                            "listening to the deck… press a button or make a gesture"
                        ))
                        .size(13.0)
                        .color(t.warn),
                    );
                    if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
                        capture = false;
                    }
                } else if theme::ghost(
                    ui,
                    t!(lang, "записать с деки", "capture from deck"),
                    t,
                )
                .clicked()
                {
                    capture = true;
                    app.set_bind_capture(true);
                    app.toast(
                        t!(
                            lang,
                            "нажмите любую кнопку на деке или сделайте жест",
                            "press any button on the deck or make a gesture"
                        ),
                        true,
                    );
                }
            });
            ui.add_space(2.0);
            ui.label(
                RichText::new(t!(
                    lang,
                    "жесты читаются с тачпадов и тачскрина, кнопки — со всех кнопок деки.",
                    "gestures come from the trackpads and touchscreen, buttons from the deck's buttons."
                ))
                .size(11.5)
                .color(t.faint),
            );
        });

        // ------------------------------------------------ action
        let ui = &mut cols[1];
        theme::card(ui, t, |ui| {
            ui.label(theme::hud(t!(lang, "что сделать", "action"), t.faint));
            ui.add_space(4.0);
            let mut kind = ActionKind::of(&editor.action);
            egui::ComboBox::from_id_salt("bind-action-kind")
                .selected_text(kind.label(lang))
                .width(ui.available_width().min(260.0))
                .show_ui(ui, |ui| {
                    for k in ActionKind::ALL {
                        if ui.selectable_label(kind == k, k.label(lang)).clicked() {
                            kind = k;
                        }
                    }
                });
            if ActionKind::of(&editor.action) != kind {
                editor.action = kind.default_action();
            }
            ui.add_space(6.0);
            match &mut editor.action {
                Action::None => {
                    ui.label(
                        RichText::new(t!(lang, "ничего не делать", "do nothing"))
                            .size(13.5)
                            .color(t.muted),
                    );
                }
                Action::Shell { command } => {
                    ui.label(
                        RichText::new(t!(lang, "команда оболочки", "shell command"))
                            .size(12.5)
                            .color(t.muted),
                    );
                    ui.add(
                        TextEdit::multiline(command)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "выполняется на этой деке, не на компьютере.",
                            "runs on this deck, not on the computer."
                        ))
                        .size(11.0)
                        .color(t.faint),
                    );
                }
                Action::Launch { target } => {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "приложение запускается на компьютере — из списка или вручную",
                            "the application starts on the computer — from the list or by hand"
                        ))
                        .size(12.5)
                        .color(t.muted),
                    );
                    // The list is the one the control page already reads, so
                    // the picker costs nothing extra. The typed value stays
                    // visible: an older bind may name something the scan skips.
                    if let Some(picked) = launch_picker(app, ui, t, target) {
                        *target = picked;
                    }
                    ui.add(
                        TextEdit::singleline(target)
                            .hint_text("steam://rungameid/…")
                            .desired_width(f32::INFINITY),
                    );
                }
                Action::Volume { delta } => {
                    let mut v = *delta as f32;
                    if theme::slider(
                        ui,
                        t,
                        t!(lang, "громкость ±", "volume ±"),
                        &mut v,
                        -50.0..=50.0,
                        |v| format!("{v:+.0}%"),
                    ) {
                        *delta = v.round() as i32;
                    }
                }
                Action::VolumeSet { percent } => {
                    let mut v = *percent as f32;
                    if theme::slider(
                        ui,
                        t,
                        t!(lang, "громкость", "volume"),
                        &mut v,
                        0.0..=100.0,
                        |v| format!("{v:.0}%"),
                    ) {
                        *percent = v.round().clamp(0.0, 100.0) as u8;
                    }
                }
                Action::Mute => {
                    ui.label(RichText::new(t!(lang, "переключить mute", "toggle mute")).size(13.5).color(t.muted));
                }
                Action::Brightness { delta } => {
                    let mut v = *delta as f32;
                    if theme::slider(
                        ui,
                        t,
                        t!(lang, "яркость ±", "brightness ±"),
                        &mut v,
                        -50.0..=50.0,
                        |v| format!("{v:+.0}%"),
                    ) {
                        *delta = v.round() as i32;
                    }
                }
                Action::BrightnessSet { percent } => {
                    let mut v = *percent as f32;
                    if theme::slider(
                        ui,
                        t,
                        t!(lang, "яркость", "brightness"),
                        &mut v,
                        0.0..=100.0,
                        |v| format!("{v:.0}%"),
                    ) {
                        *percent = v.round().clamp(0.0, 100.0) as u8;
                    }
                }
                Action::Media { command } => {
                    ui.horizontal(|ui| {
                        for (value, label) in [
                            ("play_pause", theme::icon::PLAY_PAUSE),
                            ("next", theme::icon::NEXT),
                            ("previous", theme::icon::PREV),
                            ("stop", theme::icon::STOP),
                        ] {
                            if ui
                                .selectable_label(
                                    command.as_str() == value,
                                    theme::icon(label, 16.0, t.ink),
                                )
                                .on_hover_text(value)
                                .clicked()
                            {
                                *command = value.into();
                            }
                        }
                    });
                }
                Action::Key { combo } => {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "например ctrl+alt+t или super+Return",
                            "for example ctrl+alt+t or super+Return"
                        ))
                        .size(12.5)
                        .color(t.muted),
                    );
                    ui.add(TextEdit::singleline(combo).desired_width(f32::INFINITY));
                }
                Action::Page { page } => {
                    egui::ComboBox::from_id_salt("bind-action-page")
                        .selected_text(page.clone())
                        .show_ui(ui, |ui| {
                            for p in pages {
                                if ui.selectable_label(page == p, Page::from_key(p).label(lang)).clicked() {
                                    *page = p.to_string();
                                }
                            }
                        });
                }
                Action::Remote { host, command } => {
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "белый список: media, volume, mute, lock, monitor, open:https://…, launch:steam",
                            "whitelist: media, volume, mute, lock, monitor, open:https://…, launch:steam"
                        ))
                        .size(12.0)
                        .color(t.muted),
                    );
                    ui.add(TextEdit::singleline(command).desired_width(f32::INFINITY));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!(lang, "компьютер", "computer")).size(12.5).color(t.muted));
                        let current = host.clone().unwrap_or_default();
                        egui::ComboBox::from_id_salt("bind-action-host")
                            .selected_text(if current.is_empty() {
                                t!(lang, "активный", "active").to_string()
                            } else {
                                current.clone()
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(current.is_empty(), t!(lang, "активный", "active"))
                                    .clicked()
                                {
                                    *host = None;
                                }
                                for name in &host_names {
                                    if ui.selectable_label(&current == name, name).clicked() {
                                        *host = Some(name.clone());
                                    }
                                }
                            });
                    });
                }
                Action::Ha {
                    host,
                    domain,
                    service,
                    entity,
                    ..
                } => {
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(domain)
                                .hint_text("light")
                                .desired_width(90.0),
                        );
                        ui.add(
                            TextEdit::singleline(service)
                                .hint_text("toggle")
                                .desired_width(90.0),
                        );
                    });
                    ui.add(
                        TextEdit::singleline(entity)
                            .hint_text("light.kitchen")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!(lang, "компьютер", "computer")).size(12.5).color(t.muted));
                        let current = host.clone().unwrap_or_default();
                        egui::ComboBox::from_id_salt("bind-ha-host")
                            .selected_text(if current.is_empty() {
                                t!(lang, "активный", "active").to_string()
                            } else {
                                current.clone()
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(current.is_empty(), t!(lang, "активный", "active"))
                                    .clicked()
                                {
                                    *host = None;
                                }
                                for name in &host_names {
                                    if ui.selectable_label(&current == name, name).clicked() {
                                        *host = Some(name.clone());
                                    }
                                }
                            });
                    });
                }
                Action::Url { url } => {
                    ui.add(TextEdit::singleline(url).desired_width(f32::INFINITY));
                }
                Action::Http {
                    method,
                    url,
                    headers,
                    body,
                } => {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("bind-http-method")
                            .selected_text(
                                RichText::new(method.clone())
                                    .family(FontFamily::Monospace)
                                    .size(12.5),
                            )
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
                                    if ui.selectable_label(method == m, m).clicked() {
                                        *method = m.into();
                                    }
                                }
                            });
                        ui.add(
                            TextEdit::singleline(url)
                                .hint_text("https://api.example/…")
                                .desired_width(f32::INFINITY),
                        );
                    });
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "заголовки, по одному в строке: имя: значение",
                            "headers, one per line: name: value"
                        ))
                        .size(11.5)
                        .color(t.faint),
                    );
                    let mut text = headers
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if ui
                        .add(
                            TextEdit::multiline(&mut text)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    {
                        *headers = parse_headers(&text);
                    }
                    ui.label(
                        RichText::new(t!(
                            lang,
                            "тело запроса; {key} подставляется из настроек аддона",
                            "request body; {key} comes from the addon's settings"
                        ))
                        .size(11.5)
                        .color(t.faint),
                    );
                    ui.add(
                        TextEdit::multiline(body)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                theme::switch(ui, &mut editor.enabled, t);
                ui.label(
                    RichText::new(if editor.enabled {
                        t!(lang, "включён", "enabled")
                    } else {
                        t!(lang, "выключен", "disabled")
                    })
                    .size(13.0)
                    .color(t.muted),
                );
            });
            if !matches!(editor.action, Action::None) {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(editor.action.label_lang(lang))
                        .family(FontFamily::Monospace)
                        .size(11.5)
                        .color(t.faint),
                );
            }
        });
    });

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if theme::primary(ui, t!(lang, "сохранить бинд", "save bind"), t).clicked() {
            outcome = Outcome::Save;
        }
        if theme::ghost(ui, t!(lang, "отмена", "cancel"), t).clicked() {
            outcome = Outcome::Cancel;
        }
        if editor.id.is_some() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(t!(lang, "удалить бинд", "delete bind"))
                                .size(13.0)
                                .color(t.bad),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, t.bad.gamma_multiply(0.45)))
                        .corner_radius(CornerRadius::ZERO),
                    )
                    .clicked()
                {
                    outcome = Outcome::Delete;
                }
            });
        }
    });

    match outcome {
        Outcome::None => {
            app.binds.editor = Some(editor);
            app.binds.capture = capture;
        }
        Outcome::Cancel => {
            app.binds.capture = false;
        }
        Outcome::Save => {
            let trigger = editor.trigger_for_save();
            if matches!(&trigger, Trigger::Combo { codes, .. } if codes.len() < 2) {
                app.binds.editor = Some(editor);
                app.binds.capture = false;
                app.toast(
                    t!(
                        lang,
                        "для комбо нужно минимум две кнопки",
                        "a combo needs at least two buttons"
                    ),
                    false,
                );
                return;
            }
            let name = if editor.name.trim().is_empty() {
                trigger.label_lang(lang)
            } else {
                editor.name.trim().to_string()
            };
            let id = editor
                .id
                .clone()
                .unwrap_or_else(|| random_id(8).to_string());
            let bind = Bind {
                id: id.clone(),
                name,
                trigger,
                action: editor.action.clone(),
                enabled: editor.enabled,
            };
            if let Some(existing) = app.cfg.binds.iter_mut().find(|b| b.id == id) {
                *existing = bind;
            } else {
                app.cfg.binds.push(bind);
            }
            app.binds.capture = false;
            app.save();
            app.toast(t!(lang, "бинд сохранён", "bind saved"), true);
        }
        Outcome::Delete => {
            if let Some(id) = editor.id.clone() {
                app.cfg.binds.retain(|b| b.id != id);
            }
            app.binds.capture = false;
            app.save();
            app.toast(t!(lang, "бинд удалён", "bind deleted"), true);
        }
    }
}

/// Pick the application a Launch action starts.
///
/// The list is the same one the control page shows (ordered by how often the
/// computer's user starts each program, with the real pictures), so a bind is
/// chosen from the catalog instead of typed. The text field stays below the
/// list, because an old bind may name something the scan skips (a
/// `steam://` link, a terminal command).
///
/// Returns `Some(id)` when the user picked an entry.
fn launch_picker(app: &mut App, ui: &mut Ui, t: &Tokens, target: &str) -> Option<String> {
    let lang = app.lang;
    let online = app.online();
    let stale = app
        .control_apps_at
        .map(|at| at.elapsed() > std::time::Duration::from_secs(90))
        .unwrap_or(true);
    if online && stale && app.control_apps.is_empty() && !app.control_apps_pending {
        app.load_control_apps();
    }

    let mut picked = None;
    theme::row_ends(
        ui,
        |ui| {
            ui.label(theme::hud(t!(lang, "приложение", "application"), t.faint));
            if app.control_apps_pending {
                ui.spinner();
            }
        },
        |ui| {
            if online {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        app.control_apps.len(),
                        t!(lang, "на компьютере", "on the computer")
                    ))
                    .family(FontFamily::Monospace)
                    .size(10.5)
                    .color(t.faint),
                );
            } else {
                ui.label(
                    RichText::new(t!(lang, "компьютер недоступен", "the computer is offline"))
                        .size(11.0)
                        .color(t.warn),
                );
            }
        },
    );

    if !app.control_apps_err.is_empty() {
        ui.label(
            RichText::new(sdw_core::util::truncate(&app.control_apps_err, 200))
                .size(11.5)
                .color(t.warn),
        );
    }

    ui.add(
        TextEdit::singleline(&mut app.binds.launch_filter)
            .hint_text(t!(lang, "поиск приложения", "search applications"))
            .desired_width((ui.available_width() * 0.62).max(220.0)),
    );

    if !app.control_apps.is_empty() {
        let needle = app.binds.launch_filter.trim().to_lowercase();
        let list: Vec<sdw_core::model::AppEntry> = app
            .control_apps
            .iter()
            .filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.comment.to_lowercase().contains(&needle)
                    || a.id.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();
        if list.is_empty() {
            theme::small(ui, t!(lang, "ничего не найдено", "nothing found"), t.muted);
        } else {
            app.want_app_icons(&list);
            app.ensure_app_icons(ui.ctx());
            egui::ScrollArea::vertical()
                .id_salt("bind-launch-apps")
                .max_height(216.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let width = ui.available_width();
                    for entry in &list {
                        let current = entry.id == target.trim();
                        let hit = egui::Frame::new()
                            .fill(if current { t.bg2 } else { t.well })
                            .stroke(Stroke::new(1.0, if current { t.accent } else { t.line }))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_width(width - 22.0);
                                let inner = ui
                                    .horizontal(|ui| {
                                        let glyph = match entry.kind {
                                            sdw_core::model::AppKind::Terminal => {
                                                theme::icon::TERMINAL
                                            }
                                            sdw_core::model::AppKind::Gui => theme::icon::WIDGETS,
                                        };
                                        match app.app_icons.get(&entry.id) {
                                            Some(tex) => {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    Vec2::splat(16.0),
                                                    egui::Sense::hover(),
                                                );
                                                ui.painter().image(
                                                    tex.id(),
                                                    rect,
                                                    egui::Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            }
                                            None => {
                                                ui.label(theme::icon(glyph, 16.0, t.accent));
                                            }
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&entry.name)
                                                    .size(13.5)
                                                    .color(t.strong),
                                            )
                                            .truncate(),
                                        );
                                        if current {
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        theme::icon(
                                                            theme::icon::CHECK,
                                                            16.0,
                                                            t.accent,
                                                        )
                                                        .size(16.0)
                                                        .color(t.accent),
                                                    );
                                                },
                                            );
                                        }
                                    })
                                    .response;
                                let hit = ui
                                    .interact(
                                        ui.min_rect(),
                                        egui::Id::new(("bind-launch", &entry.id)),
                                        egui::Sense::click(),
                                    )
                                    .union(inner);
                                // The row itself is the target: the whole
                                // rectangle and the gutters around it, so a
                                // finger does not have to hit the label.
                                let pad = ui.spacing().item_spacing;
                                let hit = ui
                                    .interact(
                                        ui.min_rect().expand2(Vec2::new(0.0, pad.y * 0.5)),
                                        egui::Id::new((
                                            "bind-launch-row",
                                            &entry.id,
                                            ui.available_width() as u32,
                                        )),
                                        egui::Sense::click(),
                                    )
                                    .union(hit);
                                if hit.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                hit.clicked()
                            })
                            .inner;
                        if hit {
                            picked = Some(entry.id.clone());
                        }
                        ui.add_space(2.0);
                    }
                });
        }
    } else if !app.control_apps_pending && app.control_apps_err.is_empty() && online {
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

    ui.label(
        RichText::new(t!(
            lang,
            "или вручную: .desktop-файл, команда, steam:// — всё запускается на компьютере",
            "or by hand: a .desktop file, a command, steam:// — all start on the computer"
        ))
        .size(11.0)
        .color(t.faint),
    );
    picked
}
fn gesture_kind_label(kind: GestureKind, lang: Lang) -> &'static str {
    match kind {
        GestureKind::Swipe => t!(lang, "свайп", "swipe"),
        GestureKind::Tap => t!(lang, "тап", "tap"),
        GestureKind::LongPress => t!(lang, "удержание", "long press"),
        GestureKind::PinchIn => t!(lang, "сжать", "pinch in"),
        GestureKind::PinchOut => t!(lang, "разжать", "pinch out"),
    }
}

fn swipe_arrow(dir: SwipeDir) -> &'static str {
    match dir {
        SwipeDir::Left => "←",
        SwipeDir::Right => "→",
        SwipeDir::Up => "↑",
        SwipeDir::Down => "↓",
    }
}

/// Shared little helper for pages: a key/value line.
pub fn kv(ui: &mut Ui, t: &Tokens, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(key)
                .family(FontFamily::Monospace)
                .size(11.5)
                .color(t.faint),
        );
        ui.add(egui::Label::new(RichText::new(value).size(12.5).color(t.muted)).wrap());
    });
}

/// A framed panel used by the pages for secondary content.
pub fn panel<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.well)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Sized spacer + hairline used between page sections.
pub fn divider(ui: &mut Ui, t: &Tokens) {
    ui.add_space(8.0);
    theme::separator(ui, t);
}

pub fn _touch(_: Vec2) {}

/// Read `name: value` lines into headers, ignoring blanks and comments.
fn parse_headers(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if !k.is_empty() {
                out.insert(k.to_string(), v.trim().to_string());
            }
        }
    }
    out
}
