//! Tiny ja/en catalog. Locales are JSON files embedded at compile time so a
//! clean checkout has every string (no runtime file dependency).

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    Ja,
    En,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Locale::Ja => "ja",
            Locale::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "ja" => Some(Locale::Ja),
            "en" => Some(Locale::En),
            _ => None,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Locale::Ja => Locale::En,
            Locale::En => Locale::Ja,
        }
    }

    /// UI label for the language toggle ("ja" / "en").
    pub fn label(self) -> &'static str {
        self.code()
    }
}

const JA: &str = include_str!("../../assets/locales/ja.json");
const EN: &str = include_str!("../../assets/locales/en.json");

pub struct I18n {
    locale: Locale,
    ja: Value,
    en: Value,
}

impl I18n {
    pub fn new(locale: Locale) -> Self {
        Self {
            locale,
            ja: serde_json::from_str(JA).expect("ja.json is valid"),
            en: serde_json::from_str(EN).expect("en.json is valid"),
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Translate a dotted key. Missing keys fall back to the other language,
    /// then to the key itself (so the UI never shows an empty label).
    pub fn t(&self, key: &str) -> String {
        let (primary, fallback) = match self.locale {
            Locale::Ja => (&self.ja, &self.en),
            Locale::En => (&self.en, &self.ja),
        };
        lookup(primary, key)
            .or_else(|| lookup(fallback, key))
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.to_string())
    }

    /// `{{name}}` interpolation, e.g. "Reconnecting {{n}}/{{max}}".
    pub fn tf(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut text = self.t(key);
        for (name, value) in args {
            text = text.replace(&format!("{{{{{name}}}}}"), value);
        }
        text
    }
}

fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a str> {
    let mut node = root;
    for part in key.split('.') {
        node = node.get(part)?;
    }
    node.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten(value: &Value, prefix: &str, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    flatten(v, &path, out);
                }
            }
            Value::String(_) => out.push(prefix.to_string()),
            _ => {}
        }
    }

    fn keys(json: &str) -> Vec<String> {
        let mut out = Vec::new();
        flatten(&serde_json::from_str(json).unwrap(), "", &mut out);
        out.sort();
        out
    }

    #[test]
    fn catalogs_have_identical_keys() {
        assert_eq!(keys(JA), keys(EN), "ja/en locale keys must match");
    }

    #[test]
    fn catalogs_contain_no_emoji() {
        // User requirement: no emoji in the UI. Guard the catalogs against
        // pictographs / dingbats / variation selectors sneaking in.
        for source in [JA, EN] {
            for value in collect_strings(&serde_json::from_str::<Value>(source).unwrap()) {
                for ch in value.chars() {
                    let c = ch as u32;
                    let emoji = (0x1F000..=0x1FAFF).contains(&c)
                        || (0x2600..=0x27BF).contains(&c)
                        || c == 0xFE0F
                        || c == 0x2B50;
                    assert!(!emoji, "emoji U+{c:04X} found in {source}");
                }
            }
        }
    }

    fn collect_strings(value: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match value {
            Value::Object(map) => {
                for v in map.values() {
                    out.extend(collect_strings(v));
                }
            }
            Value::String(s) => out.push(s.clone()),
            _ => {}
        }
        out
    }

    #[test]
    fn translates_and_interpolates() {
        let i18n = I18n::new(Locale::Ja);
        assert_eq!(i18n.t("steps.screen"), "画面");
        let text = i18n.tf("stream.retrying", &[("n", "2")]);
        assert_eq!(text, "再接続中 2/3");
        let en = I18n::new(Locale::En);
        assert_eq!(en.t("steps.screen"), "Screen");
        assert_eq!(en.t("missing.key"), "missing.key");
    }
}
