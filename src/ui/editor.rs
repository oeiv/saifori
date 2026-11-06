use super::*;

pub(super) fn update_total_preview(
    unit: &gtk::Entry,
    qty: &gtk::SpinButton,
    label: &gtk::Label,
    currency: &str,
) {
    let display = match model::parse_minor(unit.text().as_str())
        .and_then(|price| model::total_minor(price, qty.value_as_int() as i64))
    {
        Ok(sum) => format!("{}: {}", i18n::tr("Total"), model::money(sum, currency)),
        Err(_) => format!("{}: —", i18n::tr("Total")),
    };
    label.set_text(&display);
}

pub(super) fn transaction_dialog(state: &Rc<State>, id: Option<i64>) {
    if state.privacy.get() {
        warning(
            state,
            "Privacy mode",
            "Turn off privacy mode before editing transactions.",
        );
        return;
    }
    if state.transaction_as_page.get() {
        state.editing_transaction.set(id);
        change_page(state, Page::TransactionEditor);
        return;
    }
    let popup = gtk::Window::builder()
        .title(if id.is_some() {
            "Edit Transaction"
        } else {
            "Add Transaction"
        })
        .transient_for(&state.window)
        .modal(true)
        .default_width(540)
        .default_height(640)
        .build();
    let form = transaction_form(state, id, Some(popup.clone()));
    popup.set_child(Some(&scroll(&form)));
    popup.present();
}

pub(super) fn transaction_editor_page(state: &Rc<State>, parent: &gtk::Box) {
    if state.privacy.get() {
        parent.append(&private_placeholder());
        return;
    }
    let title = if state.editing_transaction.get().is_some() {
        "Edit Transaction"
    } else {
        "Add Transaction"
    };
    page_title(parent, title, "");
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(700);
    let form = transaction_form(state, state.editing_transaction.get(), None);
    clamp.set_child(Some(&form));
    parent.append(&clamp);
}

pub(super) fn transaction_form(
    state: &Rc<State>,
    id: Option<i64>,
    popup: Option<gtk::Window>,
) -> gtk::Box {
    let existing = id.and_then(|id| state.db.borrow().transaction(id).ok().flatten());
    let initial = existing.as_ref().map(Draft::from);
    let body = gap(Orientation::Vertical, 15);
    margin(&body, 24);
    body.add_css_class("saifori-card");
    if popup.is_some() {
        body.append(&text(
            if existing.is_some() {
                "Edit Transaction"
            } else {
                "New Transaction"
            },
            "saifori-title",
        ));
    }
    let kind_options = ["expense", "income"];
    let kind = choice(
        &kind_options,
        initial
            .as_ref()
            .map(|d| d.kind.as_str())
            .unwrap_or("expense"),
    );
    body.append(&field("Transaction type", &kind));
    let description = gtk::Entry::new();
    description.set_placeholder_text(Some(&i18n::tr("What was it for?")));
    if let Some(d) = &initial {
        description.set_text(&d.description);
    }
    body.append(&field("Description", &description));
    let qty = gtk::SpinButton::with_range(1.0, 10_000.0, 1.0);
    qty.set_numeric(true);
    qty.set_value(initial.as_ref().map_or(1.0, |d| d.quantity as f64));
    body.append(&field("Quantity", &qty));
    let amount = gtk::Entry::new();
    amount.set_input_purpose(gtk::InputPurpose::Number);
    amount.set_placeholder_text(Some("0.00"));
    if let Some(d) = &initial {
        let unit = d.amount_minor / d.quantity.max(1);
        amount.set_text(&format!("{}.{:02}", unit / 100, unit % 100));
    }
    body.append(&field("Unit price", &amount));
    let cur = currency(state);
    let currency_control = choice(
        model::CURRENCIES,
        initial
            .as_ref()
            .map(|d| d.currency.as_str())
            .unwrap_or(&cur),
    );
    body.append(&field("Currency", &currency_control));
    let total_preview = text("Total: —", "saifori-section-title");
    update_total_preview(&amount, &qty, &total_preview, &cur);
    let a = amount.clone();
    let q = qty.clone();
    let t = total_preview.clone();
    let c = currency_control.clone();
    amount.connect_changed(move |_| {
        update_total_preview(&a, &q, &t, &selected(model::CURRENCIES, &c))
    });
    let a = amount.clone();
    let q = qty.clone();
    let t = total_preview.clone();
    let c = currency_control.clone();
    qty.connect_value_changed(move |_| {
        update_total_preview(&a, &q, &t, &selected(model::CURRENCIES, &c))
    });
    body.append(&total_preview);
    let day = gtk::Entry::new();
    day.set_text(
        initial
            .as_ref()
            .map(|d| d.day.as_str())
            .unwrap_or(&model::today()),
    );
    body.append(&field("Date · YYYY-MM-DD", &day));
    let category_names = category_options(state);
    let category = choice_owned(
        &category_names,
        initial
            .as_ref()
            .map(|d| d.category.as_str())
            .unwrap_or("Other"),
    );
    body.append(&field("Category", &category));
    let method = choice(
        model::METHODS,
        initial
            .as_ref()
            .map(|d| d.method.as_str())
            .unwrap_or("Cash"),
    );
    body.append(&field("Payment method", &method));
    let note = gtk::Entry::new();
    note.set_placeholder_text(Some(&i18n::tr("Optional note")));
    if let Some(d) = &initial {
        note.set_text(&d.note);
    }
    body.append(&field("Note", &note));
    let actions = gap(Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let st = state.clone();
    let close = popup.clone();
    cancel.connect_clicked(move |_| {
        if let Some(p) = &close {
            p.close();
        } else {
            change_page(&st, Page::Transactions);
        }
    });
    actions.append(&cancel);
    let save = button("Save Transaction", "document-save-symbolic");
    save.add_css_class("suggested-action");
    let st = state.clone();
    let close = popup.clone();
    save.connect_clicked(move |_| {
        let price = match model::parse_minor(amount.text().as_str()) {
            Ok(value) => value,
            Err(message) => {
                warning(&st, "Invalid unit price", &message);
                return;
            }
        };
        let quantity = qty.value_as_int() as i64;
        let total = match model::total_minor(price, quantity) {
            Ok(value) => value,
            Err(message) => {
                warning(&st, "Invalid total", &message);
                return;
            }
        };
        let draft = Draft {
            kind: selected(&kind_options, &kind),
            day: day.text().trim().to_owned(),
            amount_minor: total,
            quantity,
            currency: selected(model::CURRENCIES, &currency_control),
            category: selected_owned(&category_names, &category),
            description: description.text().trim().to_owned(),
            method: selected(model::METHODS, &method),
            note: note.text().to_string(),
        };
        if let Err(message) = draft.validate() {
            warning(&st, "Invalid transaction", &message);
            return;
        }
        let result = if let Some(id) = id {
            st.db.borrow_mut().update(id, &draft)
        } else {
            st.db.borrow_mut().add(&draft).map(|_| ())
        };
        match result {
            Ok(()) => {
                if let Some(p) = &close {
                    p.close();
                } else {
                    st.editing_transaction.set(None);
                    st.page.set(Page::Transactions);
                }
                refresh(&st);
            }
            Err(err) => warning(&st, "Could not save", &err.to_string()),
        }
    });
    actions.append(&save);
    body.append(&actions);
    body
}

pub(super) fn confirm_delete(state: &Rc<State>, id: i64) {
    let window = gtk::Window::builder()
        .title("Delete Transaction?")
        .transient_for(&state.window)
        .modal(true)
        .default_width(420)
        .resizable(false)
        .build();
    let body = gap(Orientation::Vertical, 17);
    margin(&body, 22);
    body.append(&text("Delete this transaction?", "saifori-section-title"));
    body.append(&text(
        "This action cannot be undone. Export a backup before deleting important records.",
        "saifori-muted",
    ));
    let row = gap(Orientation::Horizontal, 10);
    row.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let w = window.clone();
    cancel.connect_clicked(move |_| w.close());
    row.append(&cancel);
    let delete = button("Delete", "user-trash-symbolic");
    delete.add_css_class("destructive-action");
    let w = window.clone();
    let st = state.clone();
    delete.connect_clicked(move |_| {
        let result = st.db.borrow_mut().delete(id);
        if result.is_ok() {
            st.selected.set(None);
        }
        if run_write(&st, result) {
            w.close();
        }
    });
    row.append(&delete);
    body.append(&row);
    window.set_child(Some(&body));
    window.present();
}
