// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Joonhoe Kim
//! sp9-settings: GTK4/libadwaita settings app for sp9-helperd (Surface Pro 9, Intel).
//!
//! A thin front-end in the pattern of tb323fu-settings: every value comes from
//! the helper over the system D-Bus, every change is a method call, and the
//! helper decides privileges through polkit (calls allow interactive
//! authorization). Polls run on a worker thread: at start, after each call,
//! whenever the helper sends PropertiesChanged or its name changes owner, and
//! every 10 s for sensor values. Pages whose object the helper does not export
//! (Root.Features) are hidden; without the helper, or with another device's
//! helper on the same bus name, a status page says so.

mod dbus;
mod i18n;

use adw::prelude::*;
use dbus::{Bus, Client, Props};
use gtk::{gio, glib};
use i18n::{n_, tr, trf};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

const APP_ID: &str = "io.github.joonhoekim.OpenDeviceHelper.SurfacePro9";
const REPO: &str = "https://github.com/joonhoekim/surface-pro-9-linux";
const OBJECTS: [&str; 6] = ["Performance", "Battery", "Display", "Input", "Pen", "Diagnostics"];
const DEBOUNCE: Duration = Duration::from_millis(400);
const POLL_SECONDS: u32 = 10;

/// platform_profile names (surface_platform_profile) and their labels.
const PROFILE_LABELS: [(&str, &str); 6] = [
    ("low-power", n_("Low Power")),
    ("balanced", n_("Balanced")),
    ("balanced-performance", n_("Balanced Performance")),
    ("performance", n_("Performance")),
    ("quiet", n_("Quiet")),
    ("cool", n_("Cool")),
];
/// Profiles with a row in Performance.LimitTable.
const TABLE_PROFILES: [&str; 4] = ["low-power", "balanced", "balanced-performance", "performance"];
const POWERS: [(&str, &str); 2] = [("ac", n_("On Charger")), ("dc", n_("On Battery"))];
const PSR_MODES: [(&str, &str); 3] = [("default", n_("Driver Default")), ("off", n_("Off")), ("psr1", n_("PSR1 Only"))];
/// iptsd contact thresholds (activation, deactivation); negative = the device preset.
const IPTSD_PRESETS: [(&str, f64, f64); 4] = [
    (n_("Device Preset (24/20)"), -1.0, -1.0),
    (n_("More Sensitive (20/16)"), 20.0, 16.0),
    (n_("Less Sensitive (32/28)"), 32.0, 28.0),
    (n_("iptsd Default (40/36)"), 40.0, 36.0),
];
const STYLUS_MODES: [(&str, i32); 3] = [(n_("Device Preset (On)"), -1), (n_("On"), 1), (n_("Off"), 0)];
const BATTERY_STATUS: [(&str, &str); 5] = [
    ("Charging", n_("Charging")),
    ("Discharging", n_("Discharging")),
    ("Full", n_("Full")),
    ("Not charging", n_("Not Charging")),
    ("Unknown", n_("Unknown")),
];
const PAGE_NAMES: [&str; 7] = ["performance", "battery", "display", "input", "pen", "diagnostics", "about"];

const CSS: &str = "
.tag { font-size: smaller; font-weight: bold; padding: 2px 8px; border-radius: 999px;
       background-color: alpha(currentColor, 0.12); }
.howto { font-family: monospace; }
";

// ---------------------------------------------------------------- widgets --

/// A page body: scrolls vertically, rows clamped to 760 px.
fn page_box() -> (gtk::ScrolledWindow, gtk::Box) {
    let bx = gtk::Box::new(gtk::Orientation::Vertical, 24);
    bx.set_margin_top(24);
    bx.set_margin_bottom(24);
    bx.set_margin_start(12);
    bx.set_margin_end(12);
    let clamp = adw::Clamp::builder().maximum_size(760).tightening_threshold(600).child(&bx).build();
    let sw = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&clamp).vexpand(true).build();
    (sw, bx)
}

fn group(page: &gtk::Box, title: &str, desc: &str) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    if !title.is_empty() {
        g.set_title(title);
    }
    if !desc.is_empty() {
        g.set_description(Some(desc));
    }
    page.append(&g);
    g
}

/// A "?" button in the group header whose popover holds the longer text.
fn group_help(g: &adw::PreferencesGroup, text: &str) {
    let l = gtk::Label::new(Some(text));
    l.set_wrap(true);
    l.set_max_width_chars(44);
    l.set_xalign(0.0);
    l.set_margin_top(6);
    l.set_margin_bottom(6);
    l.set_margin_start(6);
    l.set_margin_end(6);
    let pop = gtk::Popover::builder().child(&l).build();
    let b = gtk::MenuButton::builder().icon_name("help-about-symbolic").popover(&pop).valign(gtk::Align::Center).build();
    b.add_css_class("flat");
    b.set_tooltip_text(Some(tr("More Information")));
    g.set_header_suffix(Some(&b));
}

/// A short read-only value at the end of the row.
fn info(g: &adw::PreferencesGroup, title: &str) -> gtk::Label {
    let row = adw::ActionRow::builder().title(title).build();
    let l = gtk::Label::new(Some("…"));
    l.add_css_class("dim-label");
    l.set_wrap(false);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.set_xalign(1.0);
    l.set_valign(gtk::Align::Center);
    row.add_suffix(&l);
    g.add(&row);
    l
}

/// A long read-only value under the row title with a copy button.
#[derive(Clone)]
struct LongInfo {
    row: adw::ActionRow,
    full: Rc<RefCell<String>>,
}

impl LongInfo {
    fn set(&self, shown: &str, full: &str) {
        if self.row.subtitle().as_deref() != Some(shown) {
            self.row.set_subtitle(shown);
        }
        if *self.full.borrow() != full {
            *self.full.borrow_mut() = full.to_string();
            self.row.set_tooltip_text(Some(full));
        }
    }
}

fn info_long(g: &adw::PreferencesGroup, title: &str, toasts: &adw::ToastOverlay) -> LongInfo {
    let row = adw::ActionRow::builder().title(title).subtitle("…").build();
    row.add_css_class("property");
    row.set_subtitle_lines(2);
    let full = Rc::new(RefCell::new(String::new()));
    let b = gtk::Button::from_icon_name("edit-copy-symbolic");
    b.add_css_class("flat");
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(tr("Copy")));
    b.update_property(&[gtk::accessible::Property::Label(&trf("Copy {}", &[&title]))]);
    let (f2, t2) = (full.clone(), toasts.clone());
    b.connect_clicked(move |b| {
        b.clipboard().set_text(&f2.borrow());
        t2.add_toast(adw::Toast::builder().title(tr("Copied")).timeout(2).build());
    });
    row.add_suffix(&b);
    g.add(&row);
    LongInfo { row, full }
}

/// A row of fixed, selectable text (commands to type), not a setting.
fn howto(g: &adw::PreferencesGroup, title: &str, text: &str) {
    let row = adw::ActionRow::builder().title(title).subtitle(text).build();
    row.set_subtitle_lines(0);
    row.set_subtitle_selectable(true);
    row.add_css_class("property");
    row.add_css_class("howto");
    g.add(&row);
}

fn spin(g: &adw::PreferencesGroup, title: &str, subtitle: &str, lo: f64, hi: f64, step: f64) -> adw::SpinRow {
    let r = adw::SpinRow::with_range(lo, hi, step);
    r.set_title(title);
    if !subtitle.is_empty() {
        r.set_subtitle(subtitle);
    }
    g.add(&r);
    r
}

fn switch(g: &adw::PreferencesGroup, title: &str, subtitle: &str) -> adw::SwitchRow {
    let r = adw::SwitchRow::builder().title(title).build();
    if !subtitle.is_empty() {
        r.set_subtitle(subtitle);
    }
    g.add(&r);
    r
}

/// A combo row of translated labels.
fn combo(g: &adw::PreferencesGroup, title: &str, labels: &[&'static str]) -> adw::ComboRow {
    let shown: Vec<&str> = labels.iter().map(|l| tr(l)).collect();
    let r = adw::ComboRow::builder().title(title).model(&gtk::StringList::new(&shown)).build();
    g.add(&r);
    r
}

fn button(g: &adw::PreferencesGroup, title: &str, subtitle: &str, label: &str) -> (adw::ActionRow, gtk::Button) {
    let row = adw::ActionRow::builder().title(title).build();
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    let b = gtk::Button::with_label(label);
    b.set_valign(gtk::Align::Center);
    row.add_suffix(&b);
    row.set_activatable_widget(Some(&b));
    g.add(&row);
    (row, b)
}

fn set_text(l: &gtk::Label, t: &str) {
    if l.text() != t {
        l.set_text(t);
        l.set_tooltip_text(Some(t));
    }
}
fn set_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if w.has_css_class(class) != on {
        if on {
            w.add_css_class(class);
        } else {
            w.remove_css_class(class);
        }
    }
}
/// The spin row's text entry has keyboard focus: the user is typing.
fn editing(r: &adw::SpinRow) -> bool {
    r.root().and_then(|w| w.focus()).is_some_and(|f| f.is_ancestor(r))
}
fn set_spin(r: &adw::SpinRow, v: Option<f64>) {
    if let Some(v) = v {
        if !editing(r) && (r.value() - v).abs() > 1e-6 {
            r.set_value(v);
        }
    }
}
fn set_switch(r: &adw::SwitchRow, v: Option<bool>) {
    if let Some(v) = v {
        if r.is_active() != v {
            r.set_active(v);
        }
    }
}
fn set_selected(r: &adw::ComboRow, i: Option<usize>) {
    if let Some(i) = i {
        if r.selected() != i as u32 {
            r.set_selected(i as u32);
        }
    }
}
/// Replace a combo row's items when they differ from `labels`.
fn set_items(r: &adw::ComboRow, labels: &[String]) {
    let same = r.model().and_downcast::<gtk::StringList>().is_some_and(|m| {
        m.n_items() as usize == labels.len() && labels.iter().enumerate().all(|(i, l)| m.string(i as u32).as_deref() == Some(l.as_str()))
    });
    if !same {
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        r.set_model(Some(&gtk::StringList::new(&refs)));
    }
}

fn label_of(table: &[(&str, &'static str)], id: &str) -> String {
    table.iter().find(|(k, _)| *k == id).map(|(_, l)| tr(l).to_string()).unwrap_or_else(|| id.to_string())
}
fn profile_label(id: &str) -> String {
    label_of(&PROFILE_LABELS, id)
}
fn yes_no(v: Option<bool>) -> &'static str {
    match v {
        Some(true) => tr("Yes"),
        Some(false) => tr("No"),
        None => tr("Unknown"),
    }
}
/// Negative or NaN is "unknown" in the helper's API.
fn known(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x >= 0.0)
}
fn watts(v: Option<f64>) -> String {
    known(v).map(|w| format!("{w:.1} W")).unwrap_or_else(|| tr("Unknown").into())
}
fn degrees(v: Option<f64>) -> String {
    match v {
        Some(t) if t.is_finite() => format!("{t:.1} °C"),
        _ => tr("Unknown").into(),
    }
}
fn seconds(v: Option<f64>) -> String {
    known(v).map(|s| format!("{s:.0} s")).unwrap_or_else(|| tr("Unknown").into())
}

// --------------------------------------------------------------- pages --

struct Page {
    objects: &'static [&'static str],
    row: gtk::ListBoxRow,
    nav: adw::NavigationPage,
}

struct PerfUi {
    profile: adw::ComboRow,
    profile_ids: RefCell<Vec<String>>,
    power: gtk::Label,
    follow: adw::SwitchRow,
    epp_on: adw::SwitchRow,
    pl1: gtk::Label,
    pl2: gtk::Label,
    tau: gtk::Label,
    epp: gtk::Label,
    zones: LongInfo,
    lim_profile: adw::ComboRow,
    lim_power: adw::ComboRow,
    lim_pl1: adw::SpinRow,
    lim_pl2: adw::SpinRow,
    lim_tau: adw::SpinRow,
    lim_epp: adw::SpinRow,
    lim_save: gtk::Button,
    lim_reset: gtk::Button,
    lim_table: RefCell<Vec<dbus::Limit>>,
    /// the spin rows hold values the user typed and has not saved
    lim_dirty: Cell<bool>,
    skin: adw::SwitchRow,
    skin_state: gtk::Label,
    skin_temp: gtk::Label,
    temps: adw::ExpanderRow,
    temp_rows: RefCell<Vec<(String, gtk::Label, adw::ActionRow)>>,
    fan_rpm: gtk::Label,
    fan_allow: adw::SwitchRow,
    fan_target: adw::SpinRow,
    fan_row: adw::ActionRow,
    fan_send: gtk::Button,
    fan_release: gtk::Button,
    fan_req: gtk::Label,
}

struct BatUi {
    status: gtk::Label,
    capacity: gtk::Label,
    power: gtk::Label,
    voltage: gtk::Label,
    health: gtk::Label,
    cycles: gtk::Label,
    energy: gtk::Label,
    ac: gtk::Label,
    adapter: gtk::Label,
    sc_support: gtk::Label,
    sc_state: gtk::Label,
    sc_row: adw::ActionRow,
    sc_release: gtk::Button,
    uefi: gtk::Label,
}

struct DispUi {
    psr_ac: adw::ComboRow,
    psr_dc: adw::ComboRow,
    psr_boot: gtk::Label,
    psr_written: gtk::Label,
    psr_status: LongInfo,
    panel: gtk::Label,
    cal: gtk::Label,
    icc: LongInfo,
    icc_row: adw::ActionRow,
    icc_gen: gtk::Button,
    icc_open: gtk::Button,
    icc_path: RefCell<String>,
}

struct InputUi {
    kbd: adw::ComboRow,
    kbd_levels: RefCell<Vec<u32>>,
    kbd_none: adw::ActionRow,
    touch_off: adw::SwitchRow,
    touch_state: gtk::Label,
    sens: adw::ComboRow,
    stylus: adw::ComboRow,
    thresholds: gtk::Label,
    /// IptsdActivationThreshold, IptsdDeactivationThreshold, IptsdDisableOnStylus
    iptsd: Cell<(f64, f64, i32)>,
}

struct PenUi {
    avail: gtk::Label,
    ink: adw::SwitchRow,
    strength: adw::SpinRow,
    eraser: adw::SwitchRow,
    test: gtk::Button,
    speed: adw::SwitchRow,
    auto_couple: adw::SwitchRow,
    couple_state: gtk::Label,
    couple_row: adw::ActionRow,
    couple: gtk::Button,
}

struct DiagUi {
    last: LongInfo,
    export: gtk::Button,
    open_row: adw::ActionRow,
    open: gtk::Button,
}

struct AboutUi {
    device: gtk::Label,
    model: gtk::Label,
    version: gtk::Label,
    features: LongInfo,
    reload: gtk::Button,
    debug: RefCell<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Main,
    Absent,
    Other,
}

struct Ui {
    client: RefCell<Client>,
    updating: Cell<bool>,
    polling: Cell<bool>,
    poll_again: Cell<bool>,
    /// calls in flight, open dialogs and pending debounces per object: polls
    /// leave that object's widgets alone meanwhile
    pending: RefCell<HashMap<&'static str, u32>>,
    timers: RefCell<HashMap<&'static str, glib::SourceId>>,
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    other: adw::StatusPage,
    split: adw::NavigationSplitView,
    sidebar: gtk::ListBox,
    pages: Vec<Page>,
    selected: Cell<Option<usize>>,
    shown: Cell<Option<Shown>>,
    perf: PerfUi,
    bat: BatUi,
    disp: DispUi,
    input: InputUi,
    pen: PenUi,
    diag: DiagUi,
    about: AboutUi,
}

fn content_page(title: &str, child: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(child));
    adw::NavigationPage::builder().title(title).tag(title).child(&tv).build()
}

fn build_perf(toasts: &adw::ToastOverlay) -> (gtk::ScrolledWindow, PerfUi) {
    let (sw, b) = page_box();
    let g =
        group(&b, tr("Performance Profile"), tr("The firmware's thermal and fan profile, and the power limits the helper sets with it."));
    let profile = adw::ComboRow::builder().title(tr("Profile")).model(&gtk::StringList::new(&[])).build();
    g.add(&profile);
    let power = info(&g, tr("Power Source"));

    let g = group(&b, tr("Power Limits"), "");
    group_help(&g, tr("Each profile has a row per power source with the sustained limit (PL1), the boost limit (PL2), its time window (tau) and the EPP hint, taken from the Windows power policy of this device. The helper writes them to RAPL when the profile or the power source changes. A lower value set by the firmware still wins."));
    let follow = switch(&g, tr("Apply Profile Limits"), tr("Write PL1, PL2 and tau from the table on every profile or power change"));
    let epp_on = switch(&g, tr("Set EPP"), tr("Energy performance preference per profile (intel_pstate). Written again when another service replaces it right after a profile change."));
    let pl1 = info(&g, tr("PL1 (Sustained)"));
    let pl2 = info(&g, tr("PL2 (Boost)"));
    let tau = info(&g, tr("Time Window (tau)"));
    let epp = info(&g, tr("EPP"));
    let zones = info_long(&g, tr("RAPL Zones"), toasts);

    let g = group(&b, tr("Edit Limits"), tr("Change one row of the table. Values outside the firmware policy ranges are refused."));
    let ids: Vec<&'static str> =
        TABLE_PROFILES.iter().map(|p| PROFILE_LABELS.iter().find(|(k, _)| k == p).map(|x| x.1).unwrap_or(p)).collect();
    let lim_profile = combo(&g, tr("Profile"), &ids);
    let lim_power = combo(&g, tr("Power Source"), &POWERS.map(|p| p.1));
    let lim_pl1 = spin(&g, tr("PL1 (W)"), "5–35 W", 5.0, 35.0, 0.5);
    lim_pl1.set_digits(1);
    let lim_pl2 = spin(&g, tr("PL2 (W)"), tr("PL1–60 W"), 5.0, 60.0, 0.5);
    lim_pl2.set_digits(1);
    let lim_tau = spin(&g, tr("Time Window (s)"), "1–28 s", 1.0, 28.0, 1.0);
    let lim_epp = spin(&g, tr("EPP"), tr("0 performance – 255 power saving"), 0.0, 255.0, 1.0);
    let row = adw::ActionRow::builder().title(tr("This Row")).build();
    let lim_reset = gtk::Button::with_label(tr("Reset"));
    lim_reset.set_valign(gtk::Align::Center);
    lim_reset.add_css_class("flat");
    let lim_save = gtk::Button::with_label(tr("Save"));
    lim_save.set_valign(gtk::Align::Center);
    lim_save.add_css_class("suggested-action");
    row.add_suffix(&lim_reset);
    row.add_suffix(&lim_save);
    g.add(&row);

    let g = group(
        &b,
        tr("Skin Temperature Guard (Experimental)"),
        tr("Lowers the power limits when the back of the device gets hot, as Windows does. Off by default."),
    );
    group_help(&g, tr("Windows keeps the back cover sensor (VTS1) below 45–48 °C depending on the profile: above that it lowers PL1 step by step, and at 49, 50, 51 and 52 °C it caps PL2 at 35, 30, 20 and 15 W. The set points come from Windows; the control constants here do not. The sensor is channel temp9 of the surface_thermal hwmon, matched to VTS1 by its SAM sensor number and not yet by comparing readings. Without the guard the device can get warmer than on Windows under long loads."));
    let skin = switch(&g, tr("Skin Temperature Guard"), tr("Limits PL1 and PL2 from the back cover temperature"));
    let skin_state = info(&g, tr("State"));
    let skin_temp = info(&g, tr("Back Cover Sensor"));
    let g = group(&b, tr("Temperatures"), "");
    let temps = adw::ExpanderRow::builder().title(tr("All Sensors")).build();
    g.add(&temps);

    let g = group(
        &b,
        tr("Fan (Experimental)"),
        tr("The firmware runs the fan by itself. A request can spin up a fan the firmware has stopped, until it is released, the device suspends or the helper restarts."),
    );
    group_help(&g, tr("The request (SAM FAN 0x0b) sets the fan speed only while the firmware is not driving the fan itself, so it works when the device is idle or lightly loaded. Under load the firmware ignores it in both directions: it cannot slow the fan down and cannot speed it up. Suspend cancels it. The current request shows whether the fan is following it."));
    let fan_rpm = info(&g, tr("Fan Speed"));
    let fan_allow = switch(&g, tr("Allow Fan Requests"), tr("Asks for administrator authentication"));
    let fan_target = spin(&g, tr("Requested Speed (rpm)"), "1600–7500 rpm", 1600.0, 7500.0, 100.0);
    fan_target.set_value(3000.0);
    let fan_row = adw::ActionRow::builder().title(tr("Request")).build();
    let fan_release = gtk::Button::with_label(tr("Release"));
    fan_release.set_valign(gtk::Align::Center);
    fan_release.add_css_class("flat");
    let fan_send = gtk::Button::with_label(tr("Send"));
    fan_send.set_valign(gtk::Align::Center);
    fan_row.add_suffix(&fan_release);
    fan_row.add_suffix(&fan_send);
    g.add(&fan_row);
    let fan_req = info(&g, tr("Current Request"));

    let ui = PerfUi {
        profile,
        profile_ids: RefCell::new(Vec::new()),
        power,
        follow,
        epp_on,
        pl1,
        pl2,
        tau,
        epp,
        zones,
        lim_profile,
        lim_power,
        lim_pl1,
        lim_pl2,
        lim_tau,
        lim_epp,
        lim_save,
        lim_reset,
        lim_table: RefCell::new(Vec::new()),
        lim_dirty: Cell::new(false),
        skin,
        skin_state,
        skin_temp,
        temps,
        temp_rows: RefCell::new(Vec::new()),
        fan_rpm,
        fan_allow,
        fan_target,
        fan_row,
        fan_send,
        fan_release,
        fan_req,
    };
    (sw, ui)
}

fn build_battery() -> (gtk::ScrolledWindow, BatUi) {
    let (sw, b) = page_box();
    let g = group(&b, tr("Battery"), "");
    let status = info(&g, tr("Status"));
    let capacity = info(&g, tr("Charge"));
    let power = info(&g, tr("Power"));
    let voltage = info(&g, tr("Voltage"));
    let g = group(&b, tr("Health"), "");
    let health = info(&g, tr("Capacity Left"));
    let energy = info(&g, tr("Full Charge / Design"));
    let cycles = info(&g, tr("Charge Cycles"));
    let g = group(&b, tr("Charger"), "");
    let ac = info(&g, tr("Connected"));
    let adapter = info(&g, tr("Adapter Rating"));

    let g = group(&b, tr("Smart Charging"), tr("When the device stays plugged in for long, the firmware holds the charge near 80 %."));
    group_help(&g, tr("This is the firmware's own protection; Windows can only release it for one charge, and the firmware engages it again later by itself. The helper reads its state from the Surface Aggregator (surface_aggregator_cdev) and can release it the same way."));
    let sc_support = info(&g, tr("Support"));
    let sc_state = info(&g, tr("State"));
    let sc_row = adw::ActionRow::builder()
        .title(tr("Charge to 100 % This Time"))
        .subtitle(tr("Releases smart charging until the firmware engages it again"))
        .build();
    let sc_release = gtk::Button::with_label(tr("Release"));
    sc_release.set_valign(gtk::Align::Center);
    sc_row.add_suffix(&sc_release);
    g.add(&sc_row);

    let g = group(&b, tr("UEFI Battery Limit"), tr("Stops charging at 50 % for a device that is always plugged in. Read only here."));
    let uefi = info(&g, tr("State"));
    howto(
        &g,
        tr("How to Change It"),
        tr("Shut down, hold Volume Up and press Power to open the UEFI settings, then Boot configuration → Advanced options → Enable Battery Limit Mode. The helper never writes UEFI variables."),
    );
    let ui =
        BatUi { status, capacity, power, voltage, health, cycles, energy, ac, adapter, sc_support, sc_state, sc_row, sc_release, uefi };
    (sw, ui)
}

fn build_display(toasts: &adw::ToastOverlay) -> (gtk::ScrolledWindow, DispUi) {
    let (sw, b) = page_box();
    let g = group(
        &b,
        tr("Panel Self Refresh"),
        tr("PSR lets the panel keep the image while the GPU sleeps. Windows turns it off on the charger and uses PSR1 on battery."),
    );
    group_help(&g, tr("The helper writes the i915 debugfs switch when the power source changes, so changing it asks for administrator authentication. Driver Default writes nothing. If the screen flickers or freezes briefly with PSR, choose Off. The boot setting (i915.enable_psr) comes from the kernel command line."));
    let psr_ac = combo(&g, tr("On Charger"), &PSR_MODES.map(|p| p.1));
    let psr_dc = combo(&g, tr("On Battery"), &PSR_MODES.map(|p| p.1));
    let psr_boot = info(&g, tr("Boot Setting"));
    let psr_written = info(&g, tr("Last Written"));
    let psr_status = info_long(&g, tr("Driver State"), toasts);
    let panel = info(&g, tr("Panel"));

    let g = group(&b, tr("Color Profile"), tr("An ICC profile of the panel's own color gamut, made from its factory calibration in UEFI."));
    let cal = info(&g, tr("Calibration Data"));
    let icc = info_long(&g, tr("Profile File"), toasts);
    let icc_row =
        adw::ActionRow::builder().title(tr("Generate Profile")).subtitle(tr("Reads the calibration and writes the ICC file")).build();
    let icc_open = gtk::Button::with_label(tr("Open Folder"));
    icc_open.set_valign(gtk::Align::Center);
    icc_open.add_css_class("flat");
    let icc_gen = gtk::Button::with_label(tr("Generate"));
    icc_gen.set_valign(gtk::Align::Center);
    icc_row.add_suffix(&icc_open);
    icc_row.add_suffix(&icc_gen);
    g.add(&icc_row);
    howto(
        &g,
        tr("Apply in the Desktop"),
        tr("GNOME: Settings → Color → the built-in display → Add Profile → Import File. KDE: System Settings → Color Management."),
    );
    howto(
        &g,
        tr("Apply with colord"),
        "colormgr import-profile FILE\ncolormgr get-devices-by-kind display\ncolormgr device-add-profile DEVICE PROFILE\ncolormgr device-make-profile-default DEVICE PROFILE",
    );
    let ui = DispUi {
        psr_ac,
        psr_dc,
        psr_boot,
        psr_written,
        psr_status,
        panel,
        cal,
        icc,
        icc_row,
        icc_gen,
        icc_open,
        icc_path: RefCell::new(String::new()),
    };
    (sw, ui)
}

fn build_input() -> (gtk::ScrolledWindow, InputUi) {
    let (sw, b) = page_box();
    let g = group(&b, tr("Type Cover Backlight"), "");
    let kbd = adw::ComboRow::builder().title(tr("Level")).model(&gtk::StringList::new(&[])).build();
    g.add(&kbd);
    let kbd_none = adw::ActionRow::builder().title(tr("No Type Cover attached")).build();
    kbd_none.add_css_class("dim-label");
    g.add(&kbd_none);

    let g = group(&b, tr("Touch Screen"), "");
    let touch_off =
        switch(&g, tr("Ignore Touch While the Screen Is Off"), tr("As on Windows. Saves no power: the touch controller stays on"));
    let touch_state = info(&g, tr("Touch Input"));

    let g =
        group(&b, tr("Touch Sensitivity"), tr("iptsd contact thresholds on top of the device preset. iptsd restarts when they change."));
    group_help(&g, tr("A contact counts as a touch above the activation threshold and ends below the deactivation threshold. Lower values catch lighter touches and also more accidental ones. These presets are starting points; the best values for this panel have not been measured yet."));
    let mut sens_labels: Vec<&'static str> = IPTSD_PRESETS.iter().map(|p| p.0).collect();
    sens_labels.push(n_("Custom"));
    let sens = combo(&g, tr("Sensitivity"), &sens_labels);
    let stylus = combo(&g, tr("Ignore Touch Near the Pen"), &STYLUS_MODES.map(|p| p.0));
    let thresholds = info(&g, tr("Thresholds"));

    let ui = InputUi {
        kbd,
        kbd_levels: RefCell::new(Vec::new()),
        kbd_none,
        touch_off,
        touch_state,
        sens,
        stylus,
        thresholds,
        iptsd: Cell::new((-1.0, -1.0, -1)),
    };
    (sw, ui)
}

fn build_pen() -> (gtk::ScrolledWindow, PenUi) {
    let (sw, b) = page_box();
    let g = group(&b, tr("Slim Pen 2 Haptics"), tr("Short vibrations in the pen while it writes, as on Windows."));
    let avail = info(&g, tr("Pen"));
    let ink = switch(&g, tr("Ink Feedback"), tr("Vibrates while the pen tip touches the screen"));
    let strength = spin(&g, tr("Strength"), tr("Percent; 100 is the Windows default"), 2.0, 200.0, 5.0);
    let eraser = switch(&g, tr("Eraser Click"), tr("One click when the eraser end touches the screen; it has no continuous vibration"));
    let (_, test) = button(&g, tr("Try It"), tr("One click in the pen"), tr("Test"));
    let g = group(
        &b,
        tr("Writing Texture"),
        tr("The pen vibrates like pen on paper only while the screen knows how fast the pen moves. The helper tells it."),
    );
    group_help(&g, tr("The pen plays its continuous vibrations only while the touch controller has a pen speed from the computer. With this on, the helper computes the speed from the pen's movement and sends it, as Windows does. It works while Ink Feedback is on. Do not run the separate surface-pen-feedback service at the same time."));
    let speed = switch(&g, tr("Send Pen Speed"), tr("Needed for the continuous vibration while writing"));
    let g = group(
        &b,
        tr("Pairing Through the Cover"),
        tr("A pen placed in the keyboard cover's slot is paired with this computer, as on Windows."),
    );
    group_help(&g, tr("The cover hands the pen a key and the helper pairs with it over Bluetooth. A pen that was paired on Windows afterwards has a new key: put it back into the slot, or pair it again here. Needs a kernel with LE legacy OOB pairing."));
    let auto_couple = switch(&g, tr("Pair Automatically"), tr("When a pen is placed in the cover"));
    let couple_state = info(&g, tr("Last Pairing"));
    let (couple_row, couple) =
        button(&g, tr("Pair Now"), tr("Pairs the pen that is in the cover; a pairing that works is kept"), tr("Pair"));
    (sw, PenUi { avail, ink, strength, eraser, test, speed, auto_couple, couple_state, couple_row, couple })
}

fn build_diag(toasts: &adw::ToastOverlay) -> (gtk::ScrolledWindow, DiagUi) {
    let (sw, b) = page_box();
    let g = group(
        &b,
        tr("Diagnostics Report"),
        tr("Runs the read-only post-install check and packs the results. MAC addresses, UUIDs and serial numbers are masked."),
    );
    let (_, export) = button(&g, tr("Export Report"), tr("Takes up to a minute"), tr("Export"));
    export.add_css_class("suggested-action");
    let last = info_long(&g, tr("Last Report"), toasts);
    let (open_row, open) = button(&g, tr("Report Folder"), "", tr("Open Folder"));
    open.add_css_class("flat");
    (sw, DiagUi { last, export, open_row, open })
}

fn build_about(toasts: &adw::ToastOverlay) -> (gtk::ScrolledWindow, AboutUi) {
    let (sw, b) = page_box();
    let g = group(&b, tr("Device"), "");
    let device = info(&g, tr("Device"));
    let model = info(&g, tr("Model"));
    let g = group(&b, tr("Helper"), "");
    let version = info(&g, tr("Helper Version"));
    let app = info(&g, tr("App Version"));
    set_text(&app, env!("CARGO_PKG_VERSION"));
    let features = info_long(&g, tr("Features"), toasts);
    let (_, reload) = button(&g, tr("Reload Settings File"), "/etc/sp9/helper.toml", tr("Reload"));
    (sw, AboutUi { device, model, version, features, reload, debug: RefCell::new(String::new()) })
}

impl Ui {
    fn new(app: &adw::Application, bus: Bus) -> Rc<Self> {
        let toasts = adw::ToastOverlay::new();
        let (p_perf, perf) = build_perf(&toasts);
        let (p_bat, bat) = build_battery();
        let (p_disp, disp) = build_display(&toasts);
        let (p_in, input) = build_input();
        let (p_pen, pen) = build_pen();
        let (p_diag, diag) = build_diag(&toasts);
        let (p_about, about) = build_about(&toasts);

        let sidebar = gtk::ListBox::new();
        sidebar.add_css_class("navigation-sidebar");
        sidebar.set_selection_mode(gtk::SelectionMode::Single);
        let defs: [(&str, &str, &'static [&'static str], &gtk::ScrolledWindow); 7] = [
            (tr("Performance"), "power-profile-balanced-symbolic", &["Performance"], &p_perf),
            (tr("Battery"), "battery-good-symbolic", &["Battery"], &p_bat),
            (tr("Display"), "video-display-symbolic", &["Display"], &p_disp),
            (tr("Keyboard & Touch"), "input-keyboard-symbolic", &["Input"], &p_in),
            (tr("Pen"), "input-tablet-symbolic", &["Pen"], &p_pen),
            (tr("Diagnostics"), "preferences-system-details-symbolic", &["Diagnostics"], &p_diag),
            (tr("About"), "help-about-symbolic", &[""], &p_about),
        ];
        let mut pages = Vec::new();
        for (title, icon, objects, page) in defs {
            let bx = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            bx.append(&gtk::Image::from_icon_name(icon));
            let l = gtk::Label::new(Some(title));
            l.set_xalign(0.0);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            bx.append(&l);
            let row = gtk::ListBoxRow::builder().child(&bx).build();
            sidebar.append(&row);
            pages.push(Page { objects, row, nav: content_page(title, page) });
        }
        let scroller = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).child(&sidebar).build();
        let menu = gio::Menu::new();
        menu.append(Some(tr("Keyboard Shortcuts")), Some("app.shortcuts"));
        menu.append(Some(tr("About Surface Pro 9 Settings")), Some("app.about"));
        let menu_btn = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&menu).primary(true).build();
        menu_btn.set_tooltip_text(Some(tr("Main Menu")));
        let side_header = adw::HeaderBar::new();
        side_header.pack_end(&menu_btn);
        let side_tv = adw::ToolbarView::new();
        side_tv.add_top_bar(&side_header);
        side_tv.set_content(Some(&scroller));
        let side_page = adw::NavigationPage::builder().title(tr("Surface Pro 9")).child(&side_tv).build();

        let split = adw::NavigationSplitView::new();
        split.set_sidebar(Some(&side_page));
        split.set_content(Some(&pages[0].nav));
        split.set_min_sidebar_width(200.0);
        split.set_max_sidebar_width(260.0);
        split.set_sidebar_width_fraction(0.24);

        let retry = gtk::Button::with_label(tr("Try Again"));
        retry.add_css_class("pill");
        retry.add_css_class("suggested-action");
        retry.set_halign(gtk::Align::Center);
        let absent = adw::StatusPage::builder()
            .icon_name("dialog-warning-symbolic")
            .title(tr("Helper Service Not Running"))
            .description(tr("This app needs sp9-helperd. Start it with <tt>systemctl start sp9-helperd</tt>"))
            .child(&retry)
            .build();
        let absent_tv = adw::ToolbarView::new();
        absent_tv.add_top_bar(&adw::HeaderBar::new());
        absent_tv.set_content(Some(&absent));
        let other = adw::StatusPage::builder().icon_name("dialog-warning-symbolic").title(tr("Another Device's Helper")).build();
        let other_tv = adw::ToolbarView::new();
        other_tv.add_top_bar(&adw::HeaderBar::new());
        other_tv.set_content(Some(&other));

        let stack = gtk::Stack::new();
        stack.add_named(&split, Some("main"));
        stack.add_named(&absent_tv, Some("absent"));
        stack.add_named(&other_tv, Some("other"));
        toasts.set_child(Some(&stack));
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(tr("Surface Pro 9 Settings"))
            .default_width(900)
            .default_height(760)
            .content(&toasts)
            .build();
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 700sp").unwrap());
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(bp);

        let ui = Rc::new(Ui {
            client: RefCell::new(Client::connect(bus)),
            updating: Cell::new(false),
            polling: Cell::new(false),
            poll_again: Cell::new(false),
            pending: RefCell::new(HashMap::new()),
            timers: RefCell::new(HashMap::new()),
            window,
            toasts,
            stack,
            other,
            split,
            sidebar,
            pages,
            selected: Cell::new(None),
            shown: Cell::new(None),
            perf,
            bat,
            disp,
            input,
            pen,
            diag,
            about,
        });
        let u = ui.clone();
        retry.connect_clicked(move |_| u.refresh_now());
        ui.connect_nav();
        ui.connect_perf();
        ui.connect_battery();
        ui.connect_display();
        ui.connect_input();
        ui.connect_pen();
        ui.connect_diag();
        ui
    }

    fn toast(&self, msg: &str) {
        self.toasts.add_toast(adw::Toast::new(msg));
    }

    fn hold(&self, obj: &'static str) {
        *self.pending.borrow_mut().entry(obj).or_insert(0) += 1;
    }
    fn release(&self, obj: &'static str) {
        let mut p = self.pending.borrow_mut();
        if let Some(n) = p.get_mut(obj) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                p.remove(obj);
            }
        }
    }
    fn busy(&self, obj: &str) -> bool {
        self.pending.borrow().get(obj).is_some_and(|n| *n > 0)
    }

    /// Run a method call off the main thread (polkit may show a dialog); toast
    /// errors with the setting's name, hand the result to `done`, then poll.
    fn call_then<B, F>(self: &Rc<Self>, obj: &'static str, method: &'static str, body: B, done: F)
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType + Send + 'static,
        F: FnOnce(&Rc<Ui>, Result<Option<String>, String>) + 'static,
    {
        let client = self.client.borrow().clone();
        let ui = self.clone();
        self.hold(obj);
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || client.call(obj, method, &body))
                .await
                .unwrap_or_else(|_| Err(tr("Something went wrong").into()));
            ui.release(obj);
            if let Err(e) = &res {
                ui.toast(&format!("{}: {e}", dbus::setting_name(method)));
            }
            done(&ui, res);
            ui.refresh();
        });
    }

    fn call_simple<B>(self: &Rc<Self>, obj: &'static str, method: &'static str, body: B)
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType + Send + 'static,
    {
        self.call_then(obj, method, body, |_, _| {});
    }

    /// Run `f` 400 ms after the last change under `key`; the object counts as busy meanwhile.
    fn debounce(self: &Rc<Self>, key: &'static str, obj: &'static str, f: impl FnOnce(&Rc<Ui>) + 'static) {
        let mut timers = self.timers.borrow_mut();
        if let Some(id) = timers.remove(key) {
            id.remove();
        } else {
            self.hold(obj);
        }
        let ui = self.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            ui.timers.borrow_mut().remove(key);
            ui.release(obj);
            f(&ui);
        });
        timers.insert(key, id);
    }

    fn switch_sends(self: &Rc<Self>, r: &adw::SwitchRow, obj: &'static str, method: &'static str) {
        let ui = self.clone();
        r.connect_active_notify(move |r| {
            if !ui.updating.get() {
                ui.call_simple(obj, method, (r.is_active(),));
            }
        });
    }

    /// Ask before a risky change; the object stays busy while the dialog is
    /// open, and Cancel runs `cancel` (puts the widget back).
    #[allow(clippy::too_many_arguments)]
    fn confirm(
        self: &Rc<Self>,
        obj: &'static str,
        heading: &str,
        body: &str,
        verb: &str,
        destructive: bool,
        go: impl Fn(&Rc<Ui>) + 'static,
        cancel: impl Fn(&Rc<Ui>) + 'static,
    ) {
        self.hold(obj);
        let d = adw::AlertDialog::new(Some(heading), Some(body));
        d.add_response("cancel", tr("Cancel"));
        d.add_response("go", verb);
        d.set_response_appearance(
            "go",
            if destructive { adw::ResponseAppearance::Destructive } else { adw::ResponseAppearance::Suggested },
        );
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let ui = self.clone();
        d.connect_response(None, move |_, resp| {
            ui.release(obj);
            if resp == "go" {
                go(&ui)
            } else {
                cancel(&ui)
            }
        });
        d.present(Some(&self.window));
    }

    /// A switch that asks before turning on (off goes straight through).
    fn confirm_on(
        self: &Rc<Self>,
        r: &adw::SwitchRow,
        obj: &'static str,
        method: &'static str,
        heading: &'static str,
        body: &'static str,
        verb: &'static str,
    ) {
        let ui = self.clone();
        r.connect_active_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            if !r.is_active() {
                ui.call_simple(obj, method, (false,));
                return;
            }
            let r2 = r.clone();
            ui.confirm(
                obj,
                tr(heading),
                tr(body),
                tr(verb),
                true,
                move |ui| ui.call_simple(obj, method, (true,)),
                move |ui| {
                    ui.updating.set(true);
                    r2.set_active(false);
                    ui.updating.set(false);
                },
            );
        });
    }

    fn open_folder(self: &Rc<Self>, path: &str) {
        if path.is_empty() {
            return;
        }
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
        let ui = self.clone();
        launcher.open_containing_folder(Some(&self.window), None::<&gio::Cancellable>, move |r| {
            if let Err(e) = r {
                ui.toast(&trf("Could not open the folder: {}", &[&e]));
            }
        });
    }

    // ------------------------------------------------------------ connect --

    fn connect_nav(self: &Rc<Self>) {
        let ui = self.clone();
        self.sidebar.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            if let Some(i) = ui.pages.iter().position(|p| &p.row == row) {
                ui.selected.set(Some(i));
                ui.split.set_content(Some(&ui.pages[i].nav));
                ui.split.set_show_content(true);
            }
        });
    }

    fn connect_perf(self: &Rc<Self>) {
        let p = &self.perf;
        let ui = self.clone();
        p.profile.connect_selected_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            let id = ui.perf.profile_ids.borrow().get(r.selected() as usize).cloned();
            if let Some(id) = id {
                ui.call_simple("Performance", "SetProfile", (id,));
            }
        });
        self.switch_sends(&p.follow, "Performance", "SetFollowProfile");
        self.switch_sends(&p.epp_on, "Performance", "SetEppEnabled");
        for r in [&p.lim_profile, &p.lim_power] {
            let ui = self.clone();
            r.connect_selected_notify(move |_| {
                ui.perf.lim_dirty.set(false);
                ui.fill_limits();
            });
        }
        for r in [&p.lim_pl1, &p.lim_pl2, &p.lim_tau, &p.lim_epp] {
            let ui = self.clone();
            r.connect_value_notify(move |_| {
                if !ui.updating.get() {
                    ui.perf.lim_dirty.set(true);
                }
            });
        }
        let ui = self.clone();
        p.lim_save.connect_clicked(move |_| {
            let (prof, power) = ui.limit_key();
            let p = &ui.perf;
            let body =
                (prof, power, p.lim_pl1.value(), p.lim_pl2.value(), p.lim_tau.value(), p.lim_epp.value().round().clamp(0.0, 255.0) as u8);
            ui.call_then("Performance", "SetLimits", body, |ui, r| {
                if r.is_ok() {
                    ui.perf.lim_dirty.set(false);
                    ui.toast(tr("Limits saved"));
                }
            });
        });
        let ui = self.clone();
        p.lim_reset.connect_clicked(move |_| {
            let key = ui.limit_key();
            ui.call_then("Performance", "ResetLimits", key, |ui, r| {
                if r.is_ok() {
                    ui.perf.lim_dirty.set(false);
                }
            });
        });
        self.confirm_on(
            &p.skin,
            "Performance",
            "SetSkinGuard",
            n_("Turn On the Skin Temperature Guard?"),
            n_("The guard is experimental: its control constants are not the ones Windows uses and its sensor has not been checked on this device. It only ever lowers the power limits."),
            n_("Turn On"),
        );
        self.confirm_on(
            &p.fan_allow,
            "Performance",
            "SetFanControlAllowed",
            n_("Allow Fan Requests?"),
            n_("Fan requests are experimental. A request only spins up a fan the firmware has stopped; the firmware ignores it while it drives the fan itself, so it cannot reduce cooling. Suspend and a restart of the helper cancel it."),
            n_("Allow"),
        );
        let ui = self.clone();
        p.fan_send.connect_clicked(move |_| {
            let v = ui.perf.fan_target.value();
            ui.call_simple("Performance", "RequestFanRpm", (v,));
        });
        let ui = self.clone();
        p.fan_release.connect_clicked(move |_| ui.call_simple("Performance", "RequestFanRpm", (0.0f64,)));
    }

    fn limit_key(&self) -> (String, String) {
        let p = &self.perf;
        let prof = TABLE_PROFILES.get(p.lim_profile.selected() as usize).unwrap_or(&TABLE_PROFILES[1]).to_string();
        let power = POWERS.get(p.lim_power.selected() as usize).map(|x| x.0).unwrap_or("ac").to_string();
        (prof, power)
    }

    /// Put the selected table row into the edit rows unless the user has unsaved values there.
    fn fill_limits(&self) {
        let p = &self.perf;
        if p.lim_dirty.get() {
            return;
        }
        let (prof, power) = self.limit_key();
        let row = p.lim_table.borrow().iter().find(|l| l.profile == prof && l.power == power).cloned();
        if let Some(l) = row {
            let was = self.updating.replace(true);
            set_spin(&p.lim_pl1, Some(l.pl1));
            set_spin(&p.lim_pl2, Some(l.pl2));
            set_spin(&p.lim_tau, Some(l.tau));
            set_spin(&p.lim_epp, Some(f64::from(l.epp)));
            self.updating.set(was);
        }
    }

    fn connect_battery(self: &Rc<Self>) {
        let ui = self.clone();
        self.bat.sc_release.connect_clicked(move |_| {
            ui.confirm(
                "Battery",
                tr("Charge to 100 % This Time?"),
                tr("Smart charging is released for this charge. The firmware engages it again by itself when the device stays plugged in for long."),
                tr("Release"),
                false,
                |ui| {
                    ui.call_then("Battery", "ReleaseSmartCharging", (), |ui, r| {
                        if let Ok(Some(s)) = r {
                            ui.toast(&release_text(&s));
                        }
                    })
                },
                |_| {},
            );
        });
    }

    fn connect_display(self: &Rc<Self>) {
        for r in [&self.disp.psr_ac, &self.disp.psr_dc] {
            let ui = self.clone();
            r.connect_selected_notify(move |_| {
                if ui.updating.get() {
                    return;
                }
                let id = |r: &adw::ComboRow| PSR_MODES.get(r.selected() as usize).map(|m| m.0).unwrap_or("default").to_string();
                let body = (id(&ui.disp.psr_ac), id(&ui.disp.psr_dc));
                ui.call_simple("Display", "SetPsrPolicy", body);
            });
        }
        let ui = self.clone();
        self.disp.icc_gen.connect_clicked(move |_| {
            ui.call_then("Display", "GenerateIcc", (), |ui, r| {
                if let Ok(Some(path)) = r {
                    ui.toast(&trf("Profile written: {}", &[&path]));
                    *ui.disp.icc_path.borrow_mut() = path;
                }
            });
        });
        let ui = self.clone();
        self.disp.icc_open.connect_clicked(move |_| {
            let p = ui.disp.icc_path.borrow().clone();
            ui.open_folder(&p);
        });
    }

    fn connect_input(self: &Rc<Self>) {
        let ui = self.clone();
        self.input.kbd.connect_selected_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            let level = ui.input.kbd_levels.borrow().get(r.selected() as usize).copied();
            if let Some(level) = level {
                ui.call_simple("Input", "SetKeyboardBacklight", (level,));
            }
        });
        self.switch_sends(&self.input.touch_off, "Input", "SetTouchOffWhenDisplayOff");
        let ui = self.clone();
        self.input.sens.connect_selected_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            // "Custom" is only a display of values set elsewhere
            if let Some((_, a, d)) = IPTSD_PRESETS.get(r.selected() as usize) {
                let (_, _, s) = ui.input.iptsd.get();
                ui.call_simple("Input", "SetIptsd", (*a, *d, s));
            }
        });
        let ui = self.clone();
        self.input.stylus.connect_selected_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            if let Some((_, s)) = STYLUS_MODES.get(r.selected() as usize) {
                let (a, d, _) = ui.input.iptsd.get();
                ui.call_simple("Input", "SetIptsd", (a, d, *s));
            }
        });
    }

    fn connect_pen(self: &Rc<Self>) {
        self.switch_sends(&self.pen.ink, "Pen", "SetInkFeedback");
        let ui = self.clone();
        self.pen.strength.connect_value_notify(move |r| {
            if ui.updating.get() {
                return;
            }
            let v = r.value().round().clamp(2.0, 200.0);
            ui.debounce("pen-strength", "Pen", move |ui| ui.call_simple("Pen", "SetStrength", (v,)));
        });
        let ui = self.clone();
        self.pen.test.connect_clicked(move |_| ui.call_simple("Pen", "TestPulse", ()));
        self.switch_sends(&self.pen.eraser, "Pen", "SetEraserClick");
        self.switch_sends(&self.pen.speed, "Pen", "SetSpeedFeedback");
        self.confirm_on(
            &self.pen.auto_couple,
            "Pen",
            "SetAutoCouple",
            n_("Pair Pens Automatically?"),
            n_("Any pen placed in the cover's slot is paired with this computer over Bluetooth without asking, and a pairing it already has with this computer is replaced when it no longer works, for example after the pen was paired on Windows."),
            n_("Turn On"),
        );
        let ui = self.clone();
        self.pen.couple.connect_clicked(move |_| ui.call_simple("Pen", "Couple", ()));
    }

    fn connect_diag(self: &Rc<Self>) {
        let ui = self.clone();
        self.diag.export.connect_clicked(move |b| {
            b.set_sensitive(false);
            ui.toast(tr("Collecting the report…"));
            let b = b.clone();
            ui.call_then("Diagnostics", "Run", (), move |ui, r| {
                b.set_sensitive(true);
                if let Ok(Some(path)) = r {
                    ui.toast(&trf("Report saved: {}", &[&path]));
                }
            });
        });
        let ui = self.clone();
        self.diag.open.connect_clicked(move |_| {
            let p = ui.diag.last.full.borrow().clone();
            ui.open_folder(&p);
        });
        let ui = self.clone();
        self.about.reload.connect_clicked(move |_| {
            ui.call_then("", "Reload", (), |ui, r| match r {
                Ok(Some(problems)) if !problems.trim().is_empty() => {
                    let d = adw::AlertDialog::new(
                        Some(tr("Settings File Has Problems")),
                        Some(&format!("{}\n\n{problems}", tr("These values were ignored and the defaults used instead:"))),
                    );
                    d.add_response("ok", tr("OK"));
                    d.present(Some(&ui.window));
                }
                Ok(_) => ui.toast(tr("Settings reloaded")),
                Err(_) => {}
            });
        });
    }

    fn about_dialog(self: &Rc<Self>) {
        let d = adw::AboutDialog::builder()
            .application_name(tr("Surface Pro 9 Settings"))
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .developer_name("Joonhoe Kim")
            .license_type(gtk::License::Gpl30)
            .website(REPO)
            .comments(tr("Device settings for the Surface Pro 9 (Intel) on Linux, through the sp9-helperd service."))
            .debug_info(self.about.debug.borrow().as_str())
            .build();
        d.present(Some(&self.window));
    }

    fn shortcuts(self: &Rc<Self>) {
        let d = adw::AlertDialog::new(Some(tr("Keyboard Shortcuts")), Some(tr("Ctrl+Q  Quit\nCtrl+W  Close the window")));
        d.add_response("ok", tr("OK"));
        d.present(Some(&self.window));
    }

    // -------------------------------------------------------------- polls --

    fn poll_client(&self) -> (Client, Vec<&'static str>) {
        if !self.client.borrow().connected() {
            let bus = self.client.borrow().bus();
            *self.client.borrow_mut() = Client::connect(bus);
        }
        let skip = OBJECTS.iter().copied().filter(|o| self.busy(o)).collect();
        (self.client.borrow().clone(), skip)
    }

    /// Poll on a worker thread, then update the widgets; asks while a poll
    /// runs fold into one more poll after it.
    fn refresh(self: &Rc<Self>) {
        if self.polling.replace(true) {
            self.poll_again.set(true);
            return;
        }
        let (c, skip) = self.poll_client();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let f = gio::spawn_blocking(move || fetch(&c, &skip)).await;
            ui.polling.set(false);
            if let Ok(f) = f {
                ui.apply(f);
            }
            if ui.poll_again.replace(false) {
                ui.refresh();
            }
        });
    }

    fn refresh_now(self: &Rc<Self>) {
        let (c, skip) = self.poll_client();
        self.apply(fetch(&c, &skip));
    }

    fn show(&self, s: Shown) {
        if self.shown.get() != Some(s) {
            self.shown.set(Some(s));
            self.stack.set_visible_child_name(match s {
                Shown::Main => "main",
                Shown::Absent => "absent",
                Shown::Other => "other",
            });
        }
    }

    fn apply(self: &Rc<Self>, f: Fetched) {
        let Fetched { root, props } = f;
        let Some(root) = root else {
            self.show(Shown::Absent);
            return;
        };
        let device = dbus::s(&root, "Device").unwrap_or_default();
        if device != dbus::DEVICE {
            self.other.set_description(Some(&trf(
                "The helper on this system bus is for another device ({}). This app works with sp9-helperd on a Surface Pro 9.",
                &[&device],
            )));
            self.show(Shown::Other);
            return;
        }
        self.show(Shown::Main);
        self.updating.set(true);
        let features = dbus::strs(&root, "Features");
        let present = |o: &str| o.is_empty() || features.iter().any(|f| f == o);
        let had_selection = self.sidebar.selected_row().is_some_and(|r| r.is_visible());
        for p in &self.pages {
            p.row.set_visible(p.objects.iter().any(|o| present(o)));
        }
        if !had_selection {
            let want = self.selected.get().and_then(|i| self.pages.get(i)).filter(|p| p.row.is_visible());
            if let Some(p) = want.or_else(|| self.pages.iter().find(|p| p.row.is_visible())) {
                self.sidebar.select_row(Some(&p.row));
            }
        }
        let p = |o: &str| if self.busy(o) { None } else { props.get(o).and_then(|x| x.as_ref()) };
        if let Some(x) = p("Performance") {
            self.update_perf(x);
        }
        if let Some(x) = p("Battery") {
            self.update_battery(x);
        }
        if let Some(x) = p("Display") {
            self.update_display(x);
        }
        if let Some(x) = p("Input") {
            self.update_input(x);
        }
        if let Some(x) = p("Pen") {
            self.update_pen(x);
        }
        if let Some(x) = p("Diagnostics") {
            let last = dbus::s(x, "LastReport").unwrap_or_default();
            let shown = if last.is_empty() { tr("None yet").to_string() } else { last.clone() };
            self.diag.last.set(&shown, &last);
            self.diag.open_row.set_visible(!last.is_empty());
        }
        self.update_about(&root, &features);
        self.updating.set(false);
    }

    fn update_perf(&self, x: &Props) {
        let p = &self.perf;
        let mut ids = dbus::strs(x, "ProfileChoices");
        let cur = dbus::s(x, "Profile").unwrap_or_default();
        if !cur.is_empty() && !ids.contains(&cur) {
            ids.push(cur.clone());
        }
        set_items(&p.profile, &ids.iter().map(|i| profile_label(i)).collect::<Vec<_>>());
        set_selected(&p.profile, ids.iter().position(|i| *i == cur));
        *p.profile_ids.borrow_mut() = ids;
        set_text(&p.power, if dbus::b(x, "OnAc") == Some(true) { tr("Charger") } else { tr("Battery") });
        set_switch(&p.follow, dbus::b(x, "FollowProfile"));
        set_switch(&p.epp_on, dbus::b(x, "EppEnabled"));
        let pair = |now: Option<f64>, target: Option<f64>, f: fn(Option<f64>) -> String| match known(target) {
            Some(_) => trf("{} (target {})", &[&f(now), &f(target)]),
            None => f(now),
        };
        set_text(&p.pl1, &pair(dbus::f(x, "Pl1W"), dbus::f(x, "TargetPl1W"), watts));
        set_text(&p.pl2, &pair(dbus::f(x, "Pl2W"), dbus::f(x, "TargetPl2W"), watts));
        set_text(&p.tau, &pair(dbus::f(x, "TauS"), dbus::f(x, "TargetTauS"), seconds));
        let mut epp = dbus::strs(x, "Epp");
        epp.dedup();
        let epp_now = if epp.is_empty() { tr("Unknown").to_string() } else { epp.join(", ") };
        let epp_text = match dbus::n(x, "TargetEpp").filter(|t| *t >= 0) {
            Some(t) => trf("{} (target {})", &[&epp_now, &t]),
            None => epp_now,
        };
        set_text(&p.epp, &epp_text);
        let zones = dbus::strs(x, "RaplZones");
        let z = if zones.is_empty() { tr("None found").to_string() } else { zones.join(", ") };
        p.zones.set(&z, &z);
        *p.lim_table.borrow_mut() = dbus::limits(x, "LimitTable");
        self.fill_limits();

        set_switch(&p.skin, dbus::b(x, "SkinGuard"));
        let state = match (dbus::b(x, "SkinGuard"), dbus::b(x, "SkinGuardActive")) {
            (Some(true), Some(true)) => tr("Limiting"),
            (Some(true), _) => tr("Watching"),
            _ => tr("Off"),
        };
        set_text(&p.skin_state, state);
        set_class(&p.skin_state, "warning", state == tr("Limiting"));
        let sensor = dbus::s(x, "SkinSensor").unwrap_or_default();
        let t = degrees(dbus::f(x, "SkinTempC"));
        set_text(&p.skin_temp, &if sensor.is_empty() { tr("Not found").to_string() } else { format!("{sensor}: {t}") });
        self.update_temps(&dbus::temps(x, "Temperatures"));

        let rpm = dbus::n(x, "FanRpm").filter(|r| *r >= 0);
        set_text(&p.fan_rpm, &rpm.map(|r| format!("{r} rpm")).unwrap_or_else(|| tr("Unknown").into()));
        let allowed = dbus::b(x, "FanControlAllowed").unwrap_or(false);
        set_switch(&p.fan_allow, Some(allowed));
        p.fan_target.set_sensitive(allowed);
        p.fan_row.set_sensitive(allowed);
        let req = dbus::f(x, "FanRpmRequest").filter(|r| *r > 0.0);
        let state = dbus::s(x, "FanRequestState").unwrap_or_default();
        let followed = match state.as_str() {
            "settling" => tr("waiting for the fan"),
            "honoured" => tr("the fan follows"),
            "not-honoured" => tr("not followed: the firmware is driving the fan, or a suspend cancelled it"),
            _ => tr("fan speed unknown"),
        };
        set_text(&p.fan_req, &req.map(|r| trf("{} rpm, {}", &[&format!("{r:.0}"), &followed])).unwrap_or_else(|| tr("None").into()));
        set_class(&p.fan_req, "warning", req.is_some() && state == "not-honoured");
        p.fan_release.set_sensitive(req.is_some());
        let _ = &p.fan_send;
    }

    fn update_temps(&self, temps: &[(String, f64)]) {
        let p = &self.perf;
        let mut rows = p.temp_rows.borrow_mut();
        let names: Vec<&str> = temps.iter().map(|t| t.0.as_str()).collect();
        if rows.iter().map(|r| r.0.as_str()).ne(names.iter().copied()) {
            for (_, _, r) in rows.drain(..) {
                p.temps.remove(&r);
            }
            for (name, _) in temps {
                let row = adw::ActionRow::builder().title(name.as_str()).build();
                let l = gtk::Label::new(None);
                l.add_css_class("dim-label");
                row.add_suffix(&l);
                p.temps.add_row(&row);
                rows.push((name.clone(), l, row));
            }
        }
        for ((_, l, _), (_, t)) in rows.iter().zip(temps) {
            set_text(l, &degrees(Some(*t)));
        }
        let hottest = temps.iter().map(|t| t.1).filter(|t| t.is_finite()).fold(f64::NAN, f64::max);
        let sub = if temps.is_empty() {
            tr("None found").to_string()
        } else {
            trf("{} sensors, hottest {}", &[&temps.len(), &degrees(Some(hottest))])
        };
        if p.temps.subtitle() != sub {
            p.temps.set_subtitle(&sub);
        }
        p.temps.set_enable_expansion(!temps.is_empty());
    }

    fn update_battery(&self, x: &Props) {
        let b = &self.bat;
        let present = dbus::b(x, "Present").unwrap_or(false);
        let status = dbus::s(x, "Status").unwrap_or_else(|| "Unknown".into());
        set_text(&b.status, &if present { label_of(&BATTERY_STATUS, &status) } else { tr("No battery found").to_string() });
        let num = |k: &str| dbus::n(x, k).filter(|v| *v >= 0);
        set_text(&b.capacity, &num("Capacity").map(|c| format!("{c} %")).unwrap_or_else(|| tr("Unknown").into()));
        set_text(&b.power, &watts(dbus::f(x, "PowerW")));
        set_text(&b.voltage, &known(dbus::f(x, "VoltageV")).map(|v| format!("{v:.2} V")).unwrap_or_else(|| tr("Unknown").into()));
        set_text(&b.health, &known(dbus::f(x, "HealthPercent")).map(|h| format!("{h:.0} %")).unwrap_or_else(|| tr("Unknown").into()));
        let wh = |k: &str| known(dbus::f(x, k)).map(|v| format!("{v:.1}")).unwrap_or_else(|| "?".into());
        set_text(&b.energy, &format!("{} / {} Wh", wh("EnergyFullWh"), wh("EnergyFullDesignWh")));
        set_text(&b.cycles, &num("CycleCount").map(|c| c.to_string()).unwrap_or_else(|| tr("Unknown").into()));
        set_text(&b.ac, yes_no(dbus::b(x, "AcOnline")));
        set_text(&b.adapter, &watts(dbus::f(x, "AdapterW")));

        let avail = dbus::b(x, "SmartChargingAvailable").unwrap_or(false);
        let supported = dbus::b(x, "SmartChargingSupported").unwrap_or(false);
        set_text(
            &b.sc_support,
            match (avail, supported) {
                (false, _) => tr("Unavailable (surface_aggregator_cdev not loaded)"),
                (true, false) => tr("Not supported by the firmware"),
                (true, true) => tr("Supported"),
            },
        );
        let engaged = dbus::b(x, "SmartChargingEngaged").unwrap_or(false);
        let countdown = dbus::n(x, "SmartChargingCountdownS").unwrap_or(0);
        let status = dbus::n(x, "SmartChargingStatus").unwrap_or(0);
        let state = if !avail || !supported {
            tr("Unknown").to_string()
        } else if engaged {
            tr("Holding near 80 %").to_string()
        } else if countdown > 0 {
            trf("Engages in {} min", &[&((countdown + 59) / 60)])
        } else {
            tr("Not engaged").to_string()
        };
        set_text(&b.sc_state, &state);
        b.sc_state.set_tooltip_text(Some(&format!("{state} (0x{status:04x})")));
        b.sc_row.set_sensitive(avail && supported && (engaged || countdown > 0));
        set_text(
            &b.uefi,
            match dbus::s(x, "UefiBatteryLimit").as_deref() {
                Some("on") => tr("On (stops at 50 %)"),
                Some("off") => tr("Off"),
                _ => tr("Unknown"),
            },
        );
    }

    fn update_display(&self, x: &Props) {
        let d = &self.disp;
        let idx = |v: Option<String>| v.and_then(|v| PSR_MODES.iter().position(|m| m.0 == v));
        set_selected(&d.psr_ac, idx(dbus::s(x, "PsrAc")));
        set_selected(&d.psr_dc, idx(dbus::s(x, "PsrDc")));
        let boot = match dbus::n(x, "EnablePsr") {
            Some(-1) => tr("Driver default (-1)").to_string(),
            Some(0) => tr("Off (0)").to_string(),
            Some(n) if n > 0 => format!("PSR{n} ({n})"),
            _ => tr("Unknown").to_string(),
        };
        set_text(&d.psr_boot, &boot);
        set_text(
            &d.psr_written,
            &match dbus::n(x, "PsrWritten") {
                Some(n) if n >= 0 => format!("0x{n:02x}"),
                _ => tr("Nothing written").to_string(),
            },
        );
        let st = dbus::s(x, "PsrStatus").unwrap_or_default();
        let first = st.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        d.psr_status.set(if first.is_empty() { tr("Unknown") } else { &first }, &st);
        set_text(
            &d.panel,
            match dbus::b(x, "PanelOn") {
                Some(true) => tr("On"),
                Some(false) => tr("Off"),
                None => tr("Unknown"),
            },
        );
        let cal = dbus::b(x, "CalibrationAvailable").unwrap_or(false);
        set_text(&d.cal, if cal { tr("Found") } else { tr("Not found") });
        d.icc_gen.set_sensitive(cal);
        let path = dbus::s(x, "IccPath").unwrap_or_default();
        d.icc.set(if path.is_empty() { tr("Not generated yet") } else { &path }, &path);
        d.icc_open.set_visible(!path.is_empty());
        let _ = &d.icc_row;
        *d.icc_path.borrow_mut() = path;
    }

    fn update_input(&self, x: &Props) {
        let i = &self.input;
        let avail = dbus::b(x, "KeyboardBacklightAvailable").unwrap_or(false);
        i.kbd.set_visible(avail);
        i.kbd_none.set_visible(!avail);
        let levels = dbus::uints(x, "KeyboardBacklightLevels");
        let labels: Vec<String> =
            levels.iter().enumerate().map(|(n, l)| if *l == 0 { tr("Off").to_string() } else { trf("Level {}", &[&n]) }).collect();
        set_items(&i.kbd, &labels);
        let cur = dbus::n(x, "KeyboardBacklight").filter(|v| *v >= 0);
        set_selected(&i.kbd, cur.and_then(|c| levels.iter().position(|l| i64::from(*l) == c)));
        *i.kbd_levels.borrow_mut() = levels;

        set_switch(&i.touch_off, dbus::b(x, "TouchOffWhenDisplayOff"));
        set_text(&i.touch_state, if dbus::b(x, "TouchInhibited") == Some(true) { tr("Blocked") } else { tr("Active") });

        let a = dbus::f(x, "IptsdActivationThreshold").unwrap_or(-1.0);
        let d = dbus::f(x, "IptsdDeactivationThreshold").unwrap_or(-1.0);
        let s = dbus::n(x, "IptsdDisableOnStylus").map(|v| v as i32).unwrap_or(-1);
        i.iptsd.set((a, d, s));
        let same = |x: f64, y: f64| (x < 0.0 && y < 0.0) || (x - y).abs() < 1e-6;
        let preset = IPTSD_PRESETS.iter().position(|(_, pa, pd)| same(a, *pa) && same(d, *pd)).unwrap_or(IPTSD_PRESETS.len());
        set_selected(&i.sens, Some(preset));
        set_selected(&i.stylus, STYLUS_MODES.iter().position(|m| m.1 == s.clamp(-1, 1)));
        let th = |v: f64| if v < 0.0 { tr("preset").to_string() } else { format!("{v}") };
        set_text(&i.thresholds, &format!("{} / {}", th(a), th(d)));
    }

    fn update_pen(&self, x: &Props) {
        let p = &self.pen;
        let avail = dbus::b(x, "Available").unwrap_or(false);
        set_text(&p.avail, if avail { tr("Connected") } else { tr("Not connected (pair it over Bluetooth)") });
        set_switch(&p.ink, dbus::b(x, "InkFeedback"));
        set_spin(&p.strength, dbus::f(x, "Strength").map(f64::round));
        set_switch(&p.eraser, dbus::b(x, "EraserClick"));
        p.test.set_sensitive(avail);
        set_switch(&p.speed, dbus::b(x, "SpeedFeedback"));
        let digitizer = dbus::b(x, "DigitizerAvailable").unwrap_or(false);
        p.speed.set_sensitive(digitizer);
        set_switch(&p.auto_couple, dbus::b(x, "AutoCouple"));
        let state = dbus::s(x, "CoupleState").unwrap_or_default();
        let result = dbus::s(x, "CoupleResult").unwrap_or_default();
        let addr = dbus::s(x, "CoupledAddress").unwrap_or_default();
        let text = match (state.as_str(), result.as_str()) {
            ("coupling", _) => tr("Pairing…").to_string(),
            ("waiting-for-pen", _) => tr("Waiting for the cover to report the pen…").to_string(),
            (_, "") => tr("None yet").to_string(),
            (_, "coupled") => trf("Paired ({})", &[&addr]),
            (_, "coupled-no-hid") => trf("Paired ({}); the pen's buttons and haptics come with its next connection", &[&addr]),
            (_, "already-coupled") => trf("Already paired ({})", &[&addr]),
            (_, "connected") => trf("Already paired ({}); the pen is connected again", &[&addr]),
            (_, "no-answer") => tr("The pen did not answer; a pairing it has was kept").to_string(),
            (_, "stale-bond") => tr("The pairing no longer works; pairing again…").to_string(),
            (_, "no-cover") => tr("No cover with a pen slot").to_string(),
            (_, "no-pen") => tr("No pen in the cover").to_string(),
            (_, "unsupported-kernel") => tr("Not supported by this kernel").to_string(),
            (_, "bluetooth-off") => tr("Bluetooth is off").to_string(),
            (_, other) => trf("Failed: {}", &[&other.strip_prefix("failed: ").unwrap_or(other)]),
        };
        set_text(&p.couple_state, &text);
        let cover = dbus::b(x, "CoverAvailable").unwrap_or(false);
        p.couple_row.set_sensitive(cover);
        p.couple.set_sensitive(cover && state == "idle");
    }

    fn update_about(&self, r: &Props, features: &[String]) {
        let a = &self.about;
        set_text(&a.device, &dbus::s(r, "Device").unwrap_or_default());
        set_text(&a.model, &dbus::s(r, "Model").filter(|m| !m.is_empty()).unwrap_or_else(|| tr("Unknown").into()));
        let version = dbus::s(r, "Version").unwrap_or_default();
        set_text(&a.version, &version);
        let f = features.join(", ");
        a.features.set(&trf("{} of {} available", &[&features.len(), &OBJECTS.len()]), &f);
        *a.debug.borrow_mut() = format!(
            "Device: {}\nModel: {}\nHelper: {version}\nApp: {}\nFeatures: {f}\n",
            dbus::s(r, "Device").unwrap_or_default(),
            dbus::s(r, "Model").unwrap_or_default(),
            env!("CARGO_PKG_VERSION"),
        );
    }
}

/// ReleaseSmartCharging's answer for a person.
fn release_text(s: &str) -> String {
    match s {
        "released" => tr("Released: this charge goes to 100 %").into(),
        "counting-down" => tr("Smart charging is about to engage; try again in a moment").into(),
        "not-engaged" => tr("Smart charging is not engaged; nothing to release").into(),
        other => trf("The firmware answered {}", &[&other]),
    }
}

/// One poll of the helper: the root object's properties and each object's.
struct Fetched {
    root: Option<Props>,
    props: HashMap<&'static str, Option<Props>>,
}

fn fetch(c: &Client, skip: &[&'static str]) -> Fetched {
    let root = c.get_all("");
    let props =
        if root.is_some() { OBJECTS.iter().filter(|o| !skip.contains(o)).map(|o| (*o, c.get_all(o))).collect() } else { HashMap::new() };
    Fetched { root, props }
}

/// Watch the helper's signals on a thread and poll on each (coalesced).
fn start_watch(ui: &Rc<Ui>, bus: Bus) {
    let (tx, rx) = async_channel::bounded::<()>(1);
    std::thread::spawn(move || loop {
        let t = tx.clone();
        dbus::watch(bus, move || {
            let _ = t.try_send(());
            !t.is_closed()
        });
        if tx.is_closed() {
            return;
        }
        std::thread::sleep(Duration::from_secs(5));
    });
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        while rx.recv().await.is_ok() {
            ui.refresh();
        }
    });
}

const USAGE: &str = "usage: sp9-settings [--help] [--version] [--page NAME] [--maximized] [--session]

  --page NAME   open on this page (performance, battery, display, input, pen,
                diagnostics, about)
  --maximized   open the window maximized
  --session     use the session bus (sp9-helperd --session --fake)

GTK4/libadwaita settings for the Surface Pro 9 (Intel). Needs the sp9-helperd
service on the system bus. The UI is Korean unless the locale is English
(SP9_SETTINGS_LANG=en|ko overrides).";

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return glib::ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a == "--version") {
        println!("sp9-settings {}", env!("CARGO_PKG_VERSION"));
        return glib::ExitCode::SUCCESS;
    }
    let page = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)).map(|n| n.to_lowercase());
    let page = match page {
        Some(n) => match PAGE_NAMES.iter().position(|p| *p == n) {
            Some(i) => Some(i),
            None => {
                eprintln!("{USAGE}");
                return glib::ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let maximized = args.iter().any(|a| a == "--maximized");
    let bus = if args.iter().any(|a| a == "--session") { Bus::Session } else { Bus::System };
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|app| {
        let css = gtk::CssProvider::new();
        css.load_from_string(CSS);
        if let Some(d) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&d, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        let quit = gio::ActionEntry::builder("quit").activate(|app: &adw::Application, _, _| app.quit()).build();
        app.add_action_entries([quit]);
        app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
        app.set_accels_for_action("window.close", &["<Ctrl>w"]);
    });
    app.connect_activate(move |app| {
        if let Some(w) = app.active_window() {
            w.present();
            return;
        }
        let ui = Ui::new(app, bus);
        ui.selected.set(page);
        if maximized {
            ui.window.maximize();
        }
        let about = gio::SimpleAction::new("about", None);
        let u = ui.clone();
        about.connect_activate(move |_, _| u.about_dialog());
        app.add_action(&about);
        let sc = gio::SimpleAction::new("shortcuts", None);
        let u = ui.clone();
        sc.connect_activate(move |_, _| u.shortcuts());
        app.add_action(&sc);
        ui.refresh_now();
        start_watch(&ui, bus);
        let ui2 = ui.clone();
        glib::timeout_add_seconds_local(POLL_SECONDS, move || {
            ui2.refresh();
            glib::ControlFlow::Continue
        });
        ui.window.present();
        let ui3 = ui.clone();
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if let Some(p) = page.and_then(|i| ui3.pages.get(i)).filter(|p| p.row.is_visible()) {
                p.row.grab_focus();
                ui3.sidebar.select_row(Some(&p.row));
                ui3.split.set_show_content(true);
            }
        });
    });
    app.run_with_args::<&str>(&[])
}
