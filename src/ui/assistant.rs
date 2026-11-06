use super::*;

pub(super) fn load_secret(state: &Rc<State>) {
    if state.key_state.get() == KeyState::Busy {
        return;
    }
    state.key_state.set(KeyState::Loading);
    if state.page.get() == Page::Assistant || state.page.get() == Page::Settings {
        refresh(state);
    }
    let provider = state.ai_provider.borrow().clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(secret_store::lookup(&provider));
    });
    let st = state.clone();
    glib::timeout_add_local(Duration::from_millis(90), move || {
        match receiver.try_recv() {
            Ok(Ok(Some(key))) => {
                *st.api_key.borrow_mut() = key;
                st.key_state.set(KeyState::Stored);
                refresh(&st);
                glib::ControlFlow::Break
            }
            Ok(Ok(None)) => {
                st.api_key.borrow_mut().clear();
                st.key_state.set(KeyState::Missing);
                refresh(&st);
                glib::ControlFlow::Break
            }
            Ok(Err(_)) => {
                st.api_key.borrow_mut().clear();
                st.key_state.set(KeyState::Error);
                refresh(&st);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                st.key_state.set(KeyState::Error);
                refresh(&st);
                glib::ControlFlow::Break
            }
        }
    });
}

pub(super) fn save_secret(state: &Rc<State>, key: String) {
    if state.key_state.get() == KeyState::Busy || state.key_state.get() == KeyState::Loading {
        return;
    }
    if key.is_empty() || key.len() > 1000 || key.chars().any(char::is_whitespace) {
        warning(
            state,
            "Invalid API key",
            "Enter a key without spaces (up to 1,000 characters).",
        );
        return;
    }
    let existing = if state.api_key.borrow().is_empty() {
        KeyState::Missing
    } else {
        KeyState::Stored
    };
    let provider = state.ai_provider.borrow().clone();
    state.key_state.set(KeyState::Busy);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = secret_store::store(&provider, &key).map(|_| key);
        let _ = sender.send(result);
    });
    let st = state.clone();
    glib::timeout_add_local(Duration::from_millis(90), move || {
        match receiver.try_recv() {
            Ok(Ok(key)) => {
                *st.api_key.borrow_mut() = key;
                st.key_state.set(KeyState::Stored);
                refresh(&st);
                glib::ControlFlow::Break
            }
            Ok(Err(_)) => {
                st.key_state.set(existing);
                warning(
                    &st,
                    "Key not saved",
                    "Unlock GNOME Keyring and try again. The key was not saved to a file.",
                );
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                st.key_state.set(existing);
                warning(&st, "Key not saved", "Could not reach secure storage.");
                glib::ControlFlow::Break
            }
        }
    });
}

pub(super) fn confirm_remove_secret(state: &Rc<State>) {
    let provider = state.ai_provider.borrow().clone();
    let popup = gtk::Window::builder()
        .title(i18n::tr("Remove stored key?"))
        .transient_for(&state.window)
        .modal(true)
        .default_width(430)
        .resizable(false)
        .build();
    let body = gap(Orientation::Vertical, 14);
    margin(&body, 24);
    body.append(&text(
        &format!(
            "Remove the {} key from GNOME Keyring?",
            provider_label(&provider)
        ),
        "saifori-section-title",
    ));
    let controls = gap(Orientation::Horizontal, 10);
    controls.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let dialog = popup.clone();
    cancel.connect_clicked(move |_| dialog.close());
    controls.append(&cancel);
    let remove = button("Remove stored key", "user-trash-symbolic");
    remove.add_css_class("destructive-action");
    let st = state.clone();
    let dialog = popup.clone();
    remove.connect_clicked(move |_| {
        dialog.close();
        remove_secret(&st);
    });
    controls.append(&remove);
    body.append(&controls);
    popup.set_child(Some(&body));
    popup.present();
}

pub(super) fn remove_secret(state: &Rc<State>) {
    if matches!(state.key_state.get(), KeyState::Busy | KeyState::Loading) {
        return;
    }
    let previous = state.key_state.get();
    let provider = state.ai_provider.borrow().clone();
    state.key_state.set(KeyState::Busy);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(secret_store::remove(&provider));
    });
    let st = state.clone();
    glib::timeout_add_local(Duration::from_millis(90), move || {
        match receiver.try_recv() {
            Ok(Ok(())) => {
                st.api_key.borrow_mut().clear();
                st.key_state.set(KeyState::Missing);
                refresh(&st);
                glib::ControlFlow::Break
            }
            Ok(Err(_)) => {
                st.key_state.set(previous);
                warning(
                    &st,
                    "Key not removed",
                    "Unlock GNOME Keyring and try again.",
                );
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                st.key_state.set(previous);
                warning(&st, "Key not removed", "Could not reach secure storage.");
                glib::ControlFlow::Break
            }
        }
    });
}

// Persistent local chat sessions. Never send the saved conversation list to the remote AI provider.
pub(super) fn append_chat_reply(state: &Rc<State>, session: i64, body: String) {
    if let Err(err) = state.db.borrow_mut().chat_append(session, false, &body) {
        warning(state, "Could not save conversation", &err.to_string());
    }
    state
        .messages
        .borrow_mut()
        .push(Message { mine: false, body });
    refresh(state);
}

pub(super) fn send_chat_message(state: &Rc<State>, prompt: String) {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() || state.chat_busy.get() {
        return;
    }
    if ai::capture_word_count(&prompt) > ai::MAX_CAPTURE_WORDS
        || prompt.chars().count() > ai::MAX_CAPTURE_CHARS
    {
        warning(
            state,
            "Message too long",
            "Messages are limited to 500 words and 4,000 characters.",
        );
        return;
    }
    if matches!(state.key_state.get(), KeyState::Loading | KeyState::Busy) {
        warning(state, "Keyring busy", "Secure storage is still loading.");
        return;
    }
    let cur = currency(state);
    let context_month = state
        .chat_month
        .borrow()
        .clone()
        .unwrap_or_else(|| month(state));
    let m = ai::explicit_chat_month(&prompt, &context_month).unwrap_or(context_month);
    let existing = state.active_chat.get();
    let session = match existing {
        Some(id) => id,
        None => match state.db.borrow_mut().chat_create(&prompt, &m) {
            Ok(id) => id,
            Err(err) => {
                warning(state, "Could not create conversation", &err.to_string());
                return;
            }
        },
    };
    let stored = {
        let mut store = state.db.borrow_mut();
        store
            .chat_append(session, true, &prompt)
            .and_then(|_| store.chat_set_month(session, &m))
    };
    if let Err(err) = stored {
        warning(state, "Could not save conversation", &err.to_string());
        return;
    }
    state.active_chat.set(Some(session));
    *state.chat_month.borrow_mut() = Some(m.clone());
    state.messages.borrow_mut().push(Message {
        mine: true,
        body: prompt.clone(),
    });
    let summary = match state.db.borrow().summary(&cur, &m) {
        Ok(v) => v,
        Err(err) => {
            warning(state, "Error", &err.to_string());
            return;
        }
    };
    let cats = match state.db.borrow().categories(&cur, &m) {
        Ok(v) => v,
        Err(err) => {
            warning(state, "Error", &err.to_string());
            return;
        }
    };
    let months = match state.db.borrow().months(&cur, &m, 6) {
        Ok(v) => v,
        Err(err) => {
            warning(state, "Error", &err.to_string());
            return;
        }
    };
    let prior_questions = state
        .messages
        .borrow()
        .iter()
        .rev()
        .skip(1)
        .filter(|msg| msg.mine)
        .map(|msg| msg.body.clone())
        .collect::<Vec<_>>();
    let lang = state.language.borrow().clone();
    if let Some(response) =
        ai::verified_chat_reply(&prompt, &prior_questions, &summary, &cats, &cur, &lang, &m)
    {
        append_chat_reply(state, session, response);
        return;
    }
    let key = state.api_key.borrow().clone();
    let provider = state.ai_provider.borrow().clone();
    if key.is_empty() {
        let response = ai::local_reply(&prompt, &summary, &cats, &cur, &lang);
        append_chat_reply(state, session, response);
        return;
    }
    let history = state
        .messages
        .borrow()
        .iter()
        .rev()
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter(|message| message.mine || !message.body.chars().any(|ch| ch.is_ascii_digit()))
        .map(|message| {
            (
                (if message.mine { "user" } else { "assistant" }).to_owned(),
                message.body,
            )
        })
        .collect::<Vec<_>>();
    state.chat_busy.set(true);
    refresh(state);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let answer = ai::chat_with_ai(
            &provider,
            &key,
            &history,
            ai::ChatContext {
                summary,
                categories: cats,
                months,
                currency: cur,
                period: &m,
                language: &lang,
            },
        )
        .map_err(|e| e.to_string());
        let _ = sender.send(answer);
    });
    let st = state.clone();
    glib::timeout_add_local(Duration::from_millis(90), move || {
        match receiver.try_recv() {
            Ok(answer) => {
                st.chat_busy.set(false);
                let body = match answer {
                    Ok(value) => value,
                    Err(err) => format!("{}: {err}", i18n::tr("Request failed")),
                };
                append_chat_reply(&st, session, body);
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                st.chat_busy.set(false);
                refresh(&st);
                glib::ControlFlow::Break
            }
        }
    });
}

pub(super) fn open_chat_session(state: &Rc<State>, id: i64) {
    if state.chat_busy.get() {
        return;
    }
    let result = state.db.borrow().chat_messages(id);
    let m = state.db.borrow().chat_month(id).ok().flatten();
    match result {
        Ok(saved) => {
            state.active_chat.set(Some(id));
            *state.chat_month.borrow_mut() = m;
            *state.messages.borrow_mut() = saved
                .into_iter()
                .map(|(mine, body)| Message { mine, body })
                .collect();
            state.assistant_view.set(0);
            refresh(state);
        }
        Err(err) => warning(state, "Could not load conversation", &err.to_string()),
    }
}

pub(super) fn delete_open_chat(state: &Rc<State>) {
    if state.chat_busy.get() {
        return;
    }
    if let Some(id) = state.active_chat.get() {
        let dialog = gtk::Window::builder()
            .title(i18n::tr("Delete conversation?"))
            .transient_for(&state.window)
            .modal(true)
            .resizable(false)
            .default_width(380)
            .build();
        let content = gap(Orientation::Vertical, 13);
        margin(&content, 20);
        content.append(&text(
            "Delete this conversation and its messages?",
            "saifori-section-title",
        ));
        let actions = gap(Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        let cancel = button("Cancel", "");
        let w = dialog.clone();
        cancel.connect_clicked(move |_| w.close());
        actions.append(&cancel);
        let approve = button("Delete", "user-trash-symbolic");
        approve.add_css_class("destructive-action");
        let st = state.clone();
        let w = dialog.clone();
        approve.connect_clicked(move |_| {
            w.close();
            delete_chat_now(&st, id);
        });
        actions.append(&approve);
        content.append(&actions);
        dialog.set_child(Some(&content));
        dialog.present();
    }
}

pub(super) fn delete_chat_now(state: &Rc<State>, id: i64) {
    let result = { state.db.borrow_mut().chat_delete(id) };
    match result {
        Ok(()) => {
            state.active_chat.set(None);
            state.messages.borrow_mut().clear();
            *state.chat_month.borrow_mut() = None;
            refresh(state);
        }
        Err(err) => warning(state, "Could not delete conversation", &err.to_string()),
    }
}

/// Every AI/local suggestion remains a proposal until explicitly reviewed and saved.
/// A dedicated editor avoids crowded, error-prone inline table forms.
pub(super) fn edit_capture_draft(state: &Rc<State>, index: usize) {
    if state.privacy.get() {
        warning(
            state,
            "Privacy mode",
            "Turn off privacy mode before editing drafts.",
        );
        return;
    }
    let Some(initial) = state
        .capture_drafts
        .borrow()
        .get(index)
        .map(|(_, d)| d.clone())
    else {
        return;
    };
    let window = gtk::Window::builder()
        .title("Edit draft")
        .transient_for(&state.window)
        .modal(true)
        .default_width(520)
        .default_height(625)
        .build();
    let body = gap(Orientation::Vertical, 14);
    margin(&body, 22);
    body.append(&text("Edit draft", "saifori-title"));

    let description = gtk::Entry::new();
    description.set_text(&initial.description);
    body.append(&field("Description", &description));
    let prices = gap(Orientation::Horizontal, 12);
    prices.set_homogeneous(true);
    let unit = gtk::Entry::new();
    unit.set_input_purpose(gtk::InputPurpose::Number);
    let unit_value = initial.amount_minor / initial.quantity.max(1);
    unit.set_text(&format!("{}.{:02}", unit_value / 100, unit_value % 100));
    let quantity = gtk::SpinButton::with_range(1.0, 10000.0, 1.0);
    quantity.set_numeric(true);
    quantity.set_value(initial.quantity as f64);
    prices.append(&field("Unit price", &unit));
    prices.append(&field("Quantity", &quantity));
    body.append(&prices);
    let currency_control = choice(model::CURRENCIES, &initial.currency);
    body.append(&field("Currency", &currency_control));
    let total = text("Total: —", "saifori-section-title");
    update_total_preview(&unit, &quantity, &total, &initial.currency);
    let a = unit.clone();
    let q = quantity.clone();
    let t = total.clone();
    let c = currency_control.clone();
    unit.connect_changed(move |_| {
        update_total_preview(&a, &q, &t, &selected(model::CURRENCIES, &c))
    });
    let a = unit.clone();
    let q = quantity.clone();
    let t = total.clone();
    let c = currency_control.clone();
    quantity.connect_value_changed(move |_| {
        update_total_preview(&a, &q, &t, &selected(model::CURRENCIES, &c))
    });
    let a = unit.clone();
    let q = quantity.clone();
    let t = total.clone();
    currency_control.connect_selected_notify(move |c| {
        update_total_preview(&a, &q, &t, &selected(model::CURRENCIES, c))
    });
    body.append(&total);
    let day = gtk::Entry::new();
    day.set_text(&initial.day);
    day.set_placeholder_text(Some("YYYY-MM-DD"));
    body.append(&field("Date · YYYY-MM-DD", &day));
    let category_names = category_options(state);
    let category = choice_owned(&category_names, &initial.category);
    body.append(&field("Category", &category));
    let kinds = ["expense", "income"];
    let kind = choice(&kinds, &initial.kind);
    body.append(&field("Transaction type", &kind));
    let method = choice(model::METHODS, &initial.method);
    body.append(&field("Payment method", &method));
    let note = gtk::Entry::new();
    note.set_text(&initial.note);
    body.append(&field("Note", &note));
    let actions = gap(Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let popup = window.clone();
    cancel.connect_clicked(move |_| popup.close());
    actions.append(&cancel);
    let save = button("Apply changes", "document-save-symbolic");
    save.add_css_class("suggested-action");
    let st = state.clone();
    let popup = window.clone();
    save.connect_clicked(move |_| {
        let price = match model::parse_minor(unit.text().as_str()) {
            Ok(value) => value,
            Err(msg) => {
                warning(&st, "Invalid unit price", &msg);
                return;
            }
        };
        let qty = quantity.value_as_int() as i64;
        let amount_minor = match model::total_minor(price, qty) {
            Ok(total) => total,
            Err(msg) => {
                warning(&st, "Invalid total", &msg);
                return;
            }
        };
        let draft = Draft {
            day: day.text().trim().to_owned(),
            kind: selected(&kinds, &kind),
            amount_minor,
            quantity: qty,
            currency: selected(model::CURRENCIES, &currency_control),
            category: selected_owned(&category_names, &category),
            description: description.text().trim().to_owned(),
            method: selected(model::METHODS, &method),
            note: note.text().to_string(),
        };
        if let Err(msg) = draft.validate() {
            warning(&st, "Check draft", &msg);
            return;
        }
        {
            let mut proposals = st.capture_drafts.borrow_mut();
            let Some((_, existing)) = proposals.get_mut(index) else {
                return;
            };
            *existing = draft;
        }
        popup.close();
        refresh(&st);
    });
    actions.append(&save);
    body.append(&actions);
    window.set_child(Some(&scroll(&body)));
    window.present();
}

/// Escape all server-controlled text; enable only paired **bold** delimiters.
/// This is NOT an HTML renderer and cannot create tags or links from AI output.
pub(super) fn safe_chat_markup(body: &str) -> String {
    let chunks = body.split("**").collect::<Vec<_>>();
    let mut rendered = String::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let escaped = glib::markup_escape_text(chunk);
        if index % 2 == 1 && index + 1 < chunks.len() {
            rendered.push_str("<b>");
            rendered.push_str(&escaped);
            rendered.push_str("</b>");
        } else {
            rendered.push_str(&escaped);
        }
    }
    rendered
}

pub(super) fn keyring_controls(state: &Rc<State>) -> adw::PreferencesGroup {
    let provider = state.ai_provider.borrow().clone();
    let provider_name = provider_label(&provider);
    let status = match state.key_state.get() {
        KeyState::Loading => format!("Checking secure storage for {provider_name}…"),
        KeyState::Missing => format!("No {} key stored", provider_name),
        KeyState::Stored => format!("{} key stored securely · ••••••••••••", provider_name),
        KeyState::Busy => "Updating secure storage…".into(),
        KeyState::Error => "Keyring unavailable — unlock and retry".into(),
    };
    let group = preference_group("AI Keys", &status);

    let provider_row = preference_action(
        "Provider",
        "Choose which provider Saifori should use for remote AI features.",
    );
    let chooser = gtk::DropDown::from_strings(&AI_PROVIDER_LABELS);
    let index = AI_PROVIDER_IDS
        .iter()
        .position(|id| *id == provider)
        .unwrap_or(0);
    chooser.set_selected(index as u32);
    chooser.set_sensitive(
        !state.privacy.get()
            && !matches!(state.key_state.get(), KeyState::Loading | KeyState::Busy),
    );
    let st = state.clone();
    chooser.connect_selected_notify(move |dropdown| {
        let chosen = AI_PROVIDER_IDS
            .get(dropdown.selected() as usize)
            .copied()
            .unwrap_or("deepseek");
        if st.ai_provider.borrow().as_str() == chosen {
            return;
        }
        let result = { st.db.borrow_mut().set_setting("ai_provider", chosen) };
        match result {
            Ok(()) => {
                *st.ai_provider.borrow_mut() = chosen.to_owned();
                st.api_key.borrow_mut().clear();
                st.key_state.set(KeyState::Loading);
                refresh(&st);
                load_secret(&st);
            }
            Err(err) => warning(&st, "Settings error", &err.to_string()),
        }
    });
    provider_row.add_suffix(&chooser);
    group.add(&provider_row);

    let entry_row = preference_action("API key", "The stored key is never displayed.");
    let api_key = gtk::PasswordEntry::new();
    api_key.set_show_peek_icon(false);
    api_key.set_width_chars(24);
    api_key.set_hexpand(true);
    api_key.set_placeholder_text(Some(&provider_placeholder(&provider)));
    api_key.set_sensitive(
        !state.privacy.get()
            && !matches!(state.key_state.get(), KeyState::Loading | KeyState::Busy),
    );
    entry_row.add_suffix(&api_key);
    let primary_label = if state.key_state.get() == KeyState::Stored {
        "Save update"
    } else {
        "Save key"
    };
    let save = button(primary_label, "document-save-symbolic");
    save.set_sensitive(
        !state.privacy.get()
            && !matches!(state.key_state.get(), KeyState::Loading | KeyState::Busy),
    );
    let st = state.clone();
    let candidate = api_key.clone();
    save.connect_clicked(move |_| save_secret(&st, candidate.text().to_string()));
    entry_row.add_suffix(&save);
    group.add(&entry_row);

    let action_row = preference_action("Stored credential", "Managed using GNOME Keyring.");
    if state.key_state.get() == KeyState::Stored {
        let edit = button("Edit key", "document-edit-symbolic");
        let field = api_key.clone();
        let provider_id = provider.clone();
        edit.connect_clicked(move |_| {
            field.set_placeholder_text(Some(&format!(
                "Paste replacement {} key",
                provider_label(&provider_id)
            )));
            field.grab_focus();
        });
        action_row.add_suffix(&edit);
    }
    let remove = button("Remove", "user-trash-symbolic");
    remove.add_css_class("destructive-action");
    remove.set_sensitive(!state.privacy.get() && state.key_state.get() == KeyState::Stored);
    let st = state.clone();
    remove.connect_clicked(move |_| confirm_remove_secret(&st));
    action_row.add_suffix(&remove);
    if state.key_state.get() == KeyState::Error {
        let retry = button("Retry", "view-refresh-symbolic");
        let st = state.clone();
        retry.connect_clicked(move |_| load_secret(&st));
        action_row.add_suffix(&retry);
    }
    group.add(&action_row);
    group
}

pub(super) fn chat_panel(state: &Rc<State>) -> gtk::Box {
    let conversation = card();
    conversation.set_hexpand(true);
    conversation.add_css_class("saifori-conversation");
    let chat_head = gap(Orientation::Horizontal, 8);
    chat_head.set_valign(gtk::Align::Center);
    chat_head.append(&icon("saifori-sparkle-symbolic"));
    let title = text("Saifori Assistant", "saifori-section-title");
    title.set_hexpand(true);
    chat_head.append(&title);
    let model_name = if state.api_key.borrow().is_empty() {
        "Local"
    } else {
        provider_label(state.ai_provider.borrow().as_str())
    };
    let period = state
        .chat_month
        .borrow()
        .clone()
        .unwrap_or_else(|| month(state));
    let status = text(&format!("{model_name} · {period}"), "saifori-muted");
    chat_head.append(&status);
    let tools = gap(Orientation::Horizontal, 8);
    let new_chat = button("New chat", "list-add-symbolic");
    new_chat.add_css_class("flat");
    let st = state.clone();
    new_chat.connect_clicked(move |_| {
        if st.chat_busy.get() {
            return;
        }
        st.active_chat.set(None);
        st.messages.borrow_mut().clear();
        *st.chat_month.borrow_mut() = None;
        st.assistant_view.set(0);
        refresh(&st);
    });
    tools.append(&new_chat);
    let history = gtk::MenuButton::new();
    history.set_label(&i18n::tr("History"));
    let pop = gtk::Popover::new();
    let history_list = gap(Orientation::Vertical, 4);
    margin(&history_list, 10);
    match state.db.borrow().chat_sessions(30) {
        Ok(items) if items.is_empty() => {
            history_list.append(&text("No saved conversations", "saifori-muted"))
        }
        Ok(items) => {
            for (id, title, m, _) in items {
                let item = button(&format!("{} · {}", m, masked(state, &title)), "");
                item.add_css_class("flat");
                let st = state.clone();
                item.connect_clicked(move |_| open_chat_session(&st, id));
                history_list.append(&item);
            }
        }
        Err(err) => history_list.append(&text(&format!("{err}"), "saifori-negative")),
    }
    pop.set_child(Some(&history_list));
    history.set_popover(Some(&pop));
    tools.append(&history);
    let delete = button("", "user-trash-symbolic");
    delete.add_css_class("flat");
    delete.set_tooltip_text(Some("Delete this conversation"));
    delete.set_sensitive(state.active_chat.get().is_some() && !state.chat_busy.get());
    let st = state.clone();
    delete.connect_clicked(move |_| delete_open_chat(&st));
    tools.append(&delete);
    chat_head.append(&tools);
    conversation.append(&chat_head);
    conversation.append(&line());

    let messages = gap(Orientation::Vertical, 12);
    messages.add_css_class("saifori-messages");
    if state.messages.borrow().is_empty() {
        let empty = gap(Orientation::Vertical, 10);
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        empty.set_vexpand(true);
        let mark = icon("saifori-sparkle-symbolic");
        mark.set_pixel_size(36);
        empty.append(&mark);
        empty.append(&text("Ask about your spending", "saifori-section-title"));
        messages.append(&empty);
    }
    for message in state.messages.borrow().iter() {
        let row = gap(Orientation::Horizontal, 0);
        row.set_halign(if message.mine {
            gtk::Align::End
        } else {
            gtk::Align::Start
        });
        let bubble = gap(Orientation::Vertical, 0);
        bubble.add_css_class("saifori-chat-bubble");
        bubble.add_css_class(if message.mine {
            "saifori-chat-mine"
        } else {
            "saifori-chat-assistant"
        });
        bubble.set_size_request(160, -1);
        let body = if state.privacy.get() {
            "••••".to_owned()
        } else {
            message.body.clone()
        };
        // GTK labels do not render Markdown. Parse only bold markers into safe
        // escaped Pango markup. Never treat server text as trusted markup.
        let message_label = gtk::Label::new(None);
        message_label.set_markup(&safe_chat_markup(&body));
        message_label.set_direction(gtk::TextDirection::Ltr);
        message_label.set_xalign(0.0);
        message_label.set_selectable(!state.privacy.get());
        message_label.set_wrap(true);
        message_label.set_max_width_chars(50);
        bubble.append(&message_label);
        row.append(&bubble);
        messages.append(&row);
    }
    if state.chat_busy.get() {
        let busy = text("Thinking…", "saifori-muted");
        messages.append(&busy);
    }
    let chat_scroll = scroll(&messages);
    chat_scroll.set_vexpand(true);
    chat_scroll.set_min_content_height(230);
    chat_scroll.add_css_class("saifori-chat-scroll");
    let to_bottom = chat_scroll.vadjustment();
    chat_scroll.connect_map(move |_| {
        let adj = to_bottom.clone();
        glib::idle_add_local_once(move || adj.set_value((adj.upper() - adj.page_size()).max(0.0)));
    });
    conversation.append(&chat_scroll);
    let prompt_row = gap(Orientation::Horizontal, 8);
    prompt_row.add_css_class("saifori-prompt-row");
    for suggestion in ["Monthly summary", "Top categories", "Budget tips"] {
        let example = button(suggestion, "");
        example.add_css_class("flat");
        example.add_css_class("saifori-prompt");
        let st = state.clone();
        let msg = i18n::tr(suggestion);
        example.connect_clicked(move |_| send_chat_message(&st, msg.clone()));
        prompt_row.append(&example);
    }
    conversation.append(&prompt_row);
    let composer = gap(Orientation::Horizontal, 10);
    composer.add_css_class("saifori-composer");
    let entry = gtk::Entry::new();
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some(&i18n::tr("Ask a question…")));
    entry.set_sensitive(!state.chat_busy.get());
    let send = button("Send", "mail-send-symbolic");
    send.add_css_class("suggested-action");
    send.set_sensitive(!state.chat_busy.get());
    let st = state.clone();
    let field = entry.clone();
    send.connect_clicked(move |_| {
        let prompt = field.text().to_string();
        if ai::capture_word_count(&prompt) <= ai::MAX_CAPTURE_WORDS
            && prompt.chars().count() <= ai::MAX_CAPTURE_CHARS
        {
            field.set_text("");
        }
        send_chat_message(&st, prompt);
    });
    let st = state.clone();
    entry.connect_activate(move |e| {
        let prompt = e.text().to_string();
        if ai::capture_word_count(&prompt) <= ai::MAX_CAPTURE_WORDS
            && prompt.chars().count() <= ai::MAX_CAPTURE_CHARS
        {
            e.set_text("");
        }
        send_chat_message(&st, prompt);
    });
    composer.append(&entry);
    composer.append(&send);
    conversation.append(&composer);
    conversation.set_vexpand(true);
    conversation
}

pub(super) fn capture_panel(state: &Rc<State>) -> gtk::Box {
    let capture = card();
    capture.add_css_class("saifori-smart-workspace");
    let top = gap(Orientation::Horizontal, 10);
    top.set_valign(gtk::Align::Center);
    let heading = section_heading("Smart Capture", "");
    heading.set_hexpand(true);
    top.append(&heading);
    let history = gtk::MenuButton::new();
    history.set_label("History");
    history.set_tooltip_text(Some("Restore a previous capture review"));
    let popover = gtk::Popover::new();
    let previous = gap(Orientation::Vertical, 5);
    margin(&previous, 10);
    match state.db.borrow().capture_reviews() {
        Ok(items) if items.is_empty() => {
            previous.append(&text("No saved captures", "saifori-muted"))
        }
        Ok(items) => {
            for (id, source, when, preview) in items {
                let label = format!("{} · {} · {}", when, source, preview);
                let b = button(&masked(state, &label), "");
                b.add_css_class("flat");
                let st = state.clone();
                b.connect_clicked(move |_| {
                    let loaded = { st.db.borrow().load_capture_review(id) };
                    match loaded {
                        Ok((original, drafts)) => {
                            *st.capture_input.borrow_mut() = original;
                            *st.capture_drafts.borrow_mut() =
                                drafts.into_iter().map(|d| (true, d)).collect();
                            st.capture_scroll_position.set(0.0);
                            st.assistant_view.set(1);
                            refresh(&st);
                        }
                        Err(err) => warning(&st, "Cannot restore capture", &err.to_string()),
                    }
                });
                previous.append(&b);
            }
        }
        Err(err) => previous.append(&text(&err.to_string(), "saifori-negative")),
    }
    popover.set_child(Some(&previous));
    history.set_popover(Some(&popover));
    top.append(&history);
    capture.append(&top);
    let composer = gap(Orientation::Vertical, 6);
    composer.add_css_class("saifori-capture-composer");
    composer.append(&text(
        "Enter expenses, dates and quantities",
        "saifori-muted",
    ));
    let input = gtk::TextView::new();
    input.set_wrap_mode(gtk::WrapMode::WordChar);
    input.set_top_margin(9);
    input.set_bottom_margin(9);
    input.set_left_margin(9);
    input.set_right_margin(9);
    if state.privacy.get() {
        input.buffer().set_text("••••");
        input.set_editable(false);
        input.set_cursor_visible(false);
    } else {
        input.buffer().set_text(&state.capture_input.borrow());
    }
    let st = state.clone();
    input.buffer().connect_changed(move |buffer| {
        if !st.privacy.get() {
            *st.capture_input.borrow_mut() = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
        }
    });
    let input_scroll = scroll(&input);
    input_scroll.set_min_content_height(98);
    input_scroll.set_max_content_height(145);
    input_scroll.set_propagate_natural_height(true);
    input_scroll.set_vexpand(false);
    input_scroll.add_css_class("saifori-capture-editor");
    composer.append(&input_scroll);
    let count_row = gap(Orientation::Horizontal, 6);
    count_row.set_halign(gtk::Align::End);
    let counter = text(
        &format!(
            "{} / {} {}",
            ai::capture_word_count(&state.capture_input.borrow()),
            ai::MAX_CAPTURE_WORDS,
            i18n::tr("words")
        ),
        "saifori-muted",
    );
    if ai::validate_capture_input(&state.capture_input.borrow()).is_err()
        && !state.capture_input.borrow().is_empty()
    {
        counter.add_css_class("saifori-negative");
    }
    count_row.append(&counter);
    composer.append(&count_row);
    let word_counter = counter.clone();
    let st = state.clone();
    input.buffer().connect_changed(move |buffer| {
        if st.privacy.get() {
            return;
        }
        let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let words = ai::capture_word_count(value.as_str());
        word_counter.set_text(&format!(
            "{} / {} {}",
            words,
            ai::MAX_CAPTURE_WORDS,
            i18n::tr("words")
        ));
        if words > ai::MAX_CAPTURE_WORDS || value.chars().count() > ai::MAX_CAPTURE_CHARS {
            word_counter.add_css_class("saifori-negative");
        } else {
            word_counter.remove_css_class("saifori-negative");
        }
    });
    let capture_actions = gap(Orientation::Horizontal, 8);
    let local = button("Review locally", "edit-find-symbolic");
    local.set_sensitive(!state.privacy.get());
    let st = state.clone();
    local.connect_clicked(move |_|{
        let raw=st.capture_input.borrow().clone();
        if let Err(err)=ai::validate_capture_input(&raw){warning(&st,"Input limit",&err.to_string());return;}
        let drafts=ai::capture_text(&raw,&currency(&st));
        if drafts.is_empty(){warning(&st,"No drafts","No valid expenses were found. Check the date, amount and quantity.");return;}
        if st.capture_drafts.borrow().len()+drafts.len()>ai::MAX_CAPTURE_DRAFTS {
            warning(&st,"Draft limit","A maximum of 50 drafts may be reviewed at once. Save or remove existing drafts, or split the input into smaller batches.");return;
        }
        let history_result=st.db.borrow_mut().save_capture_review("local",&raw,&drafts);
        if let Err(err)=history_result {warning(&st,"Capture history",&err.to_string());}
        st.capture_drafts.borrow_mut().extend(drafts.into_iter().map(|d|(true,d)));
        st.assistant_view.set(1);refresh(&st);
    });
    capture_actions.append(&local);
    let extract = button("AI extract", "saifori-sparkle-symbolic");
    extract.set_sensitive(!state.privacy.get());
    let st = state.clone();
    extract.connect_clicked(move |control|{
        if matches!(st.key_state.get(),KeyState::Loading|KeyState::Busy){
            warning(&st,"Keyring busy","Please wait until secure storage is ready.");return;
        }
        let key=st.api_key.borrow().clone();
        let provider=st.ai_provider.borrow().clone();
        if key.is_empty(){warning(&st,"API key needed",&format!("Save a {} API key first.",provider_label(&provider)));return;}
        let raw=st.capture_input.borrow().clone();
        if let Err(err)=ai::validate_capture_input(&raw){warning(&st,"Input limit",&err.to_string());return;}
        if st.capture_drafts.borrow().len()>=ai::MAX_CAPTURE_DRAFTS {
            warning(&st,"Draft limit","Review and save existing drafts before extracting more (maximum 50).");return;
        }
        control.set_sensitive(false);
        st.capture_busy.set(true);
        let (sender,receiver)=std::sync::mpsc::channel();let cur=currency(&st);
        let cats=category_options(&st);
        std::thread::spawn(move ||{let _=sender.send(ai::extract_with_ai(&provider,&key,&raw,&cur,&cats).map_err(|e|e.to_string()));});
        let btn=control.clone();let current=st.clone();
        glib::timeout_add_local(Duration::from_millis(90),move ||{
            match receiver.try_recv() {
                Ok(Ok(drafts))=>{
                    current.capture_busy.set(false);
                    if current.capture_drafts.borrow().len()+drafts.len()>ai::MAX_CAPTURE_DRAFTS {
                        btn.set_sensitive(true);
                        warning(&current,"Draft limit","The result would exceed 50 review drafts. Save or remove existing drafts and try a smaller batch.");
                    }else{
                        let raw=current.capture_input.borrow().clone();
                        if let Err(err)=current.db.borrow_mut().save_capture_review("deepseek",&raw,&drafts){
                            warning(&current,"Capture history",&err.to_string());
                        }
                        current.capture_drafts.borrow_mut().extend(drafts.into_iter().map(|d|(true,d)));
                        current.assistant_view.set(1);refresh(&current);
                    }
                    glib::ControlFlow::Break
                }
                Ok(Err(err))=>{current.capture_busy.set(false);btn.set_sensitive(true);warning(&current,"AI extraction failed",&err);glib::ControlFlow::Break},
                Err(std::sync::mpsc::TryRecvError::Empty)=>glib::ControlFlow::Continue,
                Err(_)=>{current.capture_busy.set(false);btn.set_sensitive(true);glib::ControlFlow::Break},
            }
        });
    });
    capture_actions.append(&extract);
    composer.append(&capture_actions);
    capture.append(&composer);
    let saved = state.capture_drafts.borrow().clone();
    let drafts_section = gap(Orientation::Vertical, 6);
    drafts_section.set_hexpand(true);
    drafts_section.set_vexpand(true);
    if !saved.is_empty() {
        let count = saved.iter().filter(|(enabled, _)| *enabled).count();
        let total: i64 = saved
            .iter()
            .filter(|(enabled, d)| *enabled && d.kind == "expense" && d.currency == currency(state))
            .map(|(_, d)| d.amount_minor)
            .sum();
        drafts_section.append(&text(
            &format!(
                "{}: {} · {}",
                i18n::tr("Review drafts"),
                count,
                masked_money(state, total, &currency(state))
            ),
            "saifori-section-title",
        ));
        type CaptureMonths =
            BTreeMap<(String, String), BTreeMap<String, Vec<(usize, bool, Draft)>>>;
        let mut months: CaptureMonths = BTreeMap::new();
        for (index, (enabled, draft)) in saved.into_iter().enumerate() {
            let m = draft.day.get(..7).unwrap_or("Unknown").to_string();
            months
                .entry((m, draft.currency.clone()))
                .or_default()
                .entry(draft.day.clone())
                .or_default()
                .push((index, enabled, draft));
        }
        let timeline_nodes: TimelineNodes = timeline_nodes();
        let tools = gap(Orientation::Horizontal, 12);
        let total_label = text(
            &format!("{} {}", count, i18n::tr("selected")),
            "saifori-muted",
        );
        let selection_total_label = total_label.clone();
        total_label.set_hexpand(true);
        tools.append(&total_label);
        tools.append(&timeline_controls(state, &timeline_nodes));
        drafts_section.append(&tools);
        let group_list = gap(Orientation::Vertical, 8);
        for (month_index, ((m, cur), days)) in months.iter().rev().enumerate() {
            let group_expense: i64 = days
                .values()
                .flat_map(|items| items.iter())
                .filter(|(_, _, d)| d.kind == "expense")
                .map(|(_, _, d)| d.amount_minor)
                .sum();
            let group_income: i64 = days
                .values()
                .flat_map(|items| items.iter())
                .filter(|(_, _, d)| d.kind == "income")
                .map(|(_, _, d)| d.amount_minor)
                .sum();
            let day_list = gap(Orientation::Vertical, 4);
            day_list.add_css_class("saifori-timeline-days");
            for (day_index, (day, records)) in days.iter().rev().enumerate() {
                let sum: i64 = records
                    .iter()
                    .filter(|(_, _, d)| d.kind == "expense")
                    .map(|(_, _, d)| d.amount_minor)
                    .sum();
                let rows = gap(Orientation::Vertical, 5);
                for (index, enabled, draft) in records {
                    let item = gap(Orientation::Vertical, 5);
                    item.add_css_class("saifori-draft");
                    let row = gap(Orientation::Horizontal, 8);
                    row.set_valign(gtk::Align::Center);
                    let checkbox = gtk::CheckButton::new();
                    checkbox.set_active(*enabled);
                    let st = state.clone();
                    let ix = *index;
                    let selected_count = selection_total_label.clone();
                    checkbox.connect_toggled(move |widget| {
                        if let Some((on, _)) = st.capture_drafts.borrow_mut().get_mut(ix) {
                            *on = widget.is_active();
                        }
                        let count = st
                            .capture_drafts
                            .borrow()
                            .iter()
                            .filter(|(active, _)| *active)
                            .count();
                        selected_count.set_text(&format!("{} {}", count, i18n::tr("selected")));
                    });
                    row.append(&checkbox);
                    let title = text(
                        &masked(state, &draft.description),
                        "saifori-row-description",
                    );
                    title.set_hexpand(true);
                    row.append(&title);
                    let meta = text(
                        &masked(
                            state,
                            &format!(
                                "{} × {} · {}",
                                draft.quantity,
                                model::money(draft.amount_minor / draft.quantity, &draft.currency),
                                draft.category
                            ),
                        ),
                        "saifori-muted",
                    );
                    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    meta.set_hexpand(true);
                    row.append(&meta);
                    let amt = text(
                        &masked_money(state, draft.amount_minor, &draft.currency),
                        "saifori-amount",
                    );
                    amt.set_wrap(false);
                    row.append(&amt);
                    let edit = button("", "document-edit-symbolic");
                    edit.add_css_class("flat");
                    edit.set_tooltip_text(Some("Edit draft"));
                    let st = state.clone();
                    let ix = *index;
                    edit.connect_clicked(move |_| edit_capture_draft(&st, ix));
                    row.append(&edit);
                    let remove = button("", "user-trash-symbolic");
                    remove.add_css_class("flat");
                    remove.set_tooltip_text(Some("Remove draft"));
                    let st = state.clone();
                    let ix = *index;
                    remove.connect_clicked(move |_| {
                        if ix < st.capture_drafts.borrow().len() {
                            st.capture_drafts.borrow_mut().remove(ix);
                        }
                        refresh(&st);
                    });
                    row.append(&remove);
                    item.append(&row);
                    rows.append(&item);
                }
                let key = format!("draft:day:{cur}:{day}");
                let info = format!("{} · {}", records.len(), masked_money(state, sum, cur));
                day_list.append(&timeline_group(
                    state,
                    TimelineGroup {
                        key,
                        title: friendly_day(day),
                        detail: info,
                        level: TimelineLevel::Day,
                        default_open: month_index == 0 && day_index == 0,
                    },
                    &rows,
                    &timeline_nodes,
                ));
            }
            let key = format!("draft:month:{m}:{cur}");
            let info = format!(
                "{} {}  ·  {} {}",
                i18n::tr("Out"),
                masked_money(state, group_expense, cur),
                i18n::tr("In"),
                masked_money(state, group_income, cur)
            );
            group_list.append(&timeline_group(
                state,
                TimelineGroup {
                    key,
                    title: i18n::display_month(m),
                    detail: info,
                    level: TimelineLevel::Month,
                    default_open: month_index == 0,
                },
                &day_list,
                &timeline_nodes,
            ));
        }
        let draft_scroll = scroll(&group_list);
        draft_scroll.add_css_class("saifori-capture-review-scroll");
        draft_scroll.set_min_content_height(320);
        draft_scroll.set_vexpand(true);
        let position = state.capture_scroll_position.get();
        let adj = draft_scroll.vadjustment();
        let st = state.clone();
        adj.connect_value_changed(move |a| st.capture_scroll_position.set(a.value()));
        let restore = draft_scroll.vadjustment();
        draft_scroll.connect_map(move |_| {
            let max = (restore.upper() - restore.page_size()).max(0.0);
            restore.set_value(position.min(max));
        });
        drafts_section.append(&draft_scroll);
        let save = button("Save selected", "document-save-symbolic");
        save.add_css_class("suggested-action");
        save.set_sensitive(!state.privacy.get());
        save.set_halign(gtk::Align::End);
        let st = state.clone();
        save.connect_clicked(move |_| {
            let chosen: Vec<Draft> = st
                .capture_drafts
                .borrow()
                .iter()
                .filter(|(enabled, _)| *enabled)
                .map(|(_, draft)| draft.clone())
                .collect();
            if chosen.is_empty() {
                warning(
                    &st,
                    "Nothing selected",
                    "Select at least one reviewed transaction.",
                );
                return;
            }
            let result = st.db.borrow_mut().add_many(&chosen);
            if run_write(&st, result) {
                st.capture_drafts.borrow_mut().clear();
                st.capture_input.borrow_mut().clear();
                refresh(&st);
            }
        });
        drafts_section.append(&save);
    } else {
        drafts_section.append(&text(
            "Extract expenses to review before saving.",
            "saifori-muted",
        ));
    }
    capture.append(&drafts_section);
    capture.set_vexpand(true);
    capture.add_css_class("saifori-capture");
    capture
}

pub(super) fn assistant(state: &Rc<State>, parent: &gtk::Box) {
    let title = gap(Orientation::Horizontal, 12);
    let label = text("AI Assistant", "saifori-title");
    label.set_hexpand(true);
    title.append(&label);
    let settings = button("", "preferences-system-symbolic");
    settings.add_css_class("flat");
    settings.set_tooltip_text(Some("Assistant settings"));
    let st = state.clone();
    settings.connect_clicked(move |_| change_page(&st, Page::Settings));
    title.append(&settings);
    parent.append(&title);
    let conversation = chat_panel(state);
    let capture = capture_panel(state);
    if state.assistant_tabs.get() {
        let stack = gtk::Stack::new();
        stack.set_hexpand(true);
        stack.set_vexpand(true);
        stack.add_titled(&conversation, Some("chat"), &i18n::tr("Chat"));
        stack.add_titled(&capture, Some("capture"), &i18n::tr("Smart Capture"));
        stack.set_visible_child_name(if state.assistant_view.get() == 1 {
            "capture"
        } else {
            "chat"
        });
        let tabs = gtk::StackSwitcher::new();
        tabs.set_stack(Some(&stack));
        tabs.set_halign(gtk::Align::Start);
        parent.append(&tabs);
        let st = state.clone();
        stack.connect_visible_child_name_notify(move |stack| {
            st.assistant_view.set(
                if stack.visible_child_name().as_deref() == Some("capture") {
                    1
                } else {
                    0
                },
            );
        });
        parent.append(&stack);
    } else {
        let layout = gap(Orientation::Horizontal, 14);
        layout.set_vexpand(true);
        conversation.set_hexpand(true);
        capture.set_width_request(480);
        layout.append(&conversation);
        layout.append(&capture);
        parent.append(&layout);
    }
}

// Account-to-account movement is a separate ledger; it never inflates income or spending.
