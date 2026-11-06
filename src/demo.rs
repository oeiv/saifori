//! Ephemeral USD-only demonstration ledger.
//! The real SQLite database is never populated with synthetic transactions.
//! All demo writes stay in a separate in-memory SQLite connection and vanish
//! when test mode ends or the process exits.
use crate::{
    db::Store,
    model::{self, Draft},
};
use anyhow::Result;
use chrono::{Datelike, NaiveDate};

// One consistent set of product names repeats across months, making the
// Products dashboard and duplicate-review feature useful in demonstration.
struct Example {
    day: u32,
    kind: &'static str,
    description: &'static str,
    category: &'static str,
    unit_cents: i64,
    quantity: i64,
    method: &'static str,
}

const EXAMPLES: &[Example] = &[
    Example {
        day: 1,
        kind: "income",
        description: "Monthly salary",
        category: "Salary",
        unit_cents: 465_000,
        quantity: 1,
        method: "Bank Transfer",
    },
    Example {
        day: 2,
        kind: "expense",
        description: "Apartment rent",
        category: "Housing",
        unit_cents: 120_000,
        quantity: 1,
        method: "Bank Transfer",
    },
    Example {
        day: 3,
        kind: "expense",
        description: "Groceries",
        category: "Groceries",
        unit_cents: 16_480,
        quantity: 2,
        method: "Card",
    },
    Example {
        day: 3,
        kind: "expense",
        description: "Coffee latte",
        category: "Food & Drink",
        unit_cents: 475,
        quantity: 2,
        method: "Card",
    },
    Example {
        day: 4,
        kind: "expense",
        description: "Metro pass",
        category: "Transport",
        unit_cents: 8_500,
        quantity: 1,
        method: "Card",
    },
    Example {
        day: 6,
        kind: "expense",
        description: "Internet service",
        category: "Utilities",
        unit_cents: 6_900,
        quantity: 1,
        method: "Bank Transfer",
    },
    Example {
        day: 8,
        kind: "expense",
        description: "Electricity bill",
        category: "Utilities",
        unit_cents: 11_400,
        quantity: 1,
        method: "Bank Transfer",
    },
    Example {
        day: 10,
        kind: "expense",
        description: "Groceries",
        category: "Groceries",
        unit_cents: 12_900,
        quantity: 2,
        method: "Card",
    },
    Example {
        day: 12,
        kind: "expense",
        description: "Restaurant dinner",
        category: "Food & Drink",
        unit_cents: 7_300,
        quantity: 1,
        method: "Card",
    },
    Example {
        day: 14,
        kind: "expense",
        description: "Pet food",
        category: "Pets",
        unit_cents: 2_750,
        quantity: 2,
        method: "Cash",
    },
    Example {
        day: 16,
        kind: "income",
        description: "Freelance project",
        category: "Freelance",
        unit_cents: 78_000,
        quantity: 1,
        method: "Bank Transfer",
    },
    Example {
        day: 18,
        kind: "expense",
        description: "Gym membership",
        category: "Health",
        unit_cents: 4_500,
        quantity: 1,
        method: "Card",
    },
    Example {
        day: 20,
        kind: "expense",
        description: "Books",
        category: "Education",
        unit_cents: 2_900,
        quantity: 2,
        method: "Card",
    },
    Example {
        day: 22,
        kind: "expense",
        description: "Software subscription",
        category: "Subscriptions",
        unit_cents: 1_900,
        quantity: 1,
        method: "Card",
    },
    Example {
        day: 24,
        kind: "expense",
        description: "Fuel",
        category: "Transport",
        unit_cents: 5_700,
        quantity: 1,
        method: "Card",
    },
    Example {
        day: 26,
        kind: "expense",
        description: "Cinema tickets",
        category: "Entertainment",
        unit_cents: 1_350,
        quantity: 2,
        method: "Card",
    },
    Example {
        day: 28,
        kind: "expense",
        description: "Household supplies",
        category: "Shopping",
        unit_cents: 4_200,
        quantity: 1,
        method: "Cash",
    },
];

pub(crate) fn build(reference: NaiveDate) -> Result<Store> {
    let mut db = Store::memory()?;
    db.set_currency("USD")?;
    let current = format!("{:04}-{:02}", reference.year(), reference.month());
    // Twelve rolling months, including the current month, with no future-dated records.
    for offset in -11..=0 {
        let month = model::month_offset(&current, offset).expect("valid Gregorian month");
        for (index, example) in EXAMPLES.iter().enumerate() {
            if offset == 0 && example.day > reference.day() {
                continue;
            }
            let date = format!("{}-{:02}", month, example.day);
            // Variable but deterministic amounts give the charts real trends.
            let adjustment = if example.kind == "income" {
                (offset + 11) as i64 * 2_500
            } else {
                ((index as i64 * 7 + (offset + 11) as i64 * 3) % 9) * 35
            };
            let unit_cents = example.unit_cents + adjustment;
            let draft = Draft {
                day: date,
                kind: example.kind.into(),
                amount_minor: unit_cents * example.quantity,
                quantity: example.quantity,
                currency: "USD".into(),
                category: example.category.into(),
                description: example.description.into(),
                method: example.method.into(),
                note: "Sample data · Test Mode".into(),
            };
            db.add(&draft)?;
        }
        // Dashboard budget bars also use demo-only data.
        db.set_budget("USD", &month, "Groceries", 48_000)?;
        db.set_budget("USD", &month, "Food & Drink", 25_000)?;
        db.set_budget("USD", &month, "Shopping", 20_000)?;
        if offset < 0 {
            db.add_transfer(
                &format!("{}-19", month),
                15_000,
                "USD",
                "Checking",
                "Savings",
                "Monthly allocation",
            )?;
        }
    }
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_ledger_is_usd_and_spans_multiple_months() -> Result<()> {
        let reference = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        let demo = build(reference)?;
        assert_eq!(demo.currency()?, "USD");
        assert!(demo.summary("USD", "2026-11")?.expense > 0);
        assert!(demo.summary("USD", "2026-01")?.expense > 0);
        assert!(demo.year_summary("USD", 2026)?.expense > 0);
        assert_eq!(demo.summary("MAD", "2026-11")?.count, 0);
        assert!(!demo.product_totals("USD", None, 100)?.is_empty());
        assert!(!demo.transfers("USD", 100)?.is_empty());
        Ok(())
    }
    #[test]
    fn data_is_fresh_on_each_activation() -> Result<()> {
        let reference = NaiveDate::from_ymd_opt(2026, 11, 5).unwrap();
        let mut first = build(reference)?;
        let initial = first.summary("USD", "2026-11")?;
        first.add(&Draft {
            day: "2026-11-04".into(),
            kind: "expense".into(),
            amount_minor: 50_000,
            quantity: 1,
            currency: "USD".into(),
            category: "Other".into(),
            description: "Temporary test purchase".into(),
            method: "Cash".into(),
            note: String::new(),
        })?;
        assert_eq!(
            first.summary("USD", "2026-11")?.expense,
            initial.expense + 50_000
        );
        let second = build(reference)?;
        assert_eq!(second.summary("USD", "2026-11")?.expense, initial.expense);
        Ok(())
    }
    #[test]
    fn test_mode_setting_accepts_only_boolean_values() -> Result<()> {
        let mut db = Store::memory()?;
        db.set_setting("test_mode", "1")?;
        assert_eq!(db.setting("test_mode")?.as_deref(), Some("1"));
        assert!(db.set_setting("test_mode", "maybe").is_err());
        db.set_setting("test_mode", "0")?;
        assert_eq!(db.setting("test_mode")?.as_deref(), Some("0"));
        Ok(())
    }
}
