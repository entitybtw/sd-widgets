//! Three built-in languages plus any user pack. UI strings are written next to
//! their translations: `t!(lang, "Обзор", "Overview")` keeps Russian and English
//! in sync and avoids a dictionary of missing keys. Ukrainian lives in the
//! [`uk`] table, keyed by the Russian text; a pack in the `lang` folder of the
//! config directory can add any other language (see [`packs`]).

mod packs;
mod uk;

pub use packs::{
    active as active_pack, dir as pack_dir, errors as pack_errors, is_pack, list as pack_list,
    load as load_packs, set_active as set_pack, template as pack_template,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Ru,
    En,
    Uk,
    /// A user pack is active; its text comes from [`packs`].
    Custom,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Ru, Lang::En, Lang::Uk];

    pub fn code(self) -> &'static str {
        match self {
            Self::Ru => "ru",
            Self::En => "en",
            Self::Uk => "uk",
            Self::Custom => "custom",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Ru => "Русский",
            Self::En => "English",
            Self::Uk => "Українська",
            Self::Custom => "Custom",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Self::Ru => "RU",
            Self::En => "EN",
            Self::Uk => "UK",
            Self::Custom => "--",
        }
    }
    /// The built-in language for a stored code. A code that matches an installed
    /// pack gives [`Lang::Custom`]; packs must be loaded first (see
    /// [`packs::load`]).
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" | "eng" | "english" => Self::En,
            "uk" | "ua" | "ukr" | "ukrainian" | "українська" => Self::Uk,
            other => {
                if !other.is_empty() && is_pack(other) {
                    Self::Custom
                } else {
                    Self::Ru
                }
            }
        }
    }
    /// The language a plain click cycles to.
    pub fn toggle(self) -> Self {
        match self {
            Self::Ru => Self::En,
            Self::En => Self::Uk,
            Self::Uk | Self::Custom => Self::Ru,
        }
    }
    pub fn is_ukrainian(self) -> bool {
        self == Self::Uk
    }
}

/// The text in the given language: Russian as written, English as written, and
/// for Ukrainian the row of the [`uk`] table; a pair without a Ukrainian row
/// stays Russian until the table gets one.
pub fn tr<'a>(lang: Lang, ru: &'a str, en: &'a str) -> &'a str {
    // An active pack wins over the built-in table: the user picked it on
    // purpose, and a line it does not translate falls back to English below.
    if let Some(text) = packs::lookup(ru, en) {
        return text;
    }
    match lang {
        Lang::Ru => ru,
        Lang::En | Lang::Custom => en,
        Lang::Uk => uk::lookup(ru).unwrap_or(ru),
    }
}

#[macro_export]
macro_rules! t {
    ($lang:expr, $ru:expr, $en:expr) => {
        $crate::i18n::tr($lang, $ru, $en)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_string_written_in_a_call_has_a_ukrainian_row() {
        // The keys are the arguments of `t!` across the workspace; the list is
        // collected by reading the sources, so a new call without a Ukrainian
        // row fails here instead of quietly showing Russian on the Deck.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut missing: Vec<String> = Vec::new();
        for dir in ["sdw-core", "sdw-deck", "sdw-daemon"] {
            let dir = root.join("crates").join(dir).join("src");
            collect(&dir, &mut missing);
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "no Ukrainian row for:\n{}",
            missing.join("\n")
        );
    }

    fn collect(dir: &std::path::Path, missing: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, missing);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let src = std::fs::read_to_string(&path).unwrap_or_default();
                scan(&strip_comments(&src), missing);
            }
        }
    }

    /// Comments hold examples of the macro (`//! t!(lang, "Обзор", …)`), so
    /// they are blanked before the scan.
    fn strip_comments(src: &str) -> String {
        let chars: Vec<char> = src.chars().collect();
        let mut out = String::with_capacity(src.len());
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '"' {
                out.push(c);
                i += 1;
                while i < chars.len() {
                    out.push(chars[i]);
                    if chars[i] == '\\' {
                        i += 1;
                        if i < chars.len() {
                            out.push(chars[i]);
                        }
                    } else if chars[i] == '"' {
                        break;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            if c == '/' && chars.get(i + 1) == Some(&'/') {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if c == '/' && chars.get(i + 1) == Some(&'*') {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// Walk the source, find every `t!(` and take its second argument when it
    /// is a plain string literal.
    fn scan(src: &str, missing: &mut Vec<String>) {
        let bytes: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let here =
                bytes[i] == 't' && bytes[i + 1] == '!' && (i == 0 || !is_ident(bytes[i - 1]));
            if !here {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_whitespace() {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != '(' {
                i += 1;
                continue;
            }
            let mut args: Vec<String> = Vec::new();
            let mut cur = String::new();
            let mut depth = 0usize;
            // `bytes[j]` is the opening parenthesis itself.
            let mut k = j + 1;
            while k < bytes.len() {
                let c = bytes[k];
                if c == '"' {
                    let mut lit = String::from(c);
                    k += 1;
                    while k < bytes.len() {
                        lit.push(bytes[k]);
                        if bytes[k] == '\\' {
                            k += 1;
                            if k < bytes.len() {
                                lit.push(bytes[k]);
                            }
                        } else if bytes[k] == '"' {
                            break;
                        }
                        k += 1;
                    }
                    cur.push_str(&lit);
                } else if c == '(' || c == '[' || c == '{' {
                    depth += 1;
                    cur.push(c);
                } else if c == ')' || c == ']' || c == '}' {
                    if depth == 0 {
                        args.push(cur.clone());
                        break;
                    }
                    depth -= 1;
                    cur.push(c);
                } else if c == ',' && depth == 0 {
                    args.push(cur.clone());
                    cur.clear();
                } else {
                    cur.push(c);
                }
                k += 1;
            }
            // The language itself can be the first argument of a nested call,
            // so only the pair counts as a label.
            if args.len() != 3 {
                i = k.max(i + 1);
                continue;
            }
            if let Some(ru) = args.get(1) {
                let ru = ru.trim();
                if ru.starts_with('"') && ru.ends_with('"') && ru.matches('"').count() == 2 {
                    let key = unquote(ru);
                    if uk::lookup(&key).is_none() {
                        missing.push(key);
                    }
                }
            }
            i = k.max(i + 1);
        }
    }

    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn unquote(lit: &str) -> String {
        let mut out = String::new();
        let mut chars = lit[1..lit.len() - 1].chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.clone().next() {
                // `\` at the end of a line joins the next one: the newline and
                // the indentation after it disappear.
                Some('\n') => {
                    chars.next();
                    while chars.clone().next().is_some_and(|c| c == ' ' || c == '\t') {
                        chars.next();
                    }
                }
                _ => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => out.push(other),
                    None => {}
                },
            }
        }
        out
    }
}
