use super::*;

pub(super) fn budgets(state: &Rc<State>, parent: &gtk::Box) {
    let heading = gap(Orientation::Horizontal, 20);
    let title = gap(Orientation::Vertical, 4);
    title.set_hexpand(true);
    title.append(&text("Budgets", "saifori-title"));
    title.append(&text("Monthly spending limits", "saifori-subtitle"));
    heading.append(&title);
    heading.append(&month_navigation(state));
    parent.append(&heading);
    let cur = currency(state);
    let m = month(state);
    let budget_rows = match state.db.borrow().budgets(&cur, &m) {
        Ok(x) => x,
        Err(err) => {
            parent.append(&text(
                &format!("Could not load budgets: {err}"),
                "saifori-negative",
            ));
            return;
        }
    };
    let Some(costs) = checked(parent, state.db.borrow().categories(&cur, &m)) else {
        return;
    };
    let total_budget: i64 = budget_rows.iter().map(|x| x.1).sum();
    let spending: i64 = costs.iter().map(|x| x.amount_minor).sum();
    let kpis = gap(Orientation::Horizontal, 14);
    kpis.set_homogeneous(true);
    kpis.append(&metric(
        "Monthly Budget",
        masked_money(state, total_budget, &cur),
        "",
        "folder-symbolic",
    ));
    kpis.append(&metric(
        "Spent",
        masked_money(state, spending, &cur),
        "",
        "go-down-symbolic",
    ));
    kpis.append(&metric(
        "Budget Difference",
        masked_money(state, total_budget - spending, &cur),
        "",
        "accessories-calculator-symbolic",
    ));
    parent.append(&kpis);
    let form = card();
    form.append(&section_heading(
        "Set a Category Limit",
        "Amounts are stored in the selected currency and month",
    ));
    let form_row = gap(Orientation::Horizontal, 12);
    let category_names = category_options(state);
    let category = choice_owned(&category_names, "Groceries");
    category.set_hexpand(true);
    form_row.append(&category);
    let amount = gtk::Entry::new();
    amount.set_input_purpose(gtk::InputPurpose::Number);
    amount.set_placeholder_text(Some("Monthly limit, e.g. 500.00"));
    amount.set_hexpand(true);
    form_row.append(&amount);
    let save = button("Set Limit", "document-save-symbolic");
    save.add_css_class("suggested-action");
    let st = state.clone();
    save.connect_clicked(move |_| {
        let v = if amount.text().trim() == "0" {
            Ok(0)
        } else {
            model::parse_minor(amount.text().as_str())
        };
        match v {
            Ok(v) => {
                let res = st.db.borrow_mut().set_budget(
                    &currency(&st),
                    &month(&st),
                    &selected_owned(&category_names, &category),
                    v,
                );
                run_write(&st, res);
            }
            Err(err) => warning(&st, "Invalid budget amount", &err),
        }
    });
    form_row.append(&save);
    form.append(&form_row);
    parent.append(&form);
    let list = card();
    list.append(&section_heading(
        "Category Budgets",
        "Color reflects how much of each limit has been used",
    ));
    if budget_rows.is_empty() {
        list.append(&text(
            "No budgets configured for this month.",
            "saifori-muted",
        ));
    }
    for (cat, limit) in budget_rows {
        let spent = costs
            .iter()
            .find(|c| c.category == cat)
            .map(|c| c.amount_minor)
            .unwrap_or(0);
        let block = gap(Orientation::Vertical, 7);
        let row = gap(Orientation::Horizontal, 10);
        let name = text(&masked(state, &cat), "");
        name.set_hexpand(true);
        row.append(&name);
        row.append(&text(
            &format!(
                "{} / {}",
                masked_money(state, spent, &cur),
                masked_money(state, limit, &cur)
            ),
            "saifori-muted",
        ));
        block.append(&row);
        if state.privacy.get() {
            list.append(&block);
            list.append(&line());
            continue;
        }
        let bar = gtk::ProgressBar::new();
        let fraction = if limit == 0 {
            0.0
        } else {
            (spent as f64 / limit as f64).min(1.0)
        };
        bar.set_fraction(fraction);
        if limit > 0 && spent > limit {
            bar.add_css_class("error");
        }
        block.append(&bar);
        list.append(&block);
        list.append(&line());
    }
    parent.append(&list);
}

pub(super) fn analytics(state: &Rc<State>, parent: &gtk::Box) {
    let header = gap(Orientation::Horizontal, 20);
    let title = gap(Orientation::Vertical, 5);
    title.set_hexpand(true);
    title.append(&text("Analytics", "saifori-title"));
    title.append(&text("Trends and comparisons", "saifori-subtitle"));
    header.append(&title);
    header.append(&month_navigation(state));
    parent.append(&header);
    let cur = currency(state);
    let m = month(state);
    let Some(months) = checked(parent, state.db.borrow().months(&cur, &m, 12)) else {
        return;
    };
    let year = state.annual_year.get();
    let Some(annual) = checked(parent, state.db.borrow().year_summary(&cur, year)) else {
        return;
    };
    parent.append(&annual_summary_widget(state, year, &cur, &annual));
    let chart = card();
    chart.append(&section_heading(
        "Yearly Cash Flow",
        "Last 12 months · green income, blue spending",
    ));
    if state.privacy.get() {
        chart.append(&private_placeholder());
    } else {
        let drawing = charts::line_chart(months.clone());
        drawing.set_content_height(350);
        chart.append(&drawing);
    }
    parent.append(&chart);
    let table = card();
    table.append(&section_heading(
        "Monthly Comparison",
        "Amounts in the selected currency",
    ));
    for item in months.iter().rev() {
        let row = gap(Orientation::Horizontal, 16);
        row.add_css_class("saifori-list-row");
        let month = text(&item.month, "saifori-section-title");
        month.set_width_chars(12);
        row.append(&month);
        let income = text(&masked_money(state, item.income, &cur), "saifori-positive");
        income.set_hexpand(true);
        row.append(&income);
        let expenses = text(&masked_money(state, item.expense, &cur), "saifori-negative");
        expenses.set_hexpand(true);
        row.append(&expenses);
        let net = text(
            &masked_money(state, item.income - item.expense, &cur),
            "saifori-amount",
        );
        row.append(&net);
        table.append(&row);
    }
    parent.append(&table);
}

pub(super) fn products_page(state: &Rc<State>, parent: &gtk::Box) {
    page_title(parent, "Products", "");
    let cur = currency(state);
    let filters = gap(Orientation::Horizontal, 12);
    let options = dropdown(&["This month", "All time"]);
    options.set_selected(state.product_period.get());
    let results = gap(Orientation::Vertical, 4);
    results.set_hexpand(true);
    filters.append(&text("Period", "saifori-muted"));
    filters.append(&options);
    parent.append(&filters);
    let list_card = card();
    list_card.set_halign(gtk::Align::Fill);
    list_card.append(&section_heading("Most purchased products", ""));
    list_card.append(&results);
    parent.append(&list_card);
    let st = state.clone();
    let area = results.clone();
    let picker = options.clone();
    let update = move || {
        while let Some(child) = area.first_child() {
            area.remove(&child);
        }
        let m = if picker.selected() == 0 {
            Some(month(&st))
        } else {
            None
        };
        let data = st
            .db
            .borrow()
            .product_totals(&currency(&st), m.as_deref(), 100);
        match data {
            Ok(products) if products.is_empty() => {
                area.append(&text("No purchases in this period.", "saifori-empty"))
            }
            Ok(products) => {
                for (name, orders, quantity, total, last) in products {
                    let row = gap(Orientation::Horizontal, 16);
                    row.add_css_class("saifori-list-row");
                    row.append(&icon("saifori-cart-symbolic"));
                    let labels = gap(Orientation::Vertical, 3);
                    labels.set_hexpand(true);
                    labels.append(&text(&masked(&st, &name), "saifori-row-description"));
                    let orders_text = orders.to_string();
                    let qty_text = quantity.to_string();
                    let average = model::money(total / quantity.max(1), &cur);
                    let details = i18n::fmt(
                        "{orders} purchases · {qty} items · avg {avg} · last {day}",
                        &[
                            ("orders", &orders_text),
                            ("qty", &qty_text),
                            ("avg", &average),
                            ("day", &last),
                        ],
                    );
                    labels.append(&text(&masked(&st, &details), "saifori-muted"));
                    row.append(&labels);
                    let money = text(&masked_money(&st, total, &cur), "saifori-amount");
                    money.set_wrap(false);
                    row.append(&money);
                    area.append(&row);
                }
            }
            Err(err) => area.append(&text(&format!("{err}"), "saifori-negative")),
        }
    };
    product_review_sections(state, parent);
    // Initial view and subsequent period selection use the same aggregation.
    update();
    let st = state.clone();
    options.connect_selected_notify(move |widget| {
        if st.product_period.get() != widget.selected() {
            st.product_period.set(widget.selected());
            refresh(&st);
        }
    });
}

pub(super) fn confirm_name_link(state: &Rc<State>, alias: &str, canonical: &str) {
    let dialog = gtk::Window::builder()
        .title("Link product names?")
        .transient_for(&state.window)
        .modal(true)
        .default_width(470)
        .resizable(false)
        .build();
    let content = gap(Orientation::Vertical, 16);
    margin(&content, 24);
    content.append(&text("Link these product names?", "saifori-section-title"));
    let explanation=format!("{alias} → {canonical}\n\nOnly product statistics will use the unified name. Original transactions, dates and amounts remain unchanged.");
    content.append(&text(&explanation, "saifori-muted"));
    let actions = gap(Orientation::Horizontal, 12);
    actions.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let win = dialog.clone();
    cancel.connect_clicked(move |_| win.close());
    actions.append(&cancel);
    let confirm = button("Link names", "document-save-symbolic");
    confirm.add_css_class("suggested-action");
    let st = state.clone();
    let win = dialog.clone();
    let a = alias.to_string();
    let c = canonical.to_string();
    confirm.connect_clicked(move |_| {
        let result = st.db.borrow_mut().link_product_name(&a, &c);
        if run_write(&st, result) {
            win.close();
        }
    });
    actions.append(&confirm);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.present();
}

pub(super) fn product_review_sections(state: &Rc<State>, parent: &gtk::Box) {
    if state.privacy.get() {
        parent.append(&private_placeholder());
        return;
    }
    let cur = currency(state);
    let aliases = card();
    aliases.append(&section_heading(
        "Similar product names",
        "Review suggestions before grouping names in reports",
    ));
    let manual = gap(Orientation::Horizontal, 8);
    manual.add_css_class("saifori-alias-manual");
    let from = gtk::Entry::new();
    from.set_hexpand(true);
    from.set_placeholder_text(Some(&i18n::tr("Variant name")));
    let to = gtk::Entry::new();
    to.set_hexpand(true);
    to.set_placeholder_text(Some(&i18n::tr("Preferred name")));
    let link = button("Link names", "document-save-symbolic");
    let st = state.clone();
    let a = from.clone();
    let b = to.clone();
    link.connect_clicked(move |_| confirm_name_link(&st, a.text().as_str(), b.text().as_str()));
    manual.append(&from);
    manual.append(&to);
    manual.append(&link);
    aliases.append(&manual);
    match state.db.borrow().similar_product_names(&cur) {
        Ok(rows) if rows.is_empty() => {
            aliases.append(&text("No likely spelling variants found.", "saifori-muted"))
        }
        Ok(rows) => {
            for (name, other, score) in rows {
                let line = gap(Orientation::Horizontal, 10);
                line.add_css_class("saifori-list-row");
                let label = text(&format!("{name}  /  {other} · {score}% match"), "");
                label.set_hexpand(true);
                label.set_wrap(true);
                line.append(&label);
                let link = button("Use first name", "document-save-symbolic");
                link.set_tooltip_text(Some(
                    "Use the first spelling in reports. Transactions remain untouched.",
                ));
                let st = state.clone();
                let alias = other.clone();
                let canonical = name.clone();
                link.connect_clicked(move |_| confirm_name_link(&st, &alias, &canonical));
                line.append(&link);
                let reverse = button("Use second name", "document-save-symbolic");
                reverse.set_tooltip_text(Some(
                    "Use the second spelling in reports. Transactions remain untouched.",
                ));
                let st = state.clone();
                reverse.connect_clicked(move |_| confirm_name_link(&st, &name, &other));
                line.append(&reverse);
                aliases.append(&line);
            }
        }
        Err(err) => aliases.append(&text(
            &format!("Could not check product names: {err}"),
            "saifori-negative",
        )),
    }
    let current = state.db.borrow().product_aliases().unwrap_or_default();
    if !current.is_empty() {
        aliases.append(&line());
        aliases.append(&section_heading("Linked product names", ""));
        for (alias, canonical) in current {
            let item = gap(Orientation::Horizontal, 9);
            item.add_css_class("saifori-list-row");
            let label = text(&format!("{alias} → {canonical}"), "");
            label.set_hexpand(true);
            item.append(&label);
            let remove = button("Unlink", "edit-clear-symbolic");
            let st = state.clone();
            remove.connect_clicked(move |_| {
                let result = st.db.borrow_mut().unlink_product_name(&alias);
                run_write(&st, result);
            });
            item.append(&remove);
            aliases.append(&item);
        }
    }
    parent.append(&aliases);
    let duplicates = card();
    duplicates.append(&section_heading(
        "Potential duplicate entries",
        "Same day, amount, quantity and payment method; review before taking action",
    ));
    match state.db.borrow().duplicate_candidates(&cur) {
        Ok(rows) if rows.is_empty() => duplicates.append(&text(
            "No suspicious duplicate entries found.",
            "saifori-muted",
        )),
        Ok(rows) => {
            for (a, b, day, name, other, amount, _) in rows {
                let entry = gap(Orientation::Horizontal, 10);
                entry.add_css_class("saifori-list-row");
                let description = text(
                    &format!("{day} · {name} / {other} · {}", model::money(amount, &cur)),
                    "",
                );
                description.set_hexpand(true);
                description.set_wrap(true);
                entry.append(&description);
                let inspect = button("Review", "document-open-symbolic");
                let st = state.clone();
                inspect.connect_clicked(move |_| {
                    st.selected.set(Some(b));
                    change_page(&st, Page::Transactions);
                });
                entry.append(&inspect);
                let ignore = button("Not duplicate", "edit-clear-symbolic");
                let st = state.clone();
                ignore.connect_clicked(move |_| {
                    let result = st.db.borrow_mut().ignore_duplicate_pair(a, b);
                    run_write(&st, result);
                });
                entry.append(&ignore);
                duplicates.append(&entry);
            }
        }
        Err(err) => duplicates.append(&text(
            &format!("Could not check duplicates: {err}"),
            "saifori-negative",
        )),
    }
    parent.append(&duplicates);
}

// GNOME Keyring calls are blocking, so they only run on worker threads.
// The GUI gets outcomes via a short-lived GTK main-context polling callback.
