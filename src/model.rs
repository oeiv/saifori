use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.oeiv.Saifori";
pub const CURRENCIES: &[&str] = &["MAD", "USD", "EUR", "GBP", "CAD", "AUD", "CHF"];
pub const CATEGORIES: &[&str] = &[
    "Food & Drink",
    "Groceries",
    "Housing",
    "Transport",
    "Shopping",
    "Utilities",
    "Health",
    "Education",
    "Entertainment",
    "Subscriptions",
    "Business",
    "Pets",
    "Salary",
    "Freelance",
    "Savings",
    "Other",
];
pub const METHODS: &[&str] = &["Cash", "Card", "Bank Transfer", "Other"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub day: String,
    pub kind: String,
    pub amount_minor: i64,
    #[serde(default = "default_quantity")]
    pub quantity: i64,
    pub currency: String,
    pub category: String,
    pub description: String,
    pub method: String,
    pub note: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub day: String,
    pub kind: String,
    pub amount_minor: i64,
    #[serde(default = "default_quantity")]
    pub quantity: i64,
    pub currency: String,
    pub category: String,
    pub description: String,
    pub method: String,
    pub note: String,
}

pub fn default_quantity() -> i64 {
    1
}

/// Exact integer-cent multiplication; never silently rounds a unit price.
pub fn total_minor(unit_minor: i64, quantity: i64) -> Result<i64, String> {
    if !(1..=10_000).contains(&quantity) {
        return Err("Quantity must be between 1 and 10,000.".into());
    }
    let total = unit_minor
        .checked_mul(quantity)
        .ok_or("Amount is too large.")?;
    if !(1..=99_999_999_999).contains(&total) {
        return Err("Total amount is out of range.".into());
    }
    Ok(total)
}

impl Draft {
    pub fn validate(&self) -> Result<(), String> {
        NaiveDate::parse_from_str(&self.day, "%Y-%m-%d")
            .map_err(|_| "Enter a valid date (YYYY-MM-DD).".to_string())?;
        if !["income", "expense"].contains(&self.kind.as_str()) {
            return Err("Choose income or expense.".into());
        }
        if !(1..=10_000).contains(&self.quantity) {
            return Err("Quantity must be between 1 and 10,000.".into());
        }
        if self.amount_minor % self.quantity != 0 {
            return Err("Total must equal a whole-cent unit price times quantity.".into());
        }
        if self.amount_minor <= 0 || self.amount_minor > 99_999_999_999 {
            return Err("Amount must be positive and below 1 billion.".into());
        }
        if !CURRENCIES.contains(&self.currency.as_str()) {
            return Err("Unsupported currency.".into());
        }
        if self.category.trim().is_empty()
            || self.category.chars().count() > 60
            || self.category.chars().any(char::is_control)
        {
            return Err("Category must be between 1 and 60 characters.".into());
        }
        if self.description.trim().is_empty() || self.description.chars().count() > 240 {
            return Err("Description must contain 1–240 characters.".into());
        }
        if self.note.chars().count() > 2000 {
            return Err("Note is too long.".into());
        }
        if !METHODS.contains(&self.method.as_str()) {
            return Err("Choose a valid payment method.".into());
        }
        Ok(())
    }
}

impl From<&Transaction> for Draft {
    fn from(tx: &Transaction) -> Self {
        Self {
            day: tx.day.clone(),
            kind: tx.kind.clone(),
            amount_minor: tx.amount_minor,
            quantity: tx.quantity,
            currency: tx.currency.clone(),
            category: tx.category.clone(),
            description: tx.description.clone(),
            method: tx.method.clone(),
            note: tx.note.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub id: i64,
    pub day: String,
    pub amount_minor: i64,
    pub currency: String,
    pub source: String,
    pub destination: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: i64,
    pub entity: String,
    pub entity_id: i64,
    pub action: String,
    pub label: String,
    pub amount_minor: i64,
    pub currency: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub income: i64,
    pub expense: i64,
    pub count: i64,
}

impl Summary {
    pub fn balance(&self) -> i64 {
        self.income - self.expense
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthTotal {
    pub month: String,
    pub income: i64,
    pub expense: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub amount_minor: i64,
}

pub fn today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}
pub fn current_month() -> String {
    Local::now().format("%Y-%m").to_string()
}

pub fn month_offset(month: &str, offset: i32) -> Option<String> {
    let date = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d").ok()?;
    let value = date.year() * 12 + date.month0() as i32 + offset;
    let year = value.div_euclid(12);
    let month = value.rem_euclid(12) + 1;
    Some(format!("{year:04}-{month:02}"))
}

// Money is always stored as integer minor units. Reject scientific notation, nonfinite,
// ambiguous thousands separators, or extra decimal digits instead of rounding silently.
pub fn parse_minor(text: &str) -> Result<i64, String> {
    let normalized = text.trim().replace(',', ".");
    let mut parts = normalized.split('.');
    let integer = parts.next().unwrap_or("");
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 2
    {
        return Err("Use a positive amount, for example 25.50.".into());
    }
    let units = integer.parse::<i64>().map_err(|_| "Amount is too large.")?;
    let cents = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().unwrap_or(0) * 10,
        _ => fraction.parse::<i64>().unwrap_or(0),
    };
    let amount = units
        .checked_mul(100)
        .and_then(|n| n.checked_add(cents))
        .ok_or("Amount is too large.")?;
    if amount <= 0 || amount > 99_999_999_999 {
        return Err("Amount is out of range.".into());
    }
    Ok(amount)
}

pub fn money(amount: i64, currency: &str) -> String {
    let sign = if amount < 0 { "−" } else { "" };
    let positive = (amount as i128).abs();
    let major = positive / 100;
    let cents = positive % 100;
    let s = major.to_string();
    let mut formatted = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            formatted.push(if crate::i18n::language() == "es" {
                '.'
            } else {
                ','
            });
        }
        formatted.push(c);
    }
    let decimal = if crate::i18n::language() == "es" {
        ","
    } else {
        "."
    };
    format!("{sign}{formatted}{decimal}{cents:02} {currency}")
}

// Standard Adwaita icons plus a few bundled financial symbols drawn in the
// GNOME symbolic 16px style. The application registers their icon theme at startup.
pub fn category_icon(name: &str) -> &'static str {
    match name {
        "Food & Drink" => "saifori-cup-symbolic",
        "Groceries" | "Shopping" => "saifori-cart-symbolic",
        "Housing" => "go-home-symbolic",
        "Transport" => "saifori-car-symbolic",
        "Health" | "Pets" => "emote-love-symbolic",
        "Education" => "accessories-dictionary-symbolic",
        "Entertainment" => "applications-multimedia-symbolic",
        "Subscriptions" => "view-refresh-symbolic",
        "Business" | "Freelance" | "Salary" => "saifori-briefcase-symbolic",
        "Savings" => "saifori-budget-symbolic",
        "Utilities" => "preferences-system-symbolic",
        _ => "view-more-horizontal-symbolic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_minor_amounts() {
        assert_eq!(parse_minor("10.05").unwrap(), 1005);
        assert_eq!(parse_minor("0,5").unwrap(), 50);
        assert!(parse_minor("1e6").is_err());
        assert!(parse_minor("-1").is_err());
        assert!(parse_minor("12.999").is_err());
        assert!(parse_minor("1,000.00").is_err());
        assert!(parse_minor("NaN").is_err());
    }
    #[test]
    fn month_rollover() {
        assert_eq!(month_offset("2026-01", -1).unwrap(), "2025-12");
        assert_eq!(month_offset("2026-12", 1).unwrap(), "2027-01");
    }
    #[test]
    fn integer_quantity_is_exact() {
        assert_eq!(total_minor(1250, 3).unwrap(), 3750);
        assert!(total_minor(i64::MAX, 2).is_err());
        assert!(total_minor(500, 0).is_err());
    }
    #[test]
    fn format_money() {
        assert_eq!(money(1234567, "MAD"), "12,345.67 MAD");
        assert_eq!(money(-25, "USD"), "−0.25 USD");
    }
}

#[cfg(test)]
mod locale_tests {
    #[test]
    fn spanish_money_separators() {
        crate::i18n::set_language("es");
        assert_eq!(super::money(1234567, "EUR"), "12.345,67 EUR");
        assert_eq!(super::money(-25, "EUR"), "−0,25 EUR");
        crate::i18n::set_language("en");
    }
}
