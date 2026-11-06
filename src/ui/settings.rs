use super::*;

pub(super) fn custom_categories_settings(state: &Rc<State>) -> adw::PreferencesGroup {
    let group = preference_group("Custom categories", "");
    if state.privacy.get() {
        let hidden = preference_action("Categories hidden", "Turn off privacy mode to edit them.");
        group.add(&hidden);
        return group;
    }
    let create = preference_action(
        "New category",
        "Create a category for transactions and budgets.",
    );
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some(&i18n::tr("New category name")));
    entry.set_width_chars(15);
    let add = button("Add", "list-add-symbolic");
    add.add_css_class("suggested-action");
    let st = state.clone();
    let e = entry.clone();
    add.connect_clicked(move |_| {
        let name = e.text().to_string();
        let result = st.db.borrow_mut().add_custom_category(&name);
        run_write(&st, result);
    });
    create.add_suffix(&entry);
    create.add_suffix(&add);
    group.add(&create);
    if let Ok(names) = state.db.borrow().category_names() {
        let builtins = model::CATEGORIES.len();
        if names.len() == builtins {
            let empty = preference_action("No custom categories yet", "");
            group.add(&empty);
        }
        for name in names.into_iter().skip(builtins) {
            let item = preference_action(&name, "");
            let remove = button("Remove", "user-trash-symbolic");
            let st = state.clone();
            let category_name = name.clone();
            remove.connect_clicked(move |_| {
                let result = st.db.borrow_mut().remove_custom_category(&category_name);
                run_write(&st, result);
            });
            item.add_suffix(&remove);
            group.add(&item);
        }
    }
    group
}

pub(super) fn settings_page(state: &Rc<State>, page: &gtk::Box) {
    let narrow = gap(Orientation::Vertical, 22);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(680);
    clamp.set_tightening_threshold(520);
    clamp.set_child(Some(&narrow));
    page.append(&clamp);
    page_title(&narrow, "Settings", "");

    let demo = preference_group(
        "Test Mode",
        "Explore a temporary USD ledger without modifying your records.",
    );
    let demo_row = preference_switch(
        "Enable USD sample data",
        "Sample data is erased when Test Mode ends.",
        state.test_mode.get(),
    );
    let st = state.clone();
    demo_row.connect_active_notify(move |control| {
        if st.test_mode.get() != control.is_active() {
            change_test_mode(&st, control.is_active());
        }
    });
    demo.add(&demo_row);
    narrow.append(&demo);

    let appearance = preference_group("Appearance & Language", "");
    let currency_row = preference_action(
        "Default currency",
        "Each ledger is reported in its original currency.",
    );
    let chooser = choice(model::CURRENCIES, &currency(state));
    chooser.set_width_request(104);
    chooser.set_sensitive(!state.test_mode.get());
    let chooser_for_apply = chooser.clone();
    let apply = button("Apply", "document-save-symbolic");
    apply.set_sensitive(!state.test_mode.get());
    let st = state.clone();
    apply.connect_clicked(move |_| {
        let cur = selected(model::CURRENCIES, &chooser_for_apply);
        let result = { st.db.borrow_mut().set_currency(&cur) };
        match result {
            Ok(()) => {
                *st.currency.borrow_mut() = cur;
                refresh(&st);
            }
            Err(e) => warning(&st, "Settings error", &e.to_string()),
        }
    });
    currency_row.add_suffix(&chooser);
    currency_row.add_suffix(&apply);
    appearance.add(&currency_row);

    let language_row = preference_action("Language", "");
    let control = gtk::DropDown::from_strings(&["English", "Español"]);
    let index = i18n::LANGUAGES
        .iter()
        .position(|(code, _)| *code == state.language.borrow().as_str())
        .unwrap_or(0);
    control.set_selected(index as u32);
    let st = state.clone();
    control.connect_selected_notify(move |dropdown| {
        let chosen = i18n::LANGUAGES
            .get(dropdown.selected() as usize)
            .map(|(code, _)| *code)
            .unwrap_or("en");
        if st.language.borrow().as_str() == chosen {
            return;
        }
        let result = { st.db.borrow_mut().set_setting("language", chosen) };
        match result {
            Ok(()) => {
                *st.language.borrow_mut() = chosen.to_owned();
                refresh(&st);
            }
            Err(e) => warning(&st, "Settings error", &e.to_string()),
        }
    });
    language_row.add_suffix(&control);
    appearance.add(&language_row);
    let privacy = preference_switch(
        "Privacy mode",
        "Mask on-screen text and amounts; storage is not encrypted.",
        state.privacy.get(),
    );
    let st = state.clone();
    privacy.connect_active_notify(move |row| {
        let enabled = row.is_active();
        if st.privacy.get() == enabled {
            return;
        }
        let result = {
            st.db
                .borrow_mut()
                .set_setting("privacy", if enabled { "1" } else { "0" })
        };
        match result {
            Ok(()) => {
                st.privacy.set(enabled);
                refresh(&st);
            }
            Err(e) => warning(&st, "Settings error", &e.to_string()),
        }
    });
    appearance.add(&privacy);
    narrow.append(&appearance);

    let workspace = preference_group("Workspace", "");
    let assistant_tabs = preference_switch(
        "Separate Chat and Smart Capture tabs",
        "Choose between two dedicated tabs or a split assistant view.",
        state.assistant_tabs.get(),
    );
    let st = state.clone();
    assistant_tabs.connect_active_notify(move |row| {
        let enabled = row.is_active();
        if st.assistant_tabs.get() == enabled {
            return;
        }
        let result = {
            st.db
                .borrow_mut()
                .set_setting("assistant_tabs", if enabled { "1" } else { "0" })
        };
        match result {
            Ok(()) => {
                st.assistant_tabs.set(enabled);
                refresh(&st);
            }
            Err(err) => warning(&st, "Settings error", &err.to_string()),
        }
    });
    workspace.add(&assistant_tabs);
    let full_page = preference_switch(
        "Open transaction editor as a full page",
        "Use a dedicated page instead of a modal dialog.",
        state.transaction_as_page.get(),
    );
    let st = state.clone();
    full_page.connect_active_notify(move |row| {
        let enabled = row.is_active();
        if st.transaction_as_page.get() == enabled {
            return;
        }
        let result = {
            st.db
                .borrow_mut()
                .set_setting("transaction_page", if enabled { "1" } else { "0" })
        };
        match result {
            Ok(()) => {
                st.transaction_as_page.set(enabled);
                refresh(&st);
            }
            Err(err) => warning(&st, "Settings error", &err.to_string()),
        }
    });
    workspace.add(&full_page);
    let compact = preference_switch(
        "Collapse previous transfer months while scrolling",
        "Keep the active month visible within transfer history.",
        state.transfer_compact_months.get(),
    );
    let st = state.clone();
    compact.connect_active_notify(move |row| {
        let enabled = row.is_active();
        if st.transfer_compact_months.get() == enabled {
            return;
        }
        match st
            .db
            .borrow_mut()
            .set_setting("transfer_compact_months", if enabled { "1" } else { "0" })
        {
            Ok(()) => st.transfer_compact_months.set(enabled),
            Err(err) => warning(&st, "Settings error", &err.to_string()),
        }
    });
    workspace.add(&compact);
    narrow.append(&workspace);

    // Custom categories and keyring controls retain their established, audited
    // business logic; only their presentation is gradually moved into Adwaita.
    narrow.append(&custom_categories_settings(state));
    narrow.append(&keyring_controls(state));

    let data = preference_group(
        "Your Data",
        "Exports are unencrypted. Keep archives private.",
    );
    let export = preference_action(
        "Transactions",
        "Export a CSV file of your recorded transactions.",
    );
    let export_button = button("Export CSV", "document-save-as-symbolic");
    export_button.set_sensitive(!state.test_mode.get());
    let st = state.clone();
    export_button.connect_clicked(move |_| {
        export_data(
            &st,
            "Export transactions",
            "saifori-transactions.csv",
            Store::export_csv,
        )
    });
    export.add_suffix(&export_button);
    data.add(&export);
    let backup = preference_action("Backup", "Export or restore your local financial records.");
    let backup_button = button("Export", "document-save-as-symbolic");
    let restore_button = button("Restore", "document-open-symbolic");
    backup_button.set_sensitive(!state.test_mode.get());
    restore_button.set_sensitive(!state.test_mode.get());
    let st = state.clone();
    backup_button.connect_clicked(move |_| {
        export_data(
            &st,
            "Export private backup",
            "saifori-backup.json",
            Store::backup_json,
        )
    });
    let st = state.clone();
    restore_button.connect_clicked(move |_| choose_backup(&st));
    backup.add_suffix(&backup_button);
    backup.add_suffix(&restore_button);
    data.add(&backup);
    let chats = preference_action(
        "Chat history",
        "Export your locally saved assistant conversations.",
    );
    let export_chats = button("Export JSON", "document-save-as-symbolic");
    export_chats.set_sensitive(!state.test_mode.get());
    let st = state.clone();
    export_chats.connect_clicked(move |_| {
        export_data(
            &st,
            "Export private chat history",
            "saifori-chats.json",
            Store::chat_export_json,
        )
    });
    chats.add_suffix(&export_chats);
    data.add(&chats);
    narrow.append(&data);
    let help_group = preference_group("Support", "");
    let help_row = preference_action(
        "Help & FAQ",
        "Learn how transactions, privacy and Smart Capture work.",
    );
    let help_button = button("Open", "help-browser-symbolic");
    let st = state.clone();
    help_button.connect_clicked(move |_| change_page(&st, Page::Help));
    help_row.add_suffix(&help_button);
    help_group.add(&help_row);
    narrow.append(&help_group);
}
