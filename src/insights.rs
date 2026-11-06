use crate::model::{CategoryTotal, Summary};

pub fn biggest_insight(summary: &Summary, cats: &[CategoryTotal], currency: &str) -> String {
    if summary.count == 0 {
        return crate::i18n::tr("Add your first transaction to reveal useful trends.");
    }
    if let Some(top) = cats.first() {
        return crate::i18n::fmt(
            "{category} is your largest expense category this month: {amount}.",
            &[
                ("category", &crate::i18n::tr(&top.category)),
                ("amount", &crate::model::money(top.amount_minor, currency)),
            ],
        );
    }
    crate::i18n::tr("No expenses recorded in this month.")
}
