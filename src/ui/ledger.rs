use super::*;

pub(super) fn transaction_columns() -> gtk::Box {
    let row = gap(Orientation::Horizontal, 10);
    row.add_css_class("saifori-table-heading");
    row.append(&table_cell("Date", 82, "saifori-muted"));
    let desc = table_cell("Description", 170, "saifori-muted");
    desc.set_hexpand(true);
    row.append(&desc);
    row.append(&table_cell("Category", 135, "saifori-muted"));
    row.append(&table_cell("Payment method", 118, "saifori-muted"));
    row.append(&table_cell("Type", 86, "saifori-muted"));
    let amount = table_cell("Amount", 132, "saifori-muted");
    amount.set_xalign(1.0);
    row.append(&amount);
    row
}
pub(super) fn transaction_table_row(state: &Rc<State>, tx: &Transaction) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.add_css_class("saifori-table-row");
    if state.selected.get() == Some(tx.id) {
        b.add_css_class("selected");
    }
    let row = gap(Orientation::Horizontal, 10);
    row.append(&table_cell(&friendly_day(&tx.day), 82, "saifori-muted"));
    let name = gap(Orientation::Horizontal, 9);
    name.set_width_request(170);
    name.set_hexpand(true);
    let img = icon(model::category_icon(&tx.category));
    img.set_pixel_size(16);
    name.append(&img);
    let description = table_cell(
        &masked(state, &tx.description),
        110,
        "saifori-row-description",
    );
    description.set_hexpand(true);
    name.append(&description);
    row.append(&name);
    row.append(&table_cell(&masked(state, &tx.category), 135, ""));
    row.append(&table_cell(
        &masked(state, &tx.method),
        118,
        "saifori-muted",
    ));
    let kind = table_cell(
        if tx.kind == "expense" {
            "Expense"
        } else {
            "Income"
        },
        86,
        "saifori-type-pill",
    );
    kind.add_css_class(if tx.kind == "expense" {
        "saifori-type-expense"
    } else {
        "saifori-type-income"
    });
    row.append(&kind);
    let value = if state.privacy.get() {
        masked_money(state, tx.amount_minor, &tx.currency)
    } else {
        format!(
            "{}{}",
            if tx.kind == "expense" { "−" } else { "+" },
            model::money(tx.amount_minor, &tx.currency)
        )
    };
    let amount = table_cell(&value, 132, "saifori-amount");
    amount.set_xalign(1.0);
    amount.add_css_class(if tx.kind == "expense" {
        "saifori-negative"
    } else {
        "saifori-positive"
    });
    row.append(&amount);
    b.set_child(Some(&row));
    let st = state.clone();
    let id = tx.id;
    b.connect_clicked(move |_| {
        st.selected.set(Some(id));
        refresh(&st);
    });
    b
}
pub(super) fn detail_field(parent: &gtk::Box, title: &str, value: &str) {
    let row = gap(Orientation::Horizontal, 16);
    row.add_css_class("saifori-detail-row");
    let name = text(title, "saifori-muted");
    name.set_hexpand(true);
    row.append(&name);
    let val = text(value, "");
    val.set_xalign(1.0);
    row.append(&val);
    parent.append(&row);
}
pub(super) fn transactions(state: &Rc<State>, parent: &gtk::Box) {
    page_title(parent, "Transactions", "");
    let filters = gap(Orientation::Horizontal, 12);
    filters.add_css_class("saifori-filter-row");
    filters.set_hexpand(true);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search transactions…"));
    search.set_hexpand(true);
    search.set_text(&state.filter.borrow());
    if state.privacy.get() {
        search.set_text("");
        search.set_placeholder_text(Some("••••"));
    }
    let st = state.clone();
    search.connect_search_changed(move |entry| {
        *st.filter.borrow_mut() = entry.text().to_string();
    });
    filters.append(&search);
    let date_menu = gtk::MenuButton::new();
    date_menu.set_icon_name("x-office-calendar-symbolic");
    date_menu.set_tooltip_text(Some("Filter by calendar day"));
    let date_popover = gtk::Popover::new();
    let cal_box = gap(Orientation::Vertical, 8);
    margin(&cal_box, 12);
    let calendar = gtk::Calendar::new();
    cal_box.append(&calendar);
    let clear = button("All days", "edit-clear-symbolic");
    let st = state.clone();
    clear.connect_clicked(move |_| {
        *st.tx_day.borrow_mut() = None;
        refresh(&st);
    });
    cal_box.append(&clear);
    date_popover.set_child(Some(&cal_box));
    date_menu.set_popover(Some(&date_popover));
    let st = state.clone();
    calendar.connect_day_selected(move |widget| {
        let d = widget.date();
        *st.tx_day.borrow_mut() = Some(format!(
            "{:04}-{:02}-{:02}",
            d.year(),
            d.month(),
            d.day_of_month()
        ));
        refresh(&st);
    });
    filters.append(&date_menu);
    if let Some(day) = state.tx_day.borrow().clone() {
        let chip = button(&day, "edit-clear-symbolic");
        let st = state.clone();
        chip.connect_clicked(move |_| {
            *st.tx_day.borrow_mut() = None;
            refresh(&st);
        });
        filters.append(&chip);
    }
    let dates = dropdown(&["All dates", "This month", "Last 6 months"]);
    dates.set_selected(state.tx_range.get());
    filters.append(&dates);
    let kinds = dropdown(&["All types", "Expenses", "Income"]);
    kinds.set_selected(state.tx_kind.get());
    filters.append(&kinds);
    let category_options: Vec<String> = std::iter::once("All categories".to_string())
        .chain(category_options(state))
        .collect();
    let categories = choice_owned(&category_options, "All categories");
    categories.set_selected(state.tx_category.get());
    filters.append(&categories);
    // Live filters match the approved design; no redundant Apply button.
    let apply = button("Apply", "edit-find-symbolic");
    parent.append(&filters);
    if !state.privacy.get() {
        if let Ok(pairs) = state.db.borrow().duplicate_candidates(&currency(state)) {
            if !pairs.is_empty() {
                let alert = gap(Orientation::Horizontal, 12);
                alert.add_css_class("saifori-duplicate-alert");
                let label = text(&format!("{} possible duplicate entries", pairs.len()), "");
                label.set_hexpand(true);
                alert.append(&label);
                let inspect = button("Review duplicates", "edit-find-symbolic");
                let st = state.clone();
                inspect.connect_clicked(move |_| change_page(&st, Page::Products));
                alert.append(&inspect);
                parent.append(&alert);
            }
        }
    }

    let row = gap(Orientation::Horizontal, 16);
    row.set_valign(gtk::Align::Fill);
    row.set_vexpand(true);
    let list_card = card();
    list_card.set_hexpand(true);
    list_card.set_vexpand(true);
    list_card.set_width_request(760);
    list_card.add_css_class("saifori-transaction-table");
    let timeline_nodes: TimelineNodes = timeline_nodes();
    let month_anchors: MonthAnchors = Rc::new(RefCell::new(Vec::new()));
    list_card.append(&timeline_controls(state, &timeline_nodes));
    list_card.append(&transaction_columns());
    let list_area = gap(Orientation::Vertical, 0);
    // GtkViewport fills the available scroller height when the ledger is short.
    // Pin content to the TOP; otherwise collapsed groups can be stretched into
    // tall empty slots between month headers.
    list_area.set_vexpand(false);
    list_area.set_valign(gtk::Align::Start);
    let st = state.clone();
    let kinds_clone = kinds.clone();
    let dates_clone = dates.clone();
    let cats_clone = categories.clone();
    let list_clone = list_area.clone();
    let nodes = timeline_nodes.clone();
    let anchors_for_filters = month_anchors.clone();
    let category_options_apply = category_options.clone();
    let tx_filter_adjustment: Rc<RefCell<Option<gtk::Adjustment>>> = Rc::new(RefCell::new(None));
    let tx_adjustment_for_filters = tx_filter_adjustment.clone();
    apply.connect_clicked(move |_| {
        if let Some(adj) = tx_adjustment_for_filters.borrow().as_ref() {
            adj.set_value(adj.lower());
        }
        while let Some(child) = list_clone.first_child() {
            list_clone.remove(&child);
        }
        nodes.borrow_mut().groups.clear();
        nodes.borrow().update_toggle();
        anchors_for_filters.borrow_mut().clear();
        st.transaction_scroll_position.set(0.0);
        let filter = st.filter.borrow().clone();
        st.tx_kind.set(kinds_clone.selected());
        st.tx_range.set(dates_clone.selected());
        st.tx_category.set(cats_clone.selected());
        let kind = match kinds_clone.selected() {
            1 => "expense",
            2 => "income",
            _ => "",
        };
        let category = if cats_clone.selected() == 0 {
            ""
        } else {
            category_options_apply
                .get(cats_clone.selected() as usize)
                .map(String::as_str)
                .unwrap_or("")
        };
        fill_transactions(
            &st,
            &list_clone,
            TransactionFilters {
                needle: &filter,
                kind,
                date_range: dates_clone.selected(),
                category,
            },
            &nodes,
            &anchors_for_filters,
        );
    });
    let run = apply.clone();
    search.connect_search_changed(move |_| {
        run.emit_clicked();
    });
    let run = apply.clone();
    search.connect_activate(move |_| {
        run.emit_clicked();
    });
    let run = apply.clone();
    dates.connect_selected_notify(move |_| {
        run.emit_clicked();
    });
    let run = apply.clone();
    kinds.connect_selected_notify(move |_| {
        run.emit_clicked();
    });
    let run = apply.clone();
    categories.connect_selected_notify(move |_| {
        run.emit_clicked();
    });
    let kind = match state.tx_kind.get() {
        1 => "expense",
        2 => "income",
        _ => "",
    };
    let category = if state.tx_category.get() == 0 {
        ""
    } else {
        category_options
            .get(state.tx_category.get() as usize)
            .map(String::as_str)
            .unwrap_or("")
    };
    fill_transactions(
        state,
        &list_area,
        TransactionFilters {
            needle: &state.filter.borrow(),
            kind,
            date_range: state.tx_range.get(),
            category,
        },
        &timeline_nodes,
        &month_anchors,
    );
    let list_scroll = scroll(&list_area);
    *tx_filter_adjustment.borrow_mut() = Some(list_scroll.vadjustment());
    list_scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    list_scroll.set_min_content_height(180);
    list_scroll.add_css_class("saifori-transaction-scroll");
    let ledger = sticky_month_ledger(
        state,
        &list_scroll,
        &list_area,
        &month_anchors,
        &timeline_nodes,
        false,
    );
    ledger.add_css_class("saifori-transaction-ledger");
    list_card.append(&ledger);
    // Persist ledger scroll position when selecting a row refreshes its detail
    // panel. Filter changes intentionally reset to the beginning instead.
    let st = Rc::downgrade(state);
    list_scroll.vadjustment().connect_value_changed(move |a| {
        if let Some(st) = st.upgrade() {
            st.transaction_scroll_position.set(a.value());
        }
    });
    let restore = state.transaction_scroll_position.get();
    let adjustment = list_scroll.vadjustment();
    gtk::glib::idle_add_local_once(move || {
        let max = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
        adjustment.set_value(restore.clamp(adjustment.lower(), max));
    });
    row.append(&list_card);

    let details = card();
    details.set_width_request(290);
    details.set_vexpand(true);
    details.add_css_class("saifori-details-panel");
    // The panel itself is fixed. Long notes scroll in their own pane; scrolling
    // the ledger cannot move this title or its surrounding chrome.
    let details_content = gap(Orientation::Vertical, 12);
    details_content.set_vexpand(false);
    if let Some(id) = state.selected.get() {
        match state.db.borrow().transaction(id) {
            Ok(Some(tx)) => {
                details.append(&section_heading(
                    "Transaction details",
                    "Selected transaction",
                ));
                let heading = gap(Orientation::Horizontal, 11);
                let ico = icon(model::category_icon(&tx.category));
                ico.set_pixel_size(24);
                heading.append(&ico);
                let title = text(&masked(state, &tx.description), "saifori-section-title");
                title.set_wrap(true);
                heading.append(&title);
                details_content.append(&heading);
                let val = if state.privacy.get() {
                    masked_money(state, tx.amount_minor, &tx.currency)
                } else {
                    format!(
                        "{}{}",
                        if tx.kind == "expense" { "−" } else { "+" },
                        model::money(tx.amount_minor, &tx.currency)
                    )
                };
                let amount = text(&val, "saifori-kpi");
                amount.add_css_class(if tx.kind == "expense" {
                    "saifori-negative"
                } else {
                    "saifori-positive"
                });
                details_content.append(&amount);
                details_content.append(&line());
                detail_field(&details_content, "Date", &tx.day);
                detail_field(
                    &details_content,
                    "Quantity",
                    &masked(state, &tx.quantity.to_string()),
                );
                if tx.quantity > 1 {
                    detail_field(
                        &details_content,
                        "Unit price",
                        &masked_money(state, tx.amount_minor / tx.quantity, &tx.currency),
                    );
                }
                detail_field(&details_content, "Category", &masked(state, &tx.category));
                detail_field(
                    &details_content,
                    "Type",
                    if tx.kind == "expense" {
                        "Expense"
                    } else {
                        "Income"
                    },
                );
                detail_field(&details_content, "Payment", &masked(state, &tx.method));
                detail_field(&details_content, "Created (UTC)", &tx.created_at);
                if !tx.updated_at.is_empty() {
                    detail_field(&details_content, "Last updated (UTC)", &tx.updated_at);
                }
                if !tx.note.is_empty() {
                    details_content.append(&line());
                    details_content.append(&section_heading("Note", ""));
                    details_content.append(&text(&masked(state, &tx.note), "saifori-muted"));
                }
                details_content.append(&line());
                let buttons = gap(Orientation::Horizontal, 10);
                let edit = button("Edit", "document-edit-symbolic");
                let st = state.clone();
                edit.connect_clicked(move |_| transaction_dialog(&st, Some(id)));
                buttons.append(&edit);
                let delete = button("Delete", "user-trash-symbolic");
                delete.add_css_class("destructive-action");
                let st = state.clone();
                delete.connect_clicked(move |_| confirm_delete(&st, id));
                buttons.append(&delete);
                details_content.append(&buttons);
            }
            _ => details_content.append(&text(
                "Select a transaction to see its details.",
                "saifori-muted",
            )),
        }
    } else {
        details.append(&section_heading(
            "Transaction details",
            "Select a row to view and edit it",
        ));
        details_content.append(&text(
            "Choose any entry in the table to inspect its amount, category and payment method.",
            "saifori-muted",
        ));
    }
    let detail_scroll = scroll(&details_content);
    detail_scroll.set_min_content_height(160);
    detail_scroll.add_css_class("saifori-details-scroll");
    details.append(&detail_scroll);
    row.append(&details);
    parent.append(&row);
}

struct TransactionFilters<'a> {
    needle: &'a str,
    kind: &'a str,
    date_range: u32,
    category: &'a str,
}
fn fill_transactions(
    state: &Rc<State>,
    container: &gtk::Box,
    filters: TransactionFilters<'_>,
    registry: &TimelineNodes,
    anchors: &MonthAnchors,
) {
    let TransactionFilters {
        needle,
        kind,
        date_range,
        category,
    } = filters;
    let cur = currency(state);
    let lower = match date_range {
        1 => Some(month(state)),
        2 => model::month_offset(&month(state), -5),
        _ => None,
    };
    let from = lower.as_ref().map(|m| format!("{m}-01"));
    let until = if date_range == 0 {
        None
    } else {
        model::month_offset(&month(state), 1).map(|m| format!("{m}-01"))
    };
    let specific_day = state.tx_day.borrow().clone();
    let cat = if category.is_empty() {
        None
    } else {
        Some(category)
    };
    match state
        .db
        .borrow()
        .search_transactions(crate::db::TransactionSearch {
            currency: &cur,
            limit: 250,
            needle,
            kind,
            from: from.as_deref(),
            until: until.as_deref(),
            category: cat,
            day: specific_day.as_deref(),
        }) {
        Ok(records) => {
            let truncated = records.len() >= 250;
            let mut months: BTreeMap<String, BTreeMap<String, Vec<Transaction>>> = BTreeMap::new();
            for tx in records {
                let m = tx.day.get(..7).unwrap_or("Unknown").to_string();
                months
                    .entry(m)
                    .or_default()
                    .entry(tx.day.clone())
                    .or_default()
                    .push(tx);
            }
            if months.is_empty() {
                container.append(&text("No matching transactions.", "saifori-empty"));
                return;
            }
            for (month_index, (month, days)) in months.iter().rev().enumerate() {
                let expense: i64 = days
                    .values()
                    .flat_map(|v| v.iter())
                    .filter(|t| t.kind == "expense")
                    .map(|t| t.amount_minor)
                    .sum();
                let income: i64 = days
                    .values()
                    .flat_map(|v| v.iter())
                    .filter(|t| t.kind == "income")
                    .map(|t| t.amount_minor)
                    .sum();
                let count: usize = days.values().map(Vec::len).sum();
                let days_box = gap(Orientation::Vertical, 3);
                days_box.set_vexpand(false);
                days_box.set_valign(gtk::Align::Start);
                days_box.add_css_class("saifori-timeline-days");
                for (day_index, (day, records)) in days.iter().rev().enumerate() {
                    let spend: i64 = records
                        .iter()
                        .filter(|t| t.kind == "expense")
                        .map(|t| t.amount_minor)
                        .sum();
                    let rows = gap(Orientation::Vertical, 2);
                    rows.set_vexpand(false);
                    rows.set_valign(gtk::Align::Start);
                    for tx in records {
                        rows.append(&transaction_table_row(state, tx));
                    }
                    let day_key = format!("tx:day:{day}");
                    let day_label = friendly_day(day).to_string();
                    let day_details =
                        format!("{}  ·  {}", records.len(), masked_money(state, spend, &cur));
                    days_box.append(&timeline_group(
                        state,
                        TimelineGroup {
                            key: day_key,
                            title: day_label,
                            detail: day_details,
                            level: TimelineLevel::Day,
                            default_open: month_index == 0 && day_index == 0,
                        },
                        &rows,
                        registry,
                    ));
                }
                let month_key = format!("tx:month:{month}");
                let month_info = format!(
                    "{}  ·  {} {}  ·  {} {}",
                    count,
                    i18n::tr("Out"),
                    masked_money(state, expense, &cur),
                    i18n::tr("In"),
                    masked_money(state, income, &cur)
                );
                let title = i18n::display_month(month);
                let group = timeline_group(
                    state,
                    TimelineGroup {
                        key: month_key.clone(),
                        title: title.clone(),
                        detail: month_info.clone(),
                        level: TimelineLevel::Month,
                        default_open: month_index == 0,
                    },
                    &days_box,
                    registry,
                );
                anchors
                    .borrow_mut()
                    .push(MonthAnchor::new(month_key, group.clone()));
                container.append(&group);
            }
            if truncated {
                container.append(&text(
                    "Showing 250 results. Narrow the search to see older records.",
                    "saifori-muted",
                ));
            }
        }
        Err(err) => container.append(&text(
            &format!("Could not read transactions: {err}"),
            "saifori-negative",
        )),
    }
}
