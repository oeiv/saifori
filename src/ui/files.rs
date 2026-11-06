use super::*;

const MAX_BACKUP_BYTES: usize = 40_000_000;

// Keep GTK on its main context and bound file reads before allocating more memory.
fn background<T: Send + 'static>(
    state: &Rc<State>,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    done: impl FnOnce(&Rc<State>, Result<T, String>) + 'static,
) {
    state.window.set_sensitive(false);
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    let state = state.clone();
    let mut done = Some(done);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let result = match receiver.try_recv() {
            Ok(value) => value,
            Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
            Err(_) => Err("The background operation stopped unexpectedly.".into()),
        };
        state.window.set_sensitive(true);
        if let Some(done) = done.take() {
            done(&state, result);
        }
        glib::ControlFlow::Break
    });
}

pub(super) fn save_file(state: &Rc<State>, title: &str, filename: &str, bytes: Vec<u8>) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title(title);
    dialog.set_initial_name(Some(filename));
    let st = state.clone();
    dialog.save(
        Some(&state.window),
        None::<&gio::Cancellable>,
        move |result| {
            match result {
                Ok(file) => {
                    background(
                        &st,
                        move || {
                            file.replace_contents(
                                &bytes,
                                None,
                                false,
                                gio::FileCreateFlags::REPLACE_DESTINATION
                                    | gio::FileCreateFlags::PRIVATE,
                                None::<&gio::Cancellable>,
                            )
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                        },
                        |state, result| {
                            if let Err(err) = result {
                                warning(state, "Could not write file", &err);
                            }
                        },
                    );
                }
                Err(err) => {
                    // User-cancelled portal chooser is not an error.
                    if !err.matches(gio::IOErrorEnum::Cancelled) {
                        warning(&st, "Export cancelled", &err.to_string());
                    }
                }
            }
        },
    );
}

pub(super) fn choose_backup(state: &Rc<State>) {
    if state.test_mode.get() || state.chat_busy.get() || state.capture_busy.get() {
        warning(
            state,
            "Operation in progress",
            "Finish the current operation before restoring data.",
        );
        return;
    }
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Choose a Saifori JSON backup");
    let st = state.clone();
    dialog.open(
        Some(&state.window),
        None::<&gio::Cancellable>,
        move |result| match result {
            Ok(file) => background(
                &st,
                move || {
                    let stream = file
                        .read(None::<&gio::Cancellable>)
                        .map_err(|e| e.to_string())?;
                    crate::file_io::read_limited(stream.into_read(), MAX_BACKUP_BYTES)
                        .map_err(|e| e.to_string())
                },
                |state, result| match result {
                    Ok(bytes) => confirm_restore(state, bytes),
                    Err(err) => warning(state, "Could not read backup", &err),
                },
            ),
            Err(err) => {
                if !err.matches(gio::IOErrorEnum::Cancelled) {
                    warning(&st, "Could not choose backup", &err.to_string());
                }
            }
        },
    );
}

fn confirm_restore(state: &Rc<State>, bytes: Vec<u8>) {
    let popup = gtk::Window::builder()
        .title("Replace All Local Data?")
        .transient_for(&state.window)
        .modal(true)
        .resizable(false)
        .default_width(450)
        .build();
    let content = gap(Orientation::Vertical, 17);
    margin(&content, 24);
    content.append(&text("Restore backup?", "saifori-title"));
    content.append(&text("This replaces all local transactions, budgets and the default currency. Export your existing data first.","saifori-negative"));
    let actions = gap(Orientation::Horizontal, 12);
    actions.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let w = popup.clone();
    cancel.connect_clicked(move |_| w.close());
    actions.append(&cancel);
    let approve = button("Replace Local Data", "document-open-symbolic");
    approve.add_css_class("destructive-action");
    let st = state.clone();
    let w = popup.clone();
    approve.connect_clicked(move |_| {
        let path = st.real_db_path.clone();
        let bytes = bytes.clone();
        w.close();
        background(
            &st,
            move || {
                Store::open(&path)
                    .and_then(|mut store| store.restore_json(&bytes))
                    .map_err(|e| e.to_string())
            },
            |state, result| match result {
                Ok(count) => {
                    if let Ok(value) = state.db.borrow().currency() {
                        *state.currency.borrow_mut() = value;
                    }
                    state.selected.set(None);
                    refresh(state);
                    warning(
                        state,
                        "Restore complete",
                        &format!(
                            "Restored {count} transactions. Review your balances and budgets."
                        ),
                    );
                }
                Err(err) => warning(state, "Restore failed — data unchanged", &err),
            },
        );
    });
    actions.append(&approve);
    content.append(&actions);
    popup.set_child(Some(&content));
    popup.present();
}

pub(super) fn export_data(
    state: &Rc<State>,
    title: &'static str,
    filename: &'static str,
    export: fn(&Store) -> anyhow::Result<Vec<u8>>,
) {
    if state.test_mode.get() {
        return;
    }
    let path = state.real_db_path.clone();
    background(
        state,
        move || {
            Store::open(&path)
                .and_then(|store| export(&store))
                .map_err(|e| e.to_string())
        },
        move |state, result| match result {
            Ok(bytes) => save_file(state, title, filename, bytes),
            Err(err) => warning(state, "Export failed", &err),
        },
    );
}
