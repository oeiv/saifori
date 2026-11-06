//! Small, file-based UI catalog. Canonical database values stay in English.
//! Supported UI languages: English and Spanish.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("es", "Español")];
thread_local! { static CURRENT: RefCell<String> = RefCell::new("en".into()); }
static SPANISH: OnceLock<HashMap<String, String>> = OnceLock::new();

pub fn system_language() -> String {
    select_system_language(gtk::glib::language_names().iter().map(|v| v.as_str()))
}
fn select_system_language<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    for name in names {
        let base = name.split(['_', '-', '.', '@']).next().unwrap_or("en");
        if valid(base) {
            return base.to_owned();
        }
    }
    "en".into()
}

pub fn valid(lang: &str) -> bool {
    LANGUAGES.iter().any(|(code, _)| *code == lang)
}
pub fn set_language(lang: &str) {
    CURRENT.with(|v| *v.borrow_mut() = if valid(lang) { lang } else { "en" }.to_string());
}
pub fn language() -> String {
    CURRENT.with(|v| v.borrow().clone())
}
fn catalog() -> &'static HashMap<String, String> {
    SPANISH.get_or_init(|| {
        serde_json::from_str(include_str!("../locales/es.json"))
            .expect("built-in Spanish translations must be valid JSON")
    })
}
pub fn tr(key: &str) -> String {
    if language() == "es" {
        catalog()
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    } else {
        key.to_owned()
    }
}
pub fn fmt(key: &str, fields: &[(&str, &str)]) -> String {
    let mut result = tr(key);
    for (field, value) in fields {
        result = result.replace(&format!("{{{field}}}"), value);
    }
    result
}
pub fn month_name(index: usize, abbreviated: bool) -> &'static str {
    const EN: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const ES: [&str; 12] = [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ];
    const EN_SHORT: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const ES_SHORT: [&str; 12] = [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic",
    ];
    if index == 0 || index > 12 {
        return "";
    }
    let i = index - 1;
    match (language().as_str(), abbreviated) {
        ("es", true) => ES_SHORT[i],
        ("es", false) => ES[i],
        (_, true) => EN_SHORT[i],
        _ => EN[i],
    }
}
pub fn display_month(raw: &str) -> String {
    let (Some(year), Some(month)) = (raw.get(..4), raw.get(5..7)) else {
        return raw.to_string();
    };
    let Ok(index) = month.parse::<usize>() else {
        return raw.to_string();
    };
    let label = month_name(index, false);
    if label.is_empty() {
        raw.to_string()
    } else {
        format!("{label} {year}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_language_preferences() {
        assert_eq!(select_system_language(["es_ES.UTF-8", "en"]), "es");
        assert_eq!(select_system_language(["fr_FR", "en_GB"]), "en");
        assert_eq!(select_system_language(["ar_MA"]), "en");
    }
    #[test]
    fn languages_are_english_and_spanish_only() {
        assert_eq!(LANGUAGES.len(), 2);
        assert!(!catalog().is_empty());
        assert_eq!(
            catalog().get("Overview").map(String::as_str),
            Some("Resumen")
        );
        set_language("es");
        assert_eq!(language(), "es");
        set_language("ar");
        assert_eq!(language(), "en"); // historic saved preference
        set_language("xx");
        assert_eq!(language(), "en");
    }
}
