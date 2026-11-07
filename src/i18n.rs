//! The interface's language. The code is written with the English wording, and French is looked up
//! from it, so anything not in the table (G-code, words in project files) can't change by accident.

use crate::fr::FR;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    English,
    French,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::French];

    /// Each language by its own name, whichever one the program is in.
    pub fn name(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::French => "Français",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::French => "fr",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code() == code)
    }
}

// Tests run side by side, so there each one keeps its own language.
#[cfg(test)]
thread_local! {
    static LANG: std::cell::Cell<Lang> = const { std::cell::Cell::new(Lang::English) };
}
#[cfg(test)]
pub fn language() -> Lang {
    LANG.with(|l| l.get())
}
#[cfg(test)]
pub fn set_language(lang: Lang) {
    LANG.with(|l| l.set(lang));
}

#[cfg(not(test))]
static FRENCH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(not(test))]
pub fn language() -> Lang {
    if FRENCH.load(std::sync::atomic::Ordering::Relaxed) { Lang::French } else { Lang::English }
}
#[cfg(not(test))]
pub fn set_language(lang: Lang) {
    FRENCH.store(lang == Lang::French, std::sync::atomic::Ordering::Relaxed);
}

fn french() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| FR.iter().copied().collect())
}

/// A fixed text in the current language.
pub fn tr(english: &'static str) -> &'static str {
    match language() {
        Lang::English => english,
        Lang::French => french().get(english).copied().unwrap_or(english),
    }
}

/// The parts of a `format!` text: words, and blanks such as `{}` or `{unit}`, in turn.
fn pieces(template: &str) -> (Vec<&str>, Vec<&str>) {
    let (mut words, mut blanks) = (Vec::new(), Vec::new());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        words.push(&rest[..open]);
        blanks.push(&rest[open..open + close + 1]);
        rest = &rest[open + close + 1..];
    }
    words.push(rest);
    (words, blanks)
}

/// Puts what was filled into an English `format!` text into the current language's wording of it.
pub fn fill(template: &'static str, formatted: String) -> String {
    let wording = tr(template);
    if wording == template {
        return formatted;
    }
    let (words, blanks) = pieces(template);
    let mut values = Vec::with_capacity(blanks.len());
    let Some(mut rest) = formatted.strip_prefix(words[0]) else { return formatted };
    for (i, blank) in blanks.iter().enumerate() {
        let next = words[i + 1];
        let end = if i + 2 == words.len() {
            if !rest.ends_with(next) {
                return formatted;
            }
            rest.len() - next.len()
        } else {
            match rest.find(next) {
                Some(end) => end,
                None => return formatted,
            }
        };
        values.push((*blank, &rest[..end]));
        rest = &rest[end + next.len()..];
    }
    let (out_words, out_blanks) = pieces(wording);
    let mut out = String::with_capacity(formatted.len() + 16);
    for (i, word) in out_words.iter().enumerate() {
        out.push_str(word);
        if let Some(blank) = out_blanks.get(i) {
            if let Some(at) = values.iter().position(|(b, _)| b == blank) {
                out.push_str(values.remove(at).1);
            }
        }
    }
    out
}

/// Like `format!`, in the current language.
#[macro_export]
macro_rules! tf {
    ($template:literal $($rest:tt)*) => {
        $crate::i18n::fill($template, format!($template $($rest)*))
    };
}

/// The language to start in when none has been chosen: the computer's, if there's a wording for it.
pub fn of_this_computer() -> Lang {
    if computer_is_french() { Lang::French } else { Lang::English }
}

#[cfg(windows)]
fn computer_is_french() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // The low ten bits are the language, and 0x0c is French wherever it's spoken.
    unsafe { GetUserDefaultUILanguage() & 0x3ff == 0x0c }
}

/// In a browser: the language the browser asks pages for.
#[cfg(target_arch = "wasm32")]
fn computer_is_french() -> bool {
    web_sys::window().and_then(|w| w.navigator().language()).is_some_and(|l| l.starts_with("fr"))
}

#[cfg(not(any(windows, target_arch = "wasm32")))]
fn computer_is_french() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .is_some_and(|value| value.starts_with("fr"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_french_wording_keeps_the_blanks_of_its_english_one() {
        for (english, french) in FR {
            let (mut a, mut b) = (pieces(english).1, pieces(french).1);
            a.sort();
            b.sort();
            assert_eq!(a, b, "{english}");
        }
    }

    #[test]
    fn no_text_has_two_wordings() {
        let mut seen = std::collections::HashSet::new();
        for (english, _) in FR {
            assert!(seen.insert(english), "{english} is there twice");
        }
    }

    #[test]
    fn what_was_filled_in_goes_to_the_right_blanks() {
        set_language(Lang::French);
        assert_eq!(fill("Could not read {}: {e}", "Could not read a.svg: gone".into()), "Impossible de lire a.svg : gone");
        assert_eq!(fill("Loaded {added}. These couldn't be read:\n{}", "Loaded 2. These couldn't be read:\nb.pp".into()), "2 chargé(s). Ceux-ci n'ont pas pu être lus :\nb.pp");
        // A named blank can change places.
        assert_eq!(fill("Moved the selection by {} {}. Shift: x10, Ctrl: x100, Alt: x0.1, Ctrl+Shift: fixed.", "Moved the selection by 1 mm. Shift: x10, Ctrl: x100, Alt: x0.1, Ctrl+Shift: fixed.".into()), "Sélection déplacée de 1 mm. Maj : x10, Ctrl : x100, Alt : x0,1, Ctrl+Maj : distance fixe.");
        assert_eq!(tr("Save"), "Enregistrer");
        assert_eq!(tr("Nothing like this is in the table"), "Nothing like this is in the table");
        set_language(Lang::English);
        assert_eq!(fill("Could not read {}: {e}", "Could not read a.svg: gone".into()), "Could not read a.svg: gone");
        assert_eq!(tr("Save"), "Save");
    }

    #[test]
    fn text_that_doesnt_fit_its_template_comes_through_as_it_is() {
        set_language(Lang::French);
        assert_eq!(fill("Could not read {}: {e}", "something else".into()), "something else");
    }
}
