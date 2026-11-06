mod editor;
use editor::*;
mod ledger;
use ledger::*;
mod reports;
use reports::*;
mod assistant;
mod files;
mod settings;
use assistant::*;
use files::*;
use settings::*;

use crate::{
    ai, charts,
    db::Store,
    demo, i18n, insights,
    model::{self, Draft, Transaction},
    secret_store,
};
use adw::prelude::*;
use chrono::Datelike;
use gtk::glib::value::ToValue;
use gtk::{self, gio, glib, Orientation};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Transactions,
    TransactionEditor,
    Transfers,
    Activity,
    Budgets,
    Analytics,
    Products,
    Assistant,
    Settings,
    Help,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Loading,
    Missing,
    Stored,
    Busy,
    Error,
}

#[derive(Clone)]
struct Message {
    mine: bool,
    body: String,
}

const AI_PROVIDER_IDS: [&str; 3] = ["deepseek", "claude", "openai"];
const AI_PROVIDER_LABELS: [&str; 3] = ["DeepSeek", "Claude", "GPT / OpenAI"];

fn valid_ai_provider(value: &str) -> bool {
    AI_PROVIDER_IDS.contains(&value)
}
fn normalized_ai_provider(value: &str) -> String {
    let v = value.trim().to_lowercase();
    if valid_ai_provider(&v) {
        v
    } else {
        "deepseek".into()
    }
}
fn provider_label(value: &str) -> &'static str {
    match value {
        "deepseek" => "DeepSeek",
        "claude" => "Claude",
        "openai" => "GPT / OpenAI",
        _ => "DeepSeek",
    }
}
fn provider_placeholder(value: &str) -> String {
    format!("Paste new {} key", provider_label(value))
}

struct State {
    db: RefCell<Store>,
    real_db_path: std::path::PathBuf,
    test_mode: Cell<bool>,
    capture_busy: Cell<bool>,
    window: adw::ApplicationWindow,
    page: Cell<Page>,
    month: RefCell<String>,
    currency: RefCell<String>,
    selected: Cell<Option<i64>>,
    editing_transaction: Cell<Option<i64>>,
    transaction_as_page: Cell<bool>,
    filter: RefCell<String>,
    tx_kind: Cell<u32>,
    tx_range: Cell<u32>,
    tx_category: Cell<u32>,
    tx_day: RefCell<Option<String>>,
    collapsed_groups: RefCell<HashSet<String>>,
    expanded_groups: RefCell<HashSet<String>>,
    overview_range: Cell<u32>,
    annual_year: Cell<i32>,
    privacy: Cell<bool>,
    language: RefCell<String>,
    ai_provider: RefCell<String>,
    // Only a session copy of the system-keyring secret; never persisted in SQLite.
    api_key: RefCell<String>,
    key_state: Cell<KeyState>,
    chat_month: RefCell<Option<String>>,
    messages: RefCell<Vec<Message>>,
    active_chat: Cell<Option<i64>>,
    assistant_tabs: Cell<bool>,
    assistant_view: Cell<u32>,
    product_period: Cell<u32>,
    chat_busy: Cell<bool>,
    capture_input: RefCell<String>,
    capture_drafts: RefCell<Vec<(bool, Draft)>>,
    capture_scroll_position: Cell<f64>,
    transfer_compact_months: Cell<bool>,
    transaction_scroll_position: Cell<f64>,
}

pub fn launch() {
    let app = adw::Application::builder()
        .application_id(model::APP_ID)
        .build();
    app.connect_activate(|app| {
        if let Some(win) = app.active_window() {
            win.present();
            return;
        }
        let data_dir = glib::user_data_dir().join("saifori");
        if let Err(err) = private_directory(&data_dir) {
            show_startup_error(app, &format!("Cannot prepare private storage: {err}"));
            return;
        }
        let db_path = data_dir.join("ledger.sqlite3");
        let real_db = match Store::open(&db_path) {
            Ok(db) => db,
            Err(err) => {
                show_startup_error(app, &format!("Cannot open database: {err:#}"));
                return;
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
        }
        // The user's ledger is opened first to read persistent preferences. Demo
        // transactions live exclusively in an ephemeral in-memory database.
        let test_mode = real_db.setting("test_mode").ok().flatten().as_deref() == Some("1");
        let compact_months = real_db
            .setting("transfer_compact_months")
            .ok()
            .flatten()
            .as_deref()
            == Some("1");
        let assistant_tabs =
            real_db.setting("assistant_tabs").ok().flatten().as_deref() != Some("0");
        let language = real_db
            .setting("language")
            .ok()
            .flatten()
            .filter(|v| i18n::valid(v))
            .unwrap_or_else(i18n::system_language);
        let ai_provider = normalized_ai_provider(
            &real_db
                .setting("ai_provider")
                .ok()
                .flatten()
                .unwrap_or_else(|| "deepseek".into()),
        );
        let privacy = real_db.setting("privacy").ok().flatten().as_deref() == Some("1");
        let transaction_as_page = real_db
            .setting("transaction_page")
            .ok()
            .flatten()
            .as_deref()
            == Some("1");
        let db = if test_mode {
            match demo::build(chrono::Local::now().date_naive()) {
                Ok(data) => data,
                Err(err) => {
                    show_startup_error(app, &format!("Cannot prepare Test Mode: {err:#}"));
                    return;
                }
            }
        } else {
            real_db
        };
        let prior_session = db
            .chat_sessions(1)
            .ok()
            .and_then(|rows| rows.first().map(|r| r.0));
        let prior_messages = prior_session
            .and_then(|id| db.chat_messages(id).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(mine, body)| Message { mine, body })
            .collect::<Vec<_>>();
        let prior_month = prior_session.and_then(|id| db.chat_month(id).ok().flatten());
        let currency = if test_mode {
            "USD".to_string()
        } else {
            db.currency().unwrap_or_else(|_| "MAD".into())
        };
        i18n::set_language(&language);
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Saifori")
            .default_width(1540)
            .default_height(900)
            .build();
        let state = Rc::new(State {
            db: RefCell::new(db),
            real_db_path: db_path,
            test_mode: Cell::new(test_mode),
            capture_busy: Cell::new(false),
            window,
            page: Cell::new(Page::Overview),
            month: RefCell::new(model::current_month()),
            currency: RefCell::new(currency),
            selected: Cell::new(None),
            editing_transaction: Cell::new(None),
            transaction_as_page: Cell::new(transaction_as_page),
            filter: RefCell::new(String::new()),
            tx_kind: Cell::new(0),
            tx_range: Cell::new(0),
            tx_category: Cell::new(0),
            tx_day: RefCell::new(None),
            collapsed_groups: RefCell::new(HashSet::new()),
            expanded_groups: RefCell::new(HashSet::new()),
            overview_range: Cell::new(1),
            annual_year: Cell::new(chrono::Local::now().year()),
            privacy: Cell::new(privacy),
            language: RefCell::new(language),
            ai_provider: RefCell::new(ai_provider),
            api_key: RefCell::new(String::new()),
            key_state: Cell::new(KeyState::Loading),
            chat_month: RefCell::new(prior_month),
            messages: RefCell::new(prior_messages),
            active_chat: Cell::new(prior_session),
            assistant_tabs: Cell::new(assistant_tabs),
            assistant_view: Cell::new(0),
            product_period: Cell::new(0),
            chat_busy: Cell::new(false),
            capture_input: RefCell::new(String::new()),
            capture_drafts: RefCell::new(Vec::new()),
            capture_scroll_position: Cell::new(0.0),
            transfer_compact_months: Cell::new(compact_months),
            transaction_scroll_position: Cell::new(0.0),
        });
        let css = gtk::CssProvider::new();
        css.load_from_string(include_str!("../assets/style.css"));
        if let Some(display) = gtk::gdk::Display::default() {
            // Always resolve app-owned icons using stable, absolute directories.
            // GUI launches, GNOME Builder and Flatpak do not share a working directory.
            // See register_icon_paths() and the guaranteed native fallback in icon().
            register_icon_paths(&display);
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        refresh(&state);
        state.window.present();
        load_secret(&state);
    });
    app.run();
}

fn private_directory(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn show_startup_error(app: &adw::Application, msg: &str) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Saifori — Storage Error")
        .default_width(480)
        .default_height(190)
        .build();
    let content = gtk::Box::new(Orientation::Vertical, 15);
    margin(&content, 24);
    let label = gtk::Label::new(Some(msg));
    label.set_wrap(true);
    content.append(&label);
    let button = gtk::Button::with_label("Close");
    let win = window.clone();
    button.connect_clicked(move |_| win.close());
    content.append(&button);
    window.set_content(Some(&content));
    window.present();
}

fn margin<W: IsA<gtk::Widget>>(widget: &W, value: i32) {
    widget.set_margin_start(value);
    widget.set_margin_end(value);
    widget.set_margin_top(value);
    widget.set_margin_bottom(value);
}
fn gap(orientation: Orientation, spacing: i32) -> gtk::Box {
    gtk::Box::new(orientation, spacing)
}
fn text(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(&i18n::tr(value)));
    label.set_xalign(0.0);
    label.set_wrap(true);
    // Use native Adwaita typography/color roles, reserving Saifori CSS for layout.
    match class {
        "saifori-title" => label.add_css_class("title-1"),
        "saifori-section-title" | "saifori-month-name" => label.add_css_class("heading"),
        "saifori-kpi" => label.add_css_class("title-2"),
        "saifori-muted" | "saifori-subtitle" | "saifori-empty" => label.add_css_class("dim-label"),
        "saifori-positive" | "saifori-type-income" => label.add_css_class("success"),
        "saifori-negative" | "saifori-type-expense" => label.add_css_class("error"),
        _ => {}
    }
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label
}
// Every requested icon has a native GNOME Symbolic fallback. No missing-image
// placeholder is ever intentional, even with a custom icon theme or outside
// the source directory (e.g. a Flatpak/Builder launch).
fn icon_fallback(name: &str) -> &'static str {
    match name {
        "saifori-briefcase-symbolic" => "folder-documents-symbolic",
        "saifori-budget-symbolic" => "accessories-calculator-symbolic",
        "saifori-car-symbolic" => "find-location-symbolic",
        "saifori-cart-symbolic" => "folder-symbolic",
        "saifori-chart-symbolic" => "view-list-symbolic",
        "saifori-cup-symbolic" => "applications-utilities-symbolic",
        "saifori-sparkle-symbolic" => "starred-symbolic",
        // A real GNOME icon to avoid an absent icon on unusual themes.
        _ => "dialog-information-symbolic",
    }
}

fn icon(name: &str) -> gtk::Image {
    let resolved = if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        if theme.has_icon(name) {
            name
        } else {
            let fallback = icon_fallback(name);
            if theme.has_icon(fallback) {
                fallback
            } else {
                "image-missing-symbolic"
            }
        }
    } else {
        name
    };
    let image = gtk::Image::from_icon_name(resolved);
    image.set_pixel_size(18);
    image
}

// Important: do not depend on CWD; Builder frequently starts from a build
// directory, and the Flatpak binary is installed under /app/bin/.
fn register_icon_paths(display: &gtk::gdk::Display) {
    use std::path::{Path, PathBuf};
    let theme = gtk::IconTheme::for_display(display);
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets/icons"));
    }
    if let Ok(exe) = std::env::current_exe() {
        // source tree: <project>/target/{debug,release}/saifori
        // installed bundle: .../files/bin/saifori, with icons in .../files/share/icons
        for parent in exe.ancestors().take(6) {
            candidates.push(parent.join("assets/icons"));
            candidates.push(parent.join("share/icons"));
        }
    }
    if let Some(data_dirs) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in std::env::split_paths(&data_dirs) {
            candidates.push(dir.join("icons"));
        }
    }
    // Also covers standard Flatpak installations when launched via a wrapper.
    candidates.push(PathBuf::from("/app/share/icons"));
    for dir in candidates {
        if Path::new(&dir).join("hicolor/index.theme").is_file() {
            theme.add_search_path(&dir);
        }
    }
}
fn line() -> gtk::Separator {
    gtk::Separator::new(Orientation::Horizontal)
}
fn card() -> gtk::Box {
    let b = gap(Orientation::Vertical, 12);
    // Libadwaita's official .card supplies theme-aware background, border and
    // hover behavior. Saifori's small local class only sets interior spacing.
    b.add_css_class("card");
    b.add_css_class("saifori-card");
    b
}
fn section_heading(title: &str, subtitle: &str) -> gtk::Box {
    let b = gap(Orientation::Vertical, 3);
    b.append(&text(title, "saifori-section-title"));
    if !subtitle.is_empty() {
        b.append(&text(subtitle, "saifori-muted"));
    }
    b
}
// Timeline groups share the same verified interaction rules in Transactions and
// Smart Capture, while each workspace has independent saved open/closed state.
// The single bulk-toggle observes the same registry as all individual groups.
// No two contradictory Expand / Collapse buttons are ever displayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimelineLevel {
    Month,
    Day,
}

type TimelineNodes = Rc<RefCell<TimelineRegistry>>;

struct TimelineRegistry {
    groups: Vec<(String, gtk::Box, gtk::Image)>,
    toggle_label: Option<gtk::Label>,
    toggle_icon: Option<gtk::Image>,
    toggle_button: Option<glib::WeakRef<gtk::Button>>,
}

impl TimelineRegistry {
    fn new() -> Self {
        Self {
            groups: Vec::new(),
            toggle_label: None,
            toggle_icon: None,
            toggle_button: None,
        }
    }
    fn all_open(&self) -> bool {
        !self.groups.is_empty()
            && self
                .groups
                .iter()
                .all(|(_, content, _)| content.get_visible())
    }
    fn update_toggle(&self) {
        let open = self.all_open();
        if let Some(button) = self.toggle_button.as_ref().and_then(|weak| weak.upgrade()) {
            button.set_sensitive(!self.groups.is_empty());
        }
        if let Some(label) = &self.toggle_label {
            label.set_text(&i18n::tr(if open { "Collapse all" } else { "Expand all" }));
        }
        if let Some(icon) = &self.toggle_icon {
            icon.set_icon_name(Some(if open {
                "view-restore-symbolic"
            } else {
                "view-fullscreen-symbolic"
            }));
        }
    }
}

fn timeline_nodes() -> TimelineNodes {
    Rc::new(RefCell::new(TimelineRegistry::new()))
}

// No GtkRevealer is needed: the old implementation had animations disabled.
// Hide the real GtkBox content rather than retaining a revealer in the widget
// tree. A hidden widget has zero layout allocation; headers stay compact even
// when the GtkViewport is taller than the remaining visible months.
fn timeline_set_open(content: &gtk::Box, open: bool) {
    content.set_visible(open);
}

#[derive(Clone)]
struct MonthAnchor {
    key: String,
    widget: gtk::Box,
    // Keep the actual, interactive header. It will be reparented when pinned.
    header: gtk::Button,
}
impl MonthAnchor {
    fn new(key: String, widget: gtk::Box) -> Self {
        // timeline_group() always installs the month button as its first child.
        let header = widget
            .first_child()
            .and_then(|w| w.downcast::<gtk::Button>().ok())
            .expect("month timeline must start with its header button");
        Self {
            key,
            widget,
            header,
        }
    }
}
type MonthAnchors = Rc<RefCell<Vec<MonthAnchor>>>;

/// Holds the *original* button while it is at the top of the ledger.
/// A blank, non-interactive spacer occupies its old layout space. This
/// prevents content jumping and, unlike a copied button, cannot draw twice.
struct FloatingMonth {
    header: gtk::Button,
    owner: gtk::Box,
    spacer: gtk::Box,
    height: i32,
}

fn release_floating_month(overlay: &gtk::Overlay, old: FloatingMonth) {
    overlay.remove_overlay(&old.header);
    old.owner.remove(&old.spacer);
    old.header.set_valign(gtk::Align::Fill);
    old.owner.prepend(&old.header);
}

fn float_original_month(overlay: &gtk::Overlay, month: &MonthAnchor) -> FloatingMonth {
    let height = month.header.height().max(44);
    let spacer = gtk::Box::new(Orientation::Vertical, 0);
    spacer.set_size_request(-1, height);
    spacer.set_can_target(false);
    // Keep the ledger's measurements unchanged while moving the real button.
    month.widget.prepend(&spacer);
    month.widget.remove(&month.header);
    month.header.set_halign(gtk::Align::Fill);
    month.header.set_valign(gtk::Align::Start);
    overlay.add_overlay(&month.header);
    overlay.set_clip_overlay(&month.header, true);
    FloatingMonth {
        header: month.header.clone(),
        owner: month.widget.clone(),
        spacer,
        height,
    }
}

/// A real next header pushes the pinned header out of the viewport.
/// Negative values deliberately position the overlay outside its clipped area.
fn sticky_push_offset(scroll_y: f64, next_y: Option<f64>, height: i32) -> i32 {
    next_y
        .map(|y| (y - scroll_y - f64::from(height)).clamp(-f64::from(height), 0.0) as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod sticky_month_tests {
    use super::*;
    #[test]
    fn month_pushes_without_overlapping_columns() {
        assert_eq!(sticky_push_offset(0.0, None, 44), 0);
        assert_eq!(sticky_push_offset(100.0, Some(200.0), 44), 0);
        assert_eq!(sticky_push_offset(100.0, Some(140.0), 44), -4);
        assert_eq!(sticky_push_offset(100.0, Some(121.0), 44), -23);
        assert_eq!(sticky_push_offset(100.0, Some(100.0), 44), -44);
        assert_eq!(sticky_push_offset(100.0, Some(80.0), 44), -44);
    }
}

/// Sticky/push-off month header implemented with ONE actual GtkButton per
/// month. We transfer the existing header from the scrolling ledger into a
/// clipped GtkOverlay while its place is occupied by an inert spacer. The
/// original clicked/reveal handlers and accessibility name stay attached.
/// When the next month arrives, that same button is returned to its group.
/// No duplicate button, copied text, or overlaid original is ever rendered.
fn sticky_month_ledger(
    state: &Rc<State>,
    scroller: &gtk::ScrolledWindow,
    list: &gtk::Box,
    anchors: &MonthAnchors,
    registry: &TimelineNodes,
    collapse_previous_transfers: bool,
) -> gtk::Overlay {
    let overlay = gtk::Overlay::new();
    overlay.set_hexpand(true);
    overlay.set_vexpand(true);
    // A tall viewport must not distribute unused height between month groups.
    list.set_vexpand(false);
    list.set_valign(gtk::Align::Start);
    overlay.set_child(Some(scroller));

    let shift = Rc::new(Cell::new(0_i32));
    let floating: Rc<RefCell<Option<FloatingMonth>>> = Rc::new(RefCell::new(None));
    let position = shift.clone();
    let floating_for_position = floating.clone();
    let list_for_position = list.clone();
    overlay.connect_get_child_position(move |widget, _child| {
        let height = floating_for_position
            .borrow()
            .as_ref()
            .map_or(44, |month| month.height);
        let width = if list_for_position.width() > 0 {
            list_for_position.width()
        } else {
            widget.width().max(1)
        };
        Some(gtk::gdk::Rectangle::new(0, position.get(), width, height))
    });

    let updating = Rc::new(Cell::new(false));
    let adjustment = scroller.vadjustment().downgrade();
    let overlay_weak = overlay.downgrade();
    let list_weak = list.downgrade();
    let anchors_for_update = anchors.clone();
    let floating_for_update = floating.clone();
    let shift_for_update = shift.clone();
    let registry_for_update = registry.clone();
    let state_weak = Rc::downgrade(state);
    let busy = updating.clone();
    let update: Rc<dyn Fn()> = Rc::new(move || {
        // Reparenting causes size/adjustment notifications synchronously. Do
        // not enter another update halfway through a widget transfer.
        if busy.replace(true) {
            return;
        }
        let (Some(adjustment), Some(overlay), Some(list)) = (
            adjustment.upgrade(),
            overlay_weak.upgrade(),
            list_weak.upgrade(),
        ) else {
            busy.set(false);
            return;
        };

        let scroll_y = adjustment.value();
        let months = anchors_for_update.borrow();
        let ys = months
            .iter()
            .map(|m| {
                m.widget
                    .compute_bounds(&list)
                    .map(|rect| f64::from(rect.y()))
            })
            .collect::<Vec<_>>();
        let current = ys
            .iter()
            .enumerate()
            .filter_map(|(i, y)| match y {
                Some(top) if *top <= scroll_y + 1.0 => Some(i),
                _ => None,
            })
            .next_back();

        // Compare the underlying GTK object identity, not merely the index.
        // A search/filter can replace every group while keeping index 0.
        let current_header = current.and_then(|i| months.get(i).map(|m| &m.header));
        let needs_switch = match (floating_for_update.borrow().as_ref(), current_header) {
            (Some(old), Some(new_header)) => old.header != *new_header,
            (None, None) => false,
            _ => true,
        };
        if needs_switch {
            // Never hold a RefCell borrow across GTK reparenting/layout calls.
            let old = floating_for_update.borrow_mut().take();
            if let Some(old) = old {
                release_floating_month(&overlay, old);
            }
            if let Some(i) = current {
                if let Some(month) = months.get(i) {
                    let next = float_original_month(&overlay, month);
                    *floating_for_update.borrow_mut() = Some(next);
                }
            }
        }

        // The next in-list month pushes the *same* pinned button upwards until
        // it has cleared the viewport. The incoming month then takes its place.
        let next_y = current.and_then(|i| ys.get(i + 1).copied().flatten());
        let height = floating_for_update
            .borrow()
            .as_ref()
            .map_or(44, |m| m.height);
        let dy = if current.is_some() {
            sticky_push_offset(scroll_y, next_y, height)
        } else {
            0
        };
        if shift_for_update.replace(dy) != dy || needs_switch {
            overlay.queue_allocate();
        }

        if collapse_previous_transfers
            && state_weak
                .upgrade()
                .is_some_and(|st| st.transfer_compact_months.get())
        {
            if let Some(index) = current {
                for month in months.iter().take(index) {
                    if let Some((_, reveal, marker)) = registry_for_update
                        .borrow()
                        .groups
                        .iter()
                        .find(|(key, _, _)| key == &month.key)
                    {
                        if reveal.get_visible() {
                            timeline_set_open(reveal, false);
                            if let Some(st) = state_weak.upgrade() {
                                set_timeline_open(&st, &month.key, false);
                            }
                            marker.set_icon_name(Some("folder-symbolic"));
                        }
                    }
                }
            }
            registry_for_update.borrow().update_toggle();
        }
        busy.set(false);
    });
    let updater = update.clone();
    scroller
        .vadjustment()
        .connect_value_changed(move |_| updater());
    let updater = update.clone();
    scroller.vadjustment().connect_changed(move |_| updater());
    let updater = update.clone();
    overlay.connect_map(move |_| updater());
    overlay
}

fn set_timeline_open(state: &State, key: &str, open: bool) {
    if open {
        state.collapsed_groups.borrow_mut().remove(key);
        state.expanded_groups.borrow_mut().insert(key.to_string());
    } else {
        state.expanded_groups.borrow_mut().remove(key);
        state.collapsed_groups.borrow_mut().insert(key.to_string());
    }
}

fn group_open(
    expanded: &HashSet<String>,
    collapsed: &HashSet<String>,
    key: &str,
    default_open: bool,
) -> bool {
    if expanded.contains(key) {
        true
    } else if collapsed.contains(key) {
        false
    } else {
        default_open
    }
}

fn timeline_is_open(state: &State, key: &str, default_open: bool) -> bool {
    group_open(
        &state.expanded_groups.borrow(),
        &state.collapsed_groups.borrow(),
        key,
        default_open,
    )
}

#[cfg(test)]
mod timeline_tests {
    use super::*;
    #[test]
    fn groups_keep_independent_user_choices() {
        let mut expanded = HashSet::new();
        let mut collapsed = HashSet::new();
        let a = "tx:month:2026-11";
        let b = "draft:month:2026-11:MAD";
        assert!(group_open(&expanded, &collapsed, a, true));
        assert!(!group_open(&expanded, &collapsed, b, false));
        collapsed.insert(a.to_owned());
        assert!(!group_open(&expanded, &collapsed, a, true));
        expanded.insert(b.to_owned());
        assert!(group_open(&expanded, &collapsed, b, false));
        assert!(!group_open(
            &expanded,
            &collapsed,
            "tx:day:2026-11-03",
            false
        ));
    }
}

struct TimelineGroup {
    key: String,
    title: String,
    detail: String,
    level: TimelineLevel,
    default_open: bool,
}
fn timeline_group(
    state: &Rc<State>,
    group: TimelineGroup,
    content: &gtk::Box,
    registry: &TimelineNodes,
) -> gtk::Box {
    let TimelineGroup {
        key,
        title,
        detail,
        level,
        default_open,
    } = group;
    let outer = gap(Orientation::Vertical, 0);
    outer.set_hexpand(true);
    outer.set_vexpand(false);
    outer.set_valign(gtk::Align::Start);
    outer.add_css_class(match level {
        TimelineLevel::Month => "saifori-timeline-month",
        TimelineLevel::Day => "saifori-timeline-day",
    });
    let header = gtk::Button::new();
    header.set_hexpand(true);
    header.set_halign(gtk::Align::Fill);
    header.add_css_class("flat");
    header.add_css_class(match level {
        TimelineLevel::Month => "saifori-month-toggle",
        TimelineLevel::Day => "saifori-day-toggle",
    });
    let inside = gap(Orientation::Horizontal, 10);
    inside.set_hexpand(true);
    inside.set_valign(gtk::Align::Center);
    let open = timeline_is_open(state, key.as_str(), default_open);
    let marker = match level {
        TimelineLevel::Month => icon(if open {
            "folder-open-symbolic"
        } else {
            "folder-symbolic"
        }),
        TimelineLevel::Day => icon(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }),
    };
    if matches!(level, TimelineLevel::Day) {
        let dot = text("●", "saifori-timeline-node");
        dot.set_wrap(false);
        inside.append(&dot);
    }
    inside.append(&marker);
    let label = text(
        &title,
        match level {
            TimelineLevel::Month => "saifori-month-name",
            TimelineLevel::Day => "saifori-day-name",
        },
    );
    label.set_hexpand(true);
    label.set_wrap(false);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    inside.append(&label);
    let tally = text(&detail, "saifori-timeline-summary");
    tally.set_wrap(false);
    inside.append(&tally);
    header.set_child(Some(&inside));
    header.set_tooltip_text(Some(if open {
        "Collapse group"
    } else {
        "Expand group"
    }));
    content.set_vexpand(false);
    content.set_valign(gtk::Align::Start);
    // The content is the collapsible widget. There is no intermediate
    // Revealer holding a layout slot once it has been hidden.
    let reveal = (*content).clone();
    timeline_set_open(&reveal, open);
    let rv = reveal.clone();
    let marker_ref = marker.clone();
    let st = state.clone();
    let k = key.clone();
    let registered = registry.clone();
    header.connect_clicked(move |b| {
        let new_value = !rv.get_visible();
        timeline_set_open(&rv, new_value);
        set_timeline_open(&st, &k, new_value);
        marker_ref.set_icon_name(Some(match level {
            TimelineLevel::Month => {
                if new_value {
                    "folder-open-symbolic"
                } else {
                    "folder-symbolic"
                }
            }
            TimelineLevel::Day => {
                if new_value {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }
            }
        }));
        b.set_tooltip_text(Some(if new_value {
            "Collapse group"
        } else {
            "Expand group"
        }));
        registered.borrow().update_toggle();
    });
    let tooltip_button = header.clone();
    let ico = marker.clone();
    reveal.connect_visible_notify(move |rv| {
        let opened = rv.get_visible();
        ico.set_icon_name(Some(match level {
            TimelineLevel::Month => {
                if opened {
                    "folder-open-symbolic"
                } else {
                    "folder-symbolic"
                }
            }
            TimelineLevel::Day => {
                if opened {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }
            }
        }));
        tooltip_button.set_tooltip_text(Some(if opened {
            "Collapse group"
        } else {
            "Expand group"
        }));
    });
    registry
        .borrow_mut()
        .groups
        .push((key, reveal.clone(), marker));
    registry.borrow().update_toggle();
    outer.append(&header);
    outer.append(&reveal);
    outer
}

fn timeline_controls(state: &Rc<State>, registry: &TimelineNodes) -> gtk::Box {
    let controls = gap(Orientation::Horizontal, 6);
    controls.add_css_class("saifori-timeline-controls");
    let toggle = gtk::Button::new();
    toggle.add_css_class("flat");
    toggle.set_tooltip_text(Some("Expand or collapse all months and days"));
    let contents = gap(Orientation::Horizontal, 8);
    contents.set_halign(gtk::Align::Center);
    let image = icon("view-fullscreen-symbolic");
    let label = gtk::Label::new(Some(&i18n::tr("Expand all")));
    contents.append(&image);
    contents.append(&label);
    toggle.set_child(Some(&contents));
    {
        let mut entries = registry.borrow_mut();
        entries.toggle_label = Some(label);
        entries.toggle_icon = Some(image);
        entries.toggle_button = Some(toggle.downgrade());
        entries.update_toggle();
    }
    let st = state.clone();
    let refs = registry.clone();
    toggle.connect_clicked(move |_| {
        // If at least one group is closed, open them all; otherwise close all.
        // This also works when a user manually opens/closes individual months.
        let expand = !refs.borrow().all_open();
        for (key, rv, marker) in refs.borrow().groups.iter() {
            timeline_set_open(rv, expand);
            set_timeline_open(&st, key, expand);
            let is_month = key.contains(":month:");
            marker.set_icon_name(Some(if is_month {
                if expand {
                    "folder-open-symbolic"
                } else {
                    "folder-symbolic"
                }
            } else if expand {
                "pan-down-symbolic"
            } else {
                "pan-end-symbolic"
            }));
        }
        refs.borrow().update_toggle();
    });
    controls.append(&toggle);
    controls
}

fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let sw = gtk::ScrolledWindow::new();
    sw.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    sw.set_hexpand(true);
    sw.set_vexpand(true);
    sw.set_child(Some(child));
    sw
}
fn button(label: &str, icon_name: &str) -> gtk::Button {
    let b = gtk::Button::new();
    let row = gap(Orientation::Horizontal, 8);
    row.set_halign(gtk::Align::Center);
    if !icon_name.is_empty() {
        row.append(&icon(icon_name));
    }
    row.append(&gtk::Label::new(Some(&i18n::tr(label))));
    b.set_child(Some(&row));
    b
}
// A real GtkListBoxRow: Adwaita owns selection, hover and focus states.
// No CSS-simulated active buttons or manual selected-state painting.
fn navigation_row(label: &str, icon_name: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(true);
    row.set_selectable(true);
    let inner = gap(Orientation::Horizontal, 12);
    inner.set_valign(gtk::Align::Center);
    inner.set_margin_start(12);
    inner.set_margin_end(12);
    inner.set_margin_top(9);
    inner.set_margin_bottom(9);
    let image = icon(icon_name);
    image.set_pixel_size(18);
    inner.append(&image);
    let name = text(label, "");
    name.set_wrap(false);
    inner.append(&name);
    row.set_child(Some(&inner));
    row
}

// Native Libadwaita preference controls used in Settings, including built-in
// keyboard navigation, focus indication, hover and accessibility semantics.
fn preference_group(title: &str, description: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(&glib::markup_escape_text(&i18n::tr(title)));
    if !description.is_empty() {
        group.set_description(Some(&i18n::tr(description)));
    }
    group
}
fn preference_switch(title: &str, subtitle: &str, enabled: bool) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(&glib::markup_escape_text(&i18n::tr(title)));
    if !subtitle.is_empty() {
        row.set_subtitle(&i18n::tr(subtitle));
    }
    row.set_active(enabled);
    row
}
fn preference_action(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(&glib::markup_escape_text(&i18n::tr(title)));
    if !subtitle.is_empty() {
        row.set_subtitle(&i18n::tr(subtitle));
    }
    row
}

fn change_page(state: &Rc<State>, page: Page) {
    if page != Page::Transactions {
        state.transaction_scroll_position.set(0.0);
    }
    state.page.set(page);
    state.selected.set(None);
    refresh(state);
}
fn currency(state: &State) -> String {
    state.currency.borrow().clone()
}
fn month(state: &State) -> String {
    state.month.borrow().clone()
}
fn masked(state: &State, value: &str) -> String {
    if state.privacy.get() {
        "••••".into()
    } else {
        value.into()
    }
}
fn masked_money(state: &State, value: i64, cur: &str) -> String {
    if state.privacy.get() {
        format!("•••• {cur}")
    } else {
        model::money(value, cur)
    }
}
fn obscured_chart(state: &State) -> bool {
    state.privacy.get()
}
fn private_placeholder() -> gtk::Label {
    text("Hidden in privacy mode", "saifori-muted")
}
fn dropdown(options: &[&str]) -> gtk::DropDown {
    let labels: Vec<String> = options.iter().map(|v| i18n::tr(v)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::DropDown::from_strings(&refs)
}

fn checked<T>(parent: &gtk::Box, read: anyhow::Result<T>) -> Option<T> {
    match read {
        Ok(data) => Some(data),
        Err(err) => {
            parent.append(&text(
                &format!("Unable to load data: {err}"),
                "saifori-negative",
            ));
            None
        }
    }
}

fn warning(state: &Rc<State>, title: &str, message: &str) {
    let popup = gtk::Window::builder()
        .title(title)
        .transient_for(&state.window)
        .modal(true)
        .default_width(430)
        .resizable(false)
        .build();
    let content = gap(Orientation::Vertical, 16);
    margin(&content, 24);
    let headline = text(title, "saifori-section-title");
    content.append(&headline);
    content.append(&text(message, ""));
    let close = button("OK", "");
    close.add_css_class("suggested-action");
    let target = popup.clone();
    close.connect_clicked(move |_| target.close());
    close.set_halign(gtk::Align::End);
    content.append(&close);
    popup.set_child(Some(&content));
    popup.present();
}
fn run_write<T>(state: &Rc<State>, result: anyhow::Result<T>) -> bool {
    match result {
        Ok(_) => {
            refresh(state);
            true
        }
        Err(err) => {
            warning(state, "Could not save", &err.to_string());
            false
        }
    }
}

// The only persistent Test Mode value belongs to the *real* settings table.
// The demo connection is in-memory and is never merged into the real ledger.
fn change_test_mode(state: &Rc<State>, enabled: bool) {
    if state.test_mode.get() == enabled {
        return;
    }
    if state.chat_busy.get() || state.capture_busy.get() {
        warning(
            state,
            "Operation in progress",
            "Finish the current AI request before switching Test Mode.",
        );
        return;
    }
    if enabled {
        // Prepare the in-memory ledger before recording the preference. If the
        // fixture fails, the real database and current UI are left untouched.
        let demo_db = match demo::build(chrono::Local::now().date_naive()) {
            Ok(data) => data,
            Err(err) => {
                warning(state, "Test Mode error", &err.to_string());
                return;
            }
        };
        if let Err(err) = state.db.borrow_mut().set_setting("test_mode", "1") {
            warning(state, "Settings error", &err.to_string());
            return;
        }
        *state.db.borrow_mut() = demo_db;
        state.test_mode.set(true);
        *state.currency.borrow_mut() = "USD".into();
    } else {
        // First reopen the private ledger and persist the off switch there.
        // No demo transactions are copied, exported or restored into it.
        let mut real = match Store::open(&state.real_db_path) {
            Ok(data) => data,
            Err(err) => {
                warning(state, "Could not open private ledger", &err.to_string());
                return;
            }
        };
        if let Err(err) = real.set_setting("test_mode", "0") {
            warning(state, "Settings error", &err.to_string());
            return;
        }
        *state.currency.borrow_mut() = real.currency().unwrap_or_else(|_| "MAD".into());
        *state.language.borrow_mut() = real
            .setting("language")
            .ok()
            .flatten()
            .filter(|s| i18n::valid(s))
            .unwrap_or_else(i18n::system_language);
        *state.ai_provider.borrow_mut() = normalized_ai_provider(
            &real
                .setting("ai_provider")
                .ok()
                .flatten()
                .unwrap_or_else(|| "deepseek".into()),
        );
        state
            .privacy
            .set(real.setting("privacy").ok().flatten().as_deref() == Some("1"));
        state
            .assistant_tabs
            .set(real.setting("assistant_tabs").ok().flatten().as_deref() != Some("0"));
        state
            .transaction_as_page
            .set(real.setting("transaction_page").ok().flatten().as_deref() == Some("1"));
        state.transfer_compact_months.set(
            real.setting("transfer_compact_months")
                .ok()
                .flatten()
                .as_deref()
                == Some("1"),
        );
        *state.db.borrow_mut() = real;
        state.test_mode.set(false);
    }
    // Never allow personal chat or Smart Capture drafts to cross between modes.
    state.selected.set(None);
    state.editing_transaction.set(None);
    state.filter.borrow_mut().clear();
    state.tx_kind.set(0);
    state.tx_range.set(0);
    state.tx_category.set(0);
    *state.tx_day.borrow_mut() = None;
    state.collapsed_groups.borrow_mut().clear();
    state.expanded_groups.borrow_mut().clear();
    state.transaction_scroll_position.set(0.0);
    state.capture_scroll_position.set(0.0);
    state.capture_input.borrow_mut().clear();
    state.capture_drafts.borrow_mut().clear();
    let prior = state
        .db
        .borrow()
        .chat_sessions(1)
        .ok()
        .and_then(|rows| rows.first().map(|r| r.0));
    *state.messages.borrow_mut() = prior
        .and_then(|id| state.db.borrow().chat_messages(id).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(mine, body)| Message { mine, body })
        .collect();
    *state.chat_month.borrow_mut() =
        prior.and_then(|id| state.db.borrow().chat_month(id).ok().flatten());
    state.active_chat.set(prior);
    state.assistant_view.set(0);
    *state.month.borrow_mut() = model::current_month();
    state.annual_year.set(chrono::Local::now().year());
    state.page.set(Page::Overview);
    refresh(state);
}

fn refresh(state: &Rc<State>) {
    i18n::set_language(&state.language.borrow());
    let root = gap(Orientation::Vertical, 0);
    root.set_direction(gtk::TextDirection::Ltr);
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Saifori", "Personal Finance")));
    let add = button("Add Transaction", "list-add-symbolic");
    add.add_css_class("suggested-action");
    let s = state.clone();
    add.connect_clicked(move |_| transaction_dialog(&s, None));
    header.pack_end(&add);
    if state.test_mode.get() {
        let leave = button("Exit Test Mode", "window-close-symbolic");
        leave.add_css_class("flat");
        leave.set_tooltip_text(Some(
            "Leave the temporary USD demo and return to your private ledger",
        ));
        let st = state.clone();
        leave.connect_clicked(move |_| change_test_mode(&st, false));
        header.pack_end(&leave);
    }
    let privacy = button(
        "",
        if state.privacy.get() {
            "view-reveal-symbolic"
        } else {
            "view-conceal-symbolic"
        },
    );
    privacy.add_css_class("flat");
    privacy.set_tooltip_text(Some(&i18n::tr(if state.privacy.get() {
        "Show private data"
    } else {
        "Hide private data"
    })));
    let st = state.clone();
    privacy.connect_clicked(move |_| {
        let enabled = !st.privacy.get();
        let result = st
            .db
            .borrow_mut()
            .set_setting("privacy", if enabled { "1" } else { "0" });
        match result {
            Ok(()) => {
                st.privacy.set(enabled);
                refresh(&st);
            }
            Err(e) => warning(&st, "Settings error", &e.to_string()),
        }
    });
    header.pack_end(&privacy);
    let show_navigation = button("", "sidebar-show-symbolic");
    show_navigation.add_css_class("flat");
    show_navigation.set_tooltip_text(Some("Show navigation"));
    show_navigation.set_visible(false); // Only needed in the collapsed native split view.
    header.pack_start(&show_navigation);
    root.append(&header);

    let sidebar = gap(Orientation::Vertical, 12);
    sidebar.set_hexpand(false);
    sidebar.set_vexpand(true);
    sidebar.set_margin_top(12);
    sidebar.add_css_class("background");
    let brand = gap(Orientation::Horizontal, 11);
    margin(&brand, 14);
    let dev_logo = std::path::Path::new("assets/io.github.oeiv.Saifori.svg");
    let logo = if dev_logo.exists() {
        gtk::Image::from_file(dev_logo)
    } else {
        gtk::Image::from_icon_name(model::APP_ID)
    };
    logo.set_pixel_size(40);
    logo.add_css_class("icon-dropshadow");
    brand.append(&logo);
    let brand_words = gap(Orientation::Vertical, 1);
    brand_words.append(&text("Saifori", "saifori-section-title"));
    brand_words.append(&text("Personal Finance", "saifori-muted"));
    brand.append(&brand_words);
    sidebar.append(&brand);

    let links = [
        (Page::Overview, "Overview", "go-home-symbolic"),
        (Page::Transactions, "Transactions", "view-list-symbolic"),
        (Page::Transfers, "Transfers", "mail-send-receive-symbolic"),
        (Page::Activity, "Activity", "document-open-recent-symbolic"),
        (Page::Budgets, "Budgets", "saifori-budget-symbolic"),
        (Page::Analytics, "Analytics", "saifori-chart-symbolic"),
        (Page::Products, "Products", "saifori-cart-symbolic"),
        (Page::Assistant, "AI Assistant", "saifori-sparkle-symbolic"),
    ];
    let nav = gtk::ListBox::new();
    nav.add_css_class("navigation-sidebar");
    nav.set_selection_mode(gtk::SelectionMode::Single);
    nav.set_activate_on_single_click(true);
    nav.set_margin_start(8);
    nav.set_margin_end(8);
    for (index, (page, name, image)) in links.iter().enumerate() {
        let item = navigation_row(name, image);
        nav.append(&item);
        if state.page.get() == *page
            || (*page == Page::Transactions && state.page.get() == Page::TransactionEditor)
        {
            nav.select_row(Some(&item));
        }
        debug_assert_eq!(item.index(), index as i32);
    }
    let st = state.clone();
    nav.connect_row_activated(move |_list, row| {
        if let Some((page, _, _)) = links.get(row.index() as usize) {
            change_page(&st, *page);
        }
    });
    sidebar.append(&nav);

    let filler = gtk::Box::new(Orientation::Vertical, 0);
    filler.set_vexpand(true);
    sidebar.append(&filler);
    sidebar.append(&line());
    let utility = gtk::ListBox::new();
    utility.add_css_class("navigation-sidebar");
    utility.set_selection_mode(gtk::SelectionMode::Single);
    utility.set_activate_on_single_click(true);
    utility.set_margin_start(8);
    utility.set_margin_end(8);
    utility.set_margin_bottom(12);
    let help = navigation_row("Help & FAQ", "help-browser-symbolic");
    utility.append(&help);
    if state.page.get() == Page::Help {
        utility.select_row(Some(&help));
    }
    let settings = navigation_row("Settings", "preferences-system-symbolic");
    utility.append(&settings);
    if state.page.get() == Page::Settings {
        utility.select_row(Some(&settings));
    }
    let st = state.clone();
    utility.connect_row_activated(move |_list, row| {
        change_page(
            &st,
            if row.index() == 0 {
                Page::Help
            } else {
                Page::Settings
            },
        );
    });
    sidebar.append(&utility);

    let content = gap(Orientation::Vertical, 16);
    content.add_css_class("saifori-page");
    content.set_hexpand(true);
    content.set_vexpand(true);
    match state.page.get() {
        Page::Overview => overview(state, &content),
        Page::Transactions => transactions(state, &content),
        Page::TransactionEditor => transaction_editor_page(state, &content),
        Page::Transfers => transfers_page(state, &content),
        Page::Activity => activity_page(state, &content),
        Page::Budgets => budgets(state, &content),
        Page::Analytics => analytics(state, &content),
        Page::Products => products_page(state, &content),
        Page::Assistant => assistant(state, &content),
        Page::Settings => settings_page(state, &content),
        Page::Help => help_page(state, &content),
    }
    // Fixed assistant workspace: only messages and draft lists scroll, never the
    // composer, tabs, capture actions or the whole page during a chat gesture.
    let content_widget: gtk::Widget = if matches!(
        state.page.get(),
        Page::Assistant | Page::Transfers | Page::Transactions | Page::Activity
    ) {
        content.upcast()
    } else {
        let scroller = scroll(&content);
        scroller.set_hexpand(true);
        scroller.set_min_content_width(0);
        scroller.upcast()
    };
    let split = adw::NavigationSplitView::new();
    split.set_min_sidebar_width(205.0);
    split.set_max_sidebar_width(238.0);
    split.set_sidebar_width_fraction(0.18);
    split.set_show_content(true);
    split.set_sidebar(Some(&adw::NavigationPage::new(&sidebar, "Navigation")));
    split.set_content(Some(&adw::NavigationPage::new(&content_widget, "Saifori")));
    split.set_hexpand(true);
    split.set_vexpand(true);
    // Native Libadwaita breakpoint: on compact windows the sidebar becomes a
    // navigation page rather than squeezing the ledger or stealing half its width.
    let breakpoint = adw::Breakpoint::new(
        adw::BreakpointCondition::parse("max-width: 900px")
            .expect("Valid Adwaita breakpoint condition"),
    );
    let collapsed = true.to_value();
    breakpoint.add_setter(&split, "collapsed", Some(&collapsed));
    let responsive = adw::BreakpointBin::new();
    responsive.set_size_request(420, 320);
    responsive.set_child(Some(&split));
    responsive.add_breakpoint(breakpoint);
    responsive.set_hexpand(true);
    responsive.set_vexpand(true);
    let view = split.clone();
    show_navigation.connect_clicked(move |_| view.set_show_content(false));
    let toggle = show_navigation.clone();
    split.connect_collapsed_notify(move |view| toggle.set_visible(view.is_collapsed()));
    root.append(&responsive);
    state.window.set_content(Some(&root));
}

fn page_title(parent: &gtk::Box, title: &str, subtitle: &str) {
    let block = gap(Orientation::Vertical, 5);
    block.append(&text(title, "saifori-title"));
    if !subtitle.is_empty() {
        block.append(&text(subtitle, "saifori-subtitle"));
    }
    parent.append(&block);
}
fn metric(title: &str, amount: String, caption: &str, icon_name: &str) -> gtk::Box {
    let c = card();
    c.set_hexpand(true);
    c.add_css_class("saifori-metric-card");
    let row = gap(Orientation::Horizontal, 15);
    row.set_valign(gtk::Align::Center);
    let tile = gap(Orientation::Vertical, 0);
    tile.add_css_class("saifori-metric-icon");
    if title == "Income" {
        tile.add_css_class("income");
    } else if title == "Expenses" || title == "Spent" {
        tile.add_css_class("expense");
    } else {
        tile.add_css_class("balance");
    }
    tile.set_valign(gtk::Align::Center);
    // Correct diagonal financial arrows instead of go-up/go-down chevrons.
    if title == "Income" || title == "Expenses" {
        let arrow = gtk::Label::new(Some(if title == "Income" { "↗" } else { "↘" }));
        arrow.add_css_class("saifori-trend-arrow");
        tile.append(&arrow);
    } else {
        let img = icon(icon_name);
        img.set_pixel_size(22);
        tile.append(&img);
    }
    row.append(&tile);
    let values = gap(Orientation::Vertical, 4);
    values.set_hexpand(true);
    values.append(&text(title, "saifori-muted"));
    let kpi = text(&amount, "saifori-kpi");
    kpi.set_wrap(false);
    values.append(&kpi);
    values.append(&text(caption, "saifori-muted"));
    row.append(&values);
    c.append(&row);
    c
}
fn metric_icon(icon_name: &str, style: &str) -> gtk::Box {
    let tile = gap(Orientation::Vertical, 0);
    tile.add_css_class("saifori-action-icon");
    tile.add_css_class(style);
    tile.set_valign(gtk::Align::Center);
    let image = icon(icon_name);
    image.set_pixel_size(20);
    tile.append(&image);
    tile
}
fn month_navigation(state: &Rc<State>) -> gtk::Box {
    let row = gap(Orientation::Horizontal, 5);
    row.set_halign(gtk::Align::End);
    row.add_css_class("saifori-month-picker");
    let prev = button("", "go-previous-symbolic");
    prev.add_css_class("flat");
    prev.set_tooltip_text(Some("Previous month"));
    // Display a readable month rather than the raw YYYY-MM database key.
    let label = i18n::display_month(&month(state));
    let month_label = text(&label, "saifori-section-title");
    month_label.set_xalign(0.5);
    month_label.add_css_class("saifori-month-label");
    let next = button("", "go-next-symbolic");
    next.add_css_class("flat");
    next.set_tooltip_text(Some("Next month"));
    let st = state.clone();
    prev.connect_clicked(move |_| {
        if let Some(m) = model::month_offset(&month(&st), -1) {
            *st.month.borrow_mut() = m;
            refresh(&st);
        }
    });
    next.set_sensitive(month(state) < model::current_month());
    let st = state.clone();
    next.connect_clicked(move |_| {
        if let Some(m) = model::month_offset(&month(&st), 1) {
            *st.month.borrow_mut() = m;
            refresh(&st);
        }
    });
    row.append(&prev);
    row.append(&month_label);
    row.append(&next);
    row
}

fn annual_summary_widget(
    state: &Rc<State>,
    year: i32,
    cur: &str,
    annual: &model::Summary,
) -> gtk::Box {
    let wrap = gap(Orientation::Horizontal, 14);
    wrap.add_css_class("saifori-annual-summary");
    wrap.set_valign(gtk::Align::Center);
    let heading = gap(Orientation::Horizontal, 8);
    heading.set_hexpand(true);
    let previous = button("", "go-previous-symbolic");
    previous.add_css_class("flat");
    previous.set_sensitive(year > 1900);
    previous.set_tooltip_text(Some(&i18n::tr("Previous year")));
    let st = state.clone();
    previous.connect_clicked(move |_| {
        if st.annual_year.get() > 1900 {
            st.annual_year.set(st.annual_year.get() - 1);
            refresh(&st);
        }
    });
    let label = text(
        &format!("{}  {}", i18n::tr("Year total"), year),
        "saifori-section-title",
    );
    label.set_wrap(false);
    let next = button("", "go-next-symbolic");
    next.add_css_class("flat");
    next.set_sensitive(year < chrono::Local::now().year());
    next.set_tooltip_text(Some(&i18n::tr("Next year")));
    let st = state.clone();
    next.connect_clicked(move |_| {
        if st.annual_year.get() < chrono::Local::now().year() {
            st.annual_year.set(st.annual_year.get() + 1);
            refresh(&st);
        }
    });
    heading.append(&previous);
    heading.append(&label);
    heading.append(&next);
    wrap.append(&heading);
    for (name, amount, kind) in [
        ("Income", annual.income, "saifori-positive"),
        ("Expenses", annual.expense, "saifori-negative"),
        ("Net cash flow", annual.balance(), "saifori-amount"),
    ] {
        let block = gap(Orientation::Vertical, 3);
        block.add_css_class("saifori-annual-figure");
        block.append(&text(name, "saifori-muted"));
        let money = text(&masked_money(state, amount, cur), kind);
        money.set_wrap(false);
        block.append(&money);
        wrap.append(&block);
    }
    wrap
}

fn overview(state: &Rc<State>, parent: &gtk::Box) {
    let top = gap(Orientation::Horizontal, 20);
    top.set_hexpand(true);
    let t = gap(Orientation::Vertical, 5);
    t.set_hexpand(true);
    t.append(&text("Overview", "saifori-title"));
    t.append(&text("Your money at a glance.", "saifori-subtitle"));
    top.append(&t);
    top.append(&month_navigation(state));
    parent.append(&top);
    let cur = currency(state);
    let m = month(state);
    let Some(summary) = checked(parent, state.db.borrow().summary(&cur, &m)) else {
        return;
    };
    let metrics = gap(Orientation::Horizontal, 14);
    metrics.set_homogeneous(true);
    metrics.append(&metric(
        "Income",
        masked_money(state, summary.income, &cur),
        "This month",
        "go-up-symbolic",
    ));
    metrics.append(&metric(
        "Expenses",
        masked_money(state, summary.expense, &cur),
        "This month",
        "go-down-symbolic",
    ));
    metrics.append(&metric(
        "Balance",
        masked_money(state, summary.balance(), &cur),
        "Income minus expenses",
        "saifori-budget-symbolic",
    ));
    parent.append(&metrics);
    let year = state.annual_year.get();
    let Some(annual) = checked(parent, state.db.borrow().year_summary(&cur, year)) else {
        return;
    };
    parent.append(&annual_summary_widget(state, year, &cur, &annual));
    let period = match state.overview_range.get() {
        0 => 3,
        2 => 12,
        _ => 6,
    };
    let Some(months) = checked(parent, state.db.borrow().months(&cur, &m, period)) else {
        return;
    };
    let Some(cats) = checked(parent, state.db.borrow().categories(&cur, &m)) else {
        return;
    };
    let charts_row = gap(Orientation::Horizontal, 16);
    charts_row.set_valign(gtk::Align::Start);
    let left = card();
    left.set_hexpand(true);
    let chart_heading = gap(Orientation::Horizontal, 12);
    let heading = section_heading("Income vs. Expenses", "Cash flow over time");
    heading.set_hexpand(true);
    chart_heading.append(&heading);
    let range = dropdown(&["Last 3 months", "Last 6 months", "Last 12 months"]);
    range.set_selected(state.overview_range.get());
    range.add_css_class("saifori-range");
    let st = state.clone();
    range.connect_selected_notify(move |dropdown| {
        if dropdown.selected() != st.overview_range.get() {
            st.overview_range.set(dropdown.selected());
            refresh(&st);
        }
    });
    chart_heading.append(&range);
    left.append(&chart_heading);
    if obscured_chart(state) {
        left.append(&private_placeholder());
    } else {
        left.append(&charts::line_chart(months.clone()));
    }
    let legend = gap(Orientation::Horizontal, 18);
    let income_label = text("● Income", "saifori-positive");
    income_label.set_wrap(false);
    let expense_label = text("● Expenses", "saifori-chart-expense");
    expense_label.set_wrap(false);
    legend.append(&income_label);
    legend.append(&expense_label);
    left.append(&legend);
    charts_row.append(&left);
    let right = card();
    right.set_width_request(485);
    right.set_hexpand(true);
    right.append(&section_heading("Spending by Category", "This month"));
    let dough = gap(Orientation::Horizontal, 10);
    if obscured_chart(state) {
        dough.append(&private_placeholder());
    } else {
        let donut = charts::donut(cats.clone(), &cur);
        donut.set_valign(gtk::Align::Center);
        dough.append(&donut);
    }
    let legend = gap(Orientation::Vertical, 9);
    legend.set_hexpand(true);
    legend.set_valign(gtk::Align::Center);
    if cats.is_empty() {
        legend.append(&text("No spending yet", "saifori-muted"));
    }
    let total: i64 = cats.iter().map(|c| c.amount_minor).sum();
    for (i, c) in cats
        .iter()
        .take(if obscured_chart(state) { 0 } else { 7 })
        .enumerate()
    {
        let row = gap(Orientation::Horizontal, 7);
        row.add_css_class("saifori-category-row");
        let dot = text("●", "");
        dot.set_wrap(false);
        let (r, g, b) = charts::palette(i);
        dot.set_markup(&format!(
            "<span foreground=\"#{:02x}{:02x}{:02x}\">●</span>",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8
        ));
        row.append(&dot);
        let label = text(&c.category, "");
        label.set_hexpand(true);
        label.set_wrap(false);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&label);
        let percent = if total > 0 {
            100.0 * c.amount_minor as f64 / total as f64
        } else {
            0.0
        };
        let pct = text(&format!("{percent:.0}%"), "saifori-muted");
        pct.set_width_chars(4);
        pct.set_xalign(1.0);
        pct.set_wrap(false);
        row.append(&pct);
        let amt = text(
            &model::money(c.amount_minor, &cur),
            "saifori-category-amount",
        );
        amt.set_wrap(false);
        amt.set_xalign(1.0);
        row.append(&amt);
        legend.append(&row);
    }
    dough.append(&legend);
    right.append(&dough);
    charts_row.append(&right);
    parent.append(&charts_row);

    let bottom = gap(Orientation::Horizontal, 16);
    bottom.set_valign(gtk::Align::Start);
    let recent = card();
    recent.set_hexpand(true);
    let bar = gap(Orientation::Horizontal, 10);
    let heading = section_heading("Recent Transactions", "Latest recorded activity");
    heading.set_hexpand(true);
    bar.append(&heading);
    let view = button("View All", "go-next-symbolic");
    view.add_css_class("flat");
    let st = state.clone();
    view.connect_clicked(move |_| change_page(&st, Page::Transactions));
    bar.append(&view);
    recent.append(&bar);
    let Some(txs) = checked(parent, state.db.borrow().recent(&cur, 4, "", "")) else {
        return;
    };
    if txs.is_empty() {
        recent.append(&text(
            "No transactions yet. Use Add Transaction to get started.",
            "saifori-muted",
        ));
    }
    let mut current_day = String::new();
    for tx in txs {
        if current_day != tx.day {
            current_day = tx.day.clone();
            recent.append(&text(&friendly_day(&current_day), "saifori-muted"));
        }
        recent.append(&transaction_item(state, &tx, false));
    }
    bottom.append(&recent);
    let side = gap(Orientation::Vertical, 14);
    side.set_width_request(485);
    side.set_hexpand(true);
    let insight = card();
    let insight_row = gap(Orientation::Horizontal, 12);
    insight_row.set_valign(gtk::Align::Center);
    let insight_icon = metric_icon("saifori-sparkle-symbolic", "saifori-soft-purple");
    insight_row.append(&insight_icon);
    let insight_copy = gap(Orientation::Vertical, 5);
    insight_copy.set_hexpand(true);
    insight_copy.append(&section_heading("AI Insight", ""));
    // Keep the owned insight text alive for the duration of the label creation.
    let insight_message = if state.privacy.get() {
        "••••".to_owned()
    } else {
        insights::biggest_insight(&summary, &cats, &cur)
    };
    insight_copy.append(&text(&insight_message, ""));
    insight_row.append(&insight_copy);
    let details = button("", "go-next-symbolic");
    details.add_css_class("flat");
    details.set_tooltip_text(Some("Open AI Assistant"));
    let st = state.clone();
    details.connect_clicked(move |_| change_page(&st, Page::Assistant));
    insight_row.append(&details);
    insight.append(&insight_row);
    side.append(&insight);
    let quick = card();
    let quick_row = gap(Orientation::Horizontal, 12);
    quick_row.set_valign(gtk::Align::Center);
    quick_row.append(&metric_icon("scanner-symbolic", "saifori-soft-blue"));
    let copy = section_heading("Smart Capture", "Type expenses to create reviewed drafts");
    copy.set_hexpand(true);
    quick_row.append(&copy);
    let open = button("Smart Capture", "");
    let st = state.clone();
    open.connect_clicked(move |_| change_page(&st, Page::Assistant));
    quick_row.append(&open);
    quick.append(&quick_row);
    side.append(&quick);
    bottom.append(&side);
    parent.append(&bottom);
}

fn friendly_day(day: &str) -> String {
    let Ok(date) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return day.to_owned();
    };
    let today = chrono::Local::now().date_naive();
    if date == today {
        i18n::tr("Today")
    } else if today.pred_opt() == Some(date) {
        i18n::tr("Yesterday")
    } else {
        format!(
            "{} {}",
            i18n::month_name(date.month() as usize, true),
            date.day()
        )
    }
}
fn transaction_item(state: &Rc<State>, tx: &Transaction, details: bool) -> gtk::Button {
    let b = gtk::Button::new();
    b.add_css_class("flat");
    let content = gap(Orientation::Horizontal, 12);
    content.add_css_class("saifori-list-row");
    content.append(&metric_icon(
        model::category_icon(&tx.category),
        "saifori-neutral-icon",
    ));
    let names = gap(Orientation::Vertical, 2);
    names.set_hexpand(true);
    names.append(&text(&masked(state, &tx.description), ""));
    names.append(&text(
        &format!(
            "{} · {}",
            friendly_day(&tx.day),
            masked(state, &tx.category)
        ),
        "saifori-muted",
    ));
    content.append(&names);
    let amount = text(
        &if state.privacy.get() {
            masked_money(state, tx.amount_minor, &tx.currency)
        } else {
            format!(
                "{}{}",
                if tx.kind == "expense" { "−" } else { "+" },
                model::money(tx.amount_minor, &tx.currency)
            )
        },
        "saifori-amount",
    );
    amount.set_wrap(false);
    amount.set_width_chars(17);
    amount.add_css_class(if tx.kind == "expense" {
        "saifori-negative"
    } else {
        "saifori-positive"
    });
    amount.set_xalign(1.0);
    content.append(&amount);
    b.set_child(Some(&content));
    let id = tx.id;
    let st = state.clone();
    b.connect_clicked(move |_| {
        if details {
            st.selected.set(Some(id));
            refresh(&st);
        } else {
            transaction_dialog(&st, Some(id));
        }
    });
    b
}

// One native GTK row per record; no WebView, no fabricated chart/receipt data.
fn table_cell(value: &str, min_width: i32, class: &str) -> gtk::Label {
    let label = text(value, class);
    label.set_wrap(false);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(27);
    label.set_width_request(min_width);
    label
}
fn choice(options: &[&str], current: &str) -> gtk::DropDown {
    let control = dropdown(options);
    if let Some(index) = options.iter().position(|v| *v == current) {
        control.set_selected(index as u32);
    }
    control
}
fn selected(options: &[&str], control: &gtk::DropDown) -> String {
    options
        .get(control.selected() as usize)
        .unwrap_or(&options[0])
        .to_string()
}
fn choice_owned(options: &[String], current: &str) -> gtk::DropDown {
    let refs = options.iter().map(String::as_str).collect::<Vec<_>>();
    choice(&refs, current)
}
fn selected_owned(options: &[String], control: &gtk::DropDown) -> String {
    options
        .get(control.selected() as usize)
        .cloned()
        .unwrap_or_default()
}
fn category_options(state: &Rc<State>) -> Vec<String> {
    state.db.borrow().category_names().unwrap_or_else(|_| {
        model::CATEGORIES
            .iter()
            .map(|name| (*name).to_string())
            .collect()
    })
}

fn field(label: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let block = gap(Orientation::Vertical, 6);
    let title = text(label, "saifori-muted");
    title.set_mnemonic_widget(Some(widget));
    widget
        .as_ref()
        .update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
    block.append(&title);
    block.append(widget);
    block
}
fn transfers_page(state: &Rc<State>, parent: &gtk::Box) {
    page_title(parent, "Transfers", "");
    let new = card();
    new.append(&section_heading("New transfer", ""));
    let form = gap(Orientation::Vertical, 12);
    let accounts = gap(Orientation::Horizontal, 12);
    accounts.set_homogeneous(true);
    let source = gtk::Entry::new();
    source.set_placeholder_text(Some(&i18n::tr("From account")));
    let destination = gtk::Entry::new();
    destination.set_placeholder_text(Some(&i18n::tr("To account")));
    accounts.append(&field("From account", &source));
    accounts.append(&field("To account", &destination));
    form.append(&accounts);
    let money_fields = gap(Orientation::Horizontal, 12);
    money_fields.set_homogeneous(true);
    let amount = gtk::Entry::new();
    amount.set_placeholder_text(Some("0.00"));
    amount.set_input_purpose(gtk::InputPurpose::Number);
    let day = gtk::Entry::new();
    day.set_text(&model::today());
    money_fields.append(&field("Amount", &amount));
    money_fields.append(&field("Date · YYYY-MM-DD", &day));
    form.append(&money_fields);
    let note = gtk::Entry::new();
    note.set_placeholder_text(Some(&i18n::tr("Optional note")));
    form.append(&field("Note", &note));
    let save = button("Save transfer", "document-save-symbolic");
    save.add_css_class("suggested-action");
    save.set_halign(gtk::Align::End);
    let st = state.clone();
    save.connect_clicked(move |_| {
        let number = match model::parse_minor(amount.text().as_str()) {
            Ok(v) => v,
            Err(e) => {
                warning(&st, "Invalid amount", &e);
                return;
            }
        };
        let result = st.db.borrow_mut().add_transfer(
            &day.text(),
            number,
            &currency(&st),
            &source.text(),
            &destination.text(),
            &note.text(),
        );
        run_write(&st, result);
    });
    form.append(&save);
    new.append(&form);
    new.set_vexpand(false);
    parent.append(&new);
    let history = card();
    history.set_vexpand(true);
    history.add_css_class("saifori-transfer-history");
    history.append(&section_heading("Transfer history", ""));
    let nodes: TimelineNodes = timeline_nodes();
    history.append(&timeline_controls(state, &nodes));
    let column_head = gap(Orientation::Horizontal, 12);
    column_head.add_css_class("saifori-table-heading");
    let description = text("Accounts / date", "saifori-muted");
    description.set_hexpand(true);
    column_head.append(&description);
    column_head.append(&text("Amount", "saifori-muted"));
    history.append(&column_head);
    let records_view = gap(Orientation::Vertical, 5);
    records_view.set_vexpand(false);
    records_view.set_valign(gtk::Align::Start);
    let month_anchors: MonthAnchors = Rc::new(RefCell::new(Vec::new()));
    match state.db.borrow().transfers(&currency(state), 200) {
        Ok(records) if records.is_empty() => {
            records_view.append(&text("No transfers yet.", "saifori-muted"))
        }
        Ok(records) => {
            let mut months: BTreeMap<String, BTreeMap<String, Vec<model::Transfer>>> =
                BTreeMap::new();
            for transfer in records {
                let month = transfer.day.get(..7).unwrap_or("Unknown").to_string();
                months
                    .entry(month)
                    .or_default()
                    .entry(transfer.day.clone())
                    .or_default()
                    .push(transfer);
            }
            for (mi, (month, days)) in months.iter().rev().enumerate() {
                let days_box = gap(Orientation::Vertical, 3);
                days_box.set_vexpand(false);
                days_box.set_valign(gtk::Align::Start);
                days_box.add_css_class("saifori-timeline-days");
                for (di, (day, records)) in days.iter().rev().enumerate() {
                    let rows = gap(Orientation::Vertical, 4);
                    rows.set_vexpand(false);
                    rows.set_valign(gtk::Align::Start);
                    for transfer in records {
                        let row = gap(Orientation::Horizontal, 14);
                        row.add_css_class("saifori-list-row");
                        row.append(&icon("mail-send-receive-symbolic"));
                        let details = gap(Orientation::Vertical, 3);
                        details.set_hexpand(true);
                        details.append(&text(
                            &masked(
                                state,
                                &format!("{} → {}", transfer.source, transfer.destination),
                            ),
                            "saifori-row-description",
                        ));
                        let timing = i18n::fmt(
                            "{day} · Added {added} UTC",
                            &[("day", &transfer.day), ("added", &transfer.created_at)],
                        );
                        details.append(&text(&timing, "saifori-muted"));
                        if !transfer.note.is_empty() {
                            details.append(&text(&masked(state, &transfer.note), "saifori-muted"));
                        }
                        row.append(&details);
                        let value = text(
                            &masked_money(state, transfer.amount_minor, &transfer.currency),
                            "saifori-amount",
                        );
                        value.set_wrap(false);
                        row.append(&value);
                        let delete = button("", "user-trash-symbolic");
                        delete.set_tooltip_text(Some(&i18n::tr("Delete transfer")));
                        delete.add_css_class("flat");
                        let st = state.clone();
                        let id = transfer.id;
                        delete.connect_clicked(move |_| confirm_delete_transfer(&st, id));
                        row.append(&delete);
                        rows.append(&row);
                    }
                    let total: i64 = records.iter().map(|item| item.amount_minor).sum();
                    let cur = currency(state);
                    days_box.append(&timeline_group(
                        state,
                        TimelineGroup {
                            key: format!("transfer:day:{day}"),
                            title: friendly_day(day),
                            detail: format!(
                                "{} · {}",
                                records.len(),
                                masked_money(state, total, &cur)
                            ),
                            level: TimelineLevel::Day,
                            default_open: mi == 0 && di == 0,
                        },
                        &rows,
                        &nodes,
                    ));
                }
                let count = days.values().map(Vec::len).sum::<usize>();
                let cur = currency(state);
                let total: i64 = days
                    .values()
                    .flat_map(|r| r.iter())
                    .map(|r| r.amount_minor)
                    .sum();
                let display = i18n::display_month(month);
                let group = timeline_group(
                    state,
                    TimelineGroup {
                        key: format!("transfer:month:{month}"),
                        title: display.clone(),
                        detail: format!("{} · {}", count, masked_money(state, total, &cur)),
                        level: TimelineLevel::Month,
                        default_open: mi == 0,
                    },
                    &days_box,
                    &nodes,
                );
                month_anchors.borrow_mut().push(MonthAnchor::new(
                    format!("transfer:month:{month}"),
                    group.clone(),
                ));
                records_view.append(&group);
            }
        }
        Err(err) => records_view.append(&text(&format!("{err}"), "saifori-negative")),
    };
    let record_scroll = scroll(&records_view);
    record_scroll.set_min_content_height(220);
    record_scroll.add_css_class("saifori-transfer-list-scroll");
    history.append(&sticky_month_ledger(
        state,
        &record_scroll,
        &records_view,
        &month_anchors,
        &nodes,
        true,
    ));
    parent.append(&history);
}

fn confirm_delete_transfer(state: &Rc<State>, id: i64) {
    let popup = gtk::Window::builder()
        .title(i18n::tr("Delete transfer?"))
        .transient_for(&state.window)
        .modal(true)
        .resizable(false)
        .default_width(400)
        .build();
    let content = gap(Orientation::Vertical, 14);
    margin(&content, 22);
    content.append(&text("Delete transfer?", "saifori-section-title"));
    let row = gap(Orientation::Horizontal, 10);
    row.set_halign(gtk::Align::End);
    let cancel = button("Cancel", "");
    let w = popup.clone();
    cancel.connect_clicked(move |_| w.close());
    row.append(&cancel);
    let delete = button("Delete", "user-trash-symbolic");
    delete.add_css_class("destructive-action");
    let st = state.clone();
    let w = popup.clone();
    delete.connect_clicked(move |_| {
        let result = st.db.borrow_mut().delete_transfer(id);
        if run_write(&st, result) {
            w.close();
        }
    });
    row.append(&delete);
    content.append(&row);
    popup.set_child(Some(&content));
    popup.present();
}

fn activity_page(state: &Rc<State>, parent: &gtk::Box) {
    page_title(parent, "Activity", "");
    let listing = card();
    listing.set_vexpand(true);
    listing.append(&section_heading("Record history", ""));
    let nodes: TimelineNodes = timeline_nodes();
    let anchors: MonthAnchors = Rc::new(RefCell::new(Vec::new()));
    let records_view = gap(Orientation::Vertical, 0);
    records_view.set_vexpand(false);
    records_view.set_valign(gtk::Align::Start);
    listing.append(&timeline_controls(state, &nodes));
    match state.db.borrow().activities(200) {
        Ok(records) if records.is_empty() => {
            records_view.append(&text("No activity recorded yet.", "saifori-muted"))
        }
        Ok(records) => {
            let mut months: BTreeMap<String, BTreeMap<String, Vec<model::Activity>>> =
                BTreeMap::new();
            for event in records {
                let day = event.created_at.get(..10).unwrap_or("Unknown").to_string();
                let month = day.get(..7).unwrap_or("Unknown").to_string();
                months
                    .entry(month)
                    .or_default()
                    .entry(day)
                    .or_default()
                    .push(event);
            }
            for (mi, (month, days)) in months.iter().rev().enumerate() {
                let days_box = gap(Orientation::Vertical, 3);
                days_box.set_vexpand(false);
                days_box.set_valign(gtk::Align::Start);
                days_box.add_css_class("saifori-timeline-days");
                for (di, (day, records)) in days.iter().rev().enumerate() {
                    let rows = gap(Orientation::Vertical, 3);
                    rows.set_vexpand(false);
                    rows.set_valign(gtk::Align::Start);
                    for event in records {
                        let row = gap(Orientation::Horizontal, 12);
                        row.add_css_class("saifori-list-row");
                        row.append(&metric_icon(
                            if event.entity == "transfer" {
                                "mail-send-receive-symbolic"
                            } else {
                                "document-open-recent-symbolic"
                            },
                            "saifori-neutral-icon",
                        ));
                        let details = gap(Orientation::Vertical, 3);
                        details.set_hexpand(true);
                        details.append(&text(
                            &format!(
                                "{} · {}",
                                i18n::tr(&event.action),
                                masked(state, &event.label)
                            ),
                            "",
                        ));
                        details
                            .append(&text(&format!("{} UTC", event.created_at), "saifori-muted"));
                        row.append(&details);
                        let value = text(
                            &masked_money(state, event.amount_minor, &event.currency),
                            "saifori-amount",
                        );
                        value.set_wrap(false);
                        row.append(&value);
                        rows.append(&row);
                    }
                    days_box.append(&timeline_group(
                        state,
                        TimelineGroup {
                            key: format!("activity:day:{day}"),
                            title: friendly_day(day),
                            detail: format!("{}", records.len()),
                            level: TimelineLevel::Day,
                            default_open: mi == 0 && di == 0,
                        },
                        &rows,
                        &nodes,
                    ));
                }
                let count = days.values().map(Vec::len).sum::<usize>();
                let key = format!("activity:month:{month}");
                let title = i18n::display_month(month);
                let summary = format!("{}", count);
                let group = timeline_group(
                    state,
                    TimelineGroup {
                        key: key.clone(),
                        title: title.clone(),
                        detail: summary.clone(),
                        level: TimelineLevel::Month,
                        default_open: mi == 0,
                    },
                    &days_box,
                    &nodes,
                );
                anchors
                    .borrow_mut()
                    .push(MonthAnchor::new(key, group.clone()));
                records_view.append(&group);
            }
        }
        Err(err) => records_view.append(&text(&err.to_string(), "saifori-negative")),
    };
    let scroller = scroll(&records_view);
    scroller.set_min_content_height(200);
    scroller.add_css_class("saifori-activity-list-scroll");
    listing.append(&sticky_month_ledger(
        state,
        &scroller,
        &records_view,
        &anchors,
        &nodes,
        false,
    ));
    parent.append(&listing);
}

fn help_page(_state: &Rc<State>, parent: &gtk::Box) {
    page_title(parent, "Help & FAQ", "");
    let faq = card();
    let questions=[
        ("How do I add an expense?","Use Add Transaction, enter the amount and category, then save."),
        ("How do transfers work?","Transfers are recorded separately. They do not count as income or expenses, and do not adjust account balances in this version."),
        ("When was a transaction added?","Open a transaction to see its Created timestamp, or visit Activity."),
        ("How does privacy mode work?","The eye button masks names and amounts on screen. Your database and exports are not encrypted."),
        ("Where is my AI key?","The selected provider key is kept in GNOME Keyring and loaded after restarting. Saifori never displays a saved key; you can replace or remove it from Settings. Unlock the system keyring if requested."),
        ("What is sent to the AI provider?","Only the prompt you submit and aggregated totals and categories; Smart Capture sends only the text you explicitly submit."),
        ("How are dates handled in Smart Capture?","Explicit dates take priority. ISO YYYY-MM-DD, DD/MM/YYYY and relative dates such as yesterday are supported. Always review dates before saving."),
        ("What are Smart Capture limits?","One request can contain up to 500 words (4,000 characters). Review up to 50 drafts at once. Save or remove drafts before importing more; no data is silently trimmed."),
        ("Can I edit the quantity and category?","Yes. Select Edit draft before Save selected. Unit price × quantity becomes the total, calculated in exact cents."),
        ("Where are my conversations?","Each conversation is saved in the local SQLite database. Use History to open past chats, New chat to start another, or Delete to remove one. Chat messages are not encrypted at rest and are not in finance backup exports."),
        ("How do tabs work?","Enable or disable separate Chat and Smart Capture tabs in Settings. In the tab layout the composer remains visible when messages scroll."),
        ("How do I review months and dates?","Draft and transaction lists are grouped by month and day. Expand or collapse a group; the date calendar in Transactions filters a specific day."),
        ("How are product totals calculated?","Products uses expense descriptions, case-insensitive, to combine matching entries. Review descriptions for consistent naming. Transfers and income are excluded."),
        ("How do I change language?","Choose English or Español in Settings. Translation catalogs are in locales/."),
        ("Where is my data stored?","In your private local application data directory. Use Settings to create a backup."),
        ("How do I restore my records?","In Settings choose Restore Backup. Check the confirmation carefully: restore replaces local financial records."),
    ];
    for (question, answer) in questions {
        let exp = gtk::Expander::new(Some(&i18n::tr(question)));
        let response = text(answer, "saifori-muted");
        response.set_margin_top(12);
        response.set_margin_bottom(12);
        exp.set_child(Some(&response));
        faq.append(&exp);
        faq.append(&line());
    }
    parent.append(&faq);
}

#[cfg(test)]
mod chat_render_tests {
    use super::safe_chat_markup;
    #[test]
    fn escapes_untrusted_ai_markup() {
        assert_eq!(
            safe_chat_markup("<img src=bad> **684.00 MAD**"),
            "&lt;img src=bad&gt; <b>684.00 MAD</b>"
        );
    }
}
