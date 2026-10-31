//! The desktop window, drawn with egui.
//!
//! Material 3 dark scheme from a blue seed. Everything is painted by hand so
//! the concave corners (see [`crate::concave`]) can join the containers up.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Key, Mesh, Painter, Pos2, Rect,
    ScrollArea, Sense, TextEdit, Ui, UiBuilder, Vec2, pos2, vec2,
};

use crate::concave::{Concave, Toward};
use crate::core::{Error, Row, Vault, now};

// Material 3 dark scheme, blue seed (#0061A4 in Material Theme Builder).
// Written from memory of that scheme, not generated: check against the tool.
const SURFACE: Color32 = Color32::from_rgb(0x10, 0x14, 0x18);
const SURFACE_CONTAINER: Color32 = Color32::from_rgb(0x1D, 0x20, 0x24);
const SURFACE_CONTAINER_HIGH: Color32 = Color32::from_rgb(0x27, 0x2A, 0x2F);
const SURFACE_CONTAINER_HIGHEST: Color32 = Color32::from_rgb(0x32, 0x35, 0x3A);
const ON_SURFACE: Color32 = Color32::from_rgb(0xE0, 0xE2, 0xE8);
const ON_SURFACE_VARIANT: Color32 = Color32::from_rgb(0xC3, 0xC7, 0xCF);
const OUTLINE: Color32 = Color32::from_rgb(0x8D, 0x91, 0x99);
const OUTLINE_VARIANT: Color32 = Color32::from_rgb(0x43, 0x47, 0x4E);
const PRIMARY: Color32 = Color32::from_rgb(0x9F, 0xCA, 0xFF);
const ON_PRIMARY: Color32 = Color32::from_rgb(0x00, 0x32, 0x58);
const PRIMARY_CONTAINER: Color32 = Color32::from_rgb(0x00, 0x49, 0x7D);
const ON_PRIMARY_CONTAINER: Color32 = Color32::from_rgb(0xD1, 0xE4, 0xFF);
const SECONDARY_CONTAINER: Color32 = Color32::from_rgb(0x3B, 0x48, 0x58);
const ON_SECONDARY_CONTAINER: Color32 = Color32::from_rgb(0xD7, 0xE3, 0xF8);
const TERTIARY_CONTAINER: Color32 = Color32::from_rgb(0x52, 0x3F, 0x5F);
const ERROR: Color32 = Color32::from_rgb(0xFF, 0xB4, 0xAB);
const ON_ERROR: Color32 = Color32::from_rgb(0x69, 0x00, 0x05);
const ERROR_CONTAINER: Color32 = Color32::from_rgb(0x93, 0x00, 0x0A);
const ON_ERROR_CONTAINER: Color32 = Color32::from_rgb(0xFF, 0xDA, 0xD6);
const INVERSE_SURFACE: Color32 = Color32::from_rgb(0xE0, 0xE2, 0xE8);
const INVERSE_ON_SURFACE: Color32 = Color32::from_rgb(0x2E, 0x31, 0x35);

const MARGIN: f32 = 16.0;
const ROW_HEIGHT: f32 = 72.0;
const ROW_GAP: f32 = 2.0;
const OUTER_RADIUS: u8 = 28;
const INNER_RADIUS: u8 = 6;
const DELETE_WIDTH: f32 = 68.0;
const FAB: f32 = 88.0;
const CONCAVE_RADIUS: f32 = 16.0;
const PIE_RADIUS: f32 = 14.0;
const LOW_SECONDS: u64 = 5;
const HOVER_SECS: f32 = 0.18;
const PROMPT_SECS: f32 = 0.3;
const PROMPT_MIN_HEIGHT: f32 = 320.0;
const BLINK_PERIOD: f64 = 2.4;
const ARM_SECS: f64 = 3.0;
const TOAST_SECS: f64 = 2.0;

/// Opens the window and runs until it is closed.
///
/// # Errors
///
/// If the window can't be created.
pub fn run(vault: Vault) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wauther")
            .with_inner_size([440.0, 700.0])
            .with_min_inner_size([340.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "wauther",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(visuals());
            Ok(Box::new(Wauther::new(vault)))
        }),
    )
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(ON_SURFACE);
    visuals.selection.bg_fill = PRIMARY_CONTAINER;
    visuals
}

// -------------------------------------------------------------------- state

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Manual,
    Uri,
}

/// What the "add" prompt holds.
#[derive(Debug, Default)]
struct Form {
    tab: Tab,
    name: String,
    key: String,
    uri: String,
    uri_name: String,
    error: Option<String>,
}

impl Form {
    /// Adds what the form describes. On success the form is emptied.
    fn submit(&mut self, vault: &mut Vault) -> Result<String, Error> {
        let added = match self.tab {
            Tab::Manual => vault
                .add(&self.name, self.key.trim())
                .map(|()| self.name.trim().to_owned()),
            Tab::Uri => {
                let name = self.uri_name.trim();
                vault.add_uri(self.uri.trim(), (!name.is_empty()).then_some(name))
            }
        };
        match &added {
            Ok(_) => {
                *self = Self {
                    tab: self.tab,
                    ..Self::default()
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        added
    }
}

struct Wauther {
    vault: Vault,
    form: Form,
    prompt_open: bool,
    /// 0 closed, 1 open.
    prompt_t: f32,
    focus_first: bool,
    hover: HashMap<String, f32>,
    fab_hover: f32,
    dots_hover: f32,
    /// A row whose delete button was pressed once, and when.
    armed: Option<(String, f64)>,
    toast: Option<(String, f64)>,
}

impl Wauther {
    fn new(vault: Vault) -> Self {
        Self {
            vault,
            form: Form::default(),
            prompt_open: false,
            prompt_t: 0.0,
            focus_first: false,
            hover: HashMap::new(),
            fab_hover: 0.0,
            dots_hover: 0.0,
            armed: None,
            toast: None,
        }
    }

    fn open_prompt(&mut self) {
        self.prompt_open = true;
        self.focus_first = true;
        self.form.error = None;
    }

    fn say(&mut self, text: impl Into<String>, time: f64) {
        self.toast = Some((text.into(), time + TOAST_SECS));
    }
}

#[derive(Clone, Copy)]
struct Tick {
    dt: f32,
    time: f64,
    pointer: Option<Pos2>,
    /// False while the prompt covers the list.
    enabled: bool,
    /// Seconds into the current one, so the pie can run smoothly.
    subsec: f32,
}

enum RowEvent {
    Copy(String),
    Arm,
    Delete,
}

impl eframe::App for Wauther {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let full = ui.max_rect();
        let tick = Tick {
            dt: ui.input(|i| i.stable_dt).min(0.1),
            time: ui.input(|i| i.time),
            pointer: ui.input(|i| i.pointer.hover_pos()),
            enabled: !self.prompt_open && self.prompt_t < 0.01,
            subsec: subsec(),
        };
        let mut animating = false;

        if self.prompt_open && ui.input(|i| i.key_pressed(Key::Escape)) {
            self.prompt_open = false;
        }
        animating |= approach(
            &mut self.prompt_t,
            if self.prompt_open { 1.0 } else { 0.0 },
            tick.dt,
            PROMPT_SECS,
        );
        if self
            .armed
            .as_ref()
            .is_some_and(|(_, at)| tick.time - at > ARM_SECS)
        {
            self.armed = None;
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, until)| tick.time > *until)
        {
            self.toast = None;
        }

        ui.painter().rect_filled(full, CornerRadius::ZERO, SURFACE);
        let dots_y = self.draw_header(ui, full, tick, &mut animating);
        let table = Rect::from_min_max(
            pos2(full.left() + MARGIN, dots_y + 33.0),
            pos2(full.right() - MARGIN, full.bottom()),
        );
        self.draw_table(ui, table, tick, &mut animating);
        self.draw_fab(ui, full, tick, &mut animating);
        self.draw_prompt(ui, full, tick);
        self.draw_toast(ui, full, tick.time);

        ui.ctx().request_repaint_after(Duration::from_millis(
            if animating || self.toast.is_some() {
                16
            } else {
                60
            },
        ));
    }
}

// ------------------------------------------------------------------- header

impl Wauther {
    /// Draws the wordmark and the three dots. Returns the dots' y.
    fn draw_header(&mut self, ui: &mut Ui, full: Rect, tick: Tick, animating: &mut bool) -> f32 {
        let painter = ui.painter().clone();
        let font = FontId::proportional(34.0);
        let rest = painter.layout_no_wrap("auther".to_owned(), font.clone(), ON_SURFACE);
        let badge_size = 48.0;
        let gap = 6.0;
        let total = badge_size + gap + rest.size().x;
        let left = full.center().x - total / 2.0;
        let badge = Rect::from_min_size(pos2(left, full.top() + 28.0), Vec2::splat(badge_size));

        painter.rect_filled(badge, CornerRadius::same(14), TERTIARY_CONTAINER);
        bold(
            &painter,
            badge.center(),
            Align2::CENTER_CENTER,
            "W",
            FontId::proportional(32.0),
            Color32::WHITE,
        );
        // Tungsten's atomic number, like a cell of the periodic table.
        painter.text(
            badge.left_bottom() + vec2(6.0, -5.0),
            Align2::LEFT_BOTTOM,
            "74",
            FontId::proportional(10.0),
            Color32::WHITE.gamma_multiply(0.7),
        );
        bold(
            &painter,
            pos2(badge.right() + gap, badge.center().y),
            Align2::LEFT_CENTER,
            "auther",
            font,
            ON_SURFACE,
        );

        let y = badge.bottom() + 30.0;
        let spacing = 26.0;
        let area =
            Rect::from_center_size(pos2(full.center().x, y), vec2(spacing * 2.0 + 40.0, 40.0));
        let over = tick.enabled && tick.pointer.is_some_and(|p| area.contains(p));
        *animating |= approach(
            &mut self.dots_hover,
            if over { 1.0 } else { 0.0 },
            tick.dt,
            0.4,
        );
        for (i, step) in [-1.0_f32, 0.0, 1.0].into_iter().enumerate() {
            let lit = self.dots_hover * blink(tick.time, u8::try_from(i).unwrap_or(0));
            let color = OUTLINE_VARIANT.lerp_to_gamma(PRIMARY, lit);
            painter.circle_filled(pos2(full.center().x + step * spacing, y), 7.0, color);
        }
        if self.dots_hover > 0.0 {
            *animating = true;
        }
        y
    }

    fn draw_toast(&self, ui: &Ui, full: Rect, time: f64) {
        let Some((text, until)) = &self.toast else {
            return;
        };
        let alpha = ((until - time) / 0.3).clamp(0.0, 1.0) as f32;
        let painter = ui.painter();
        let galley =
            painter.layout_no_wrap(text.clone(), FontId::proportional(14.0), INVERSE_ON_SURFACE);
        let size = galley.size() + vec2(32.0, 24.0);
        let rect = Rect::from_min_size(
            pos2(full.left() + MARGIN, full.bottom() - 24.0 - size.y),
            size,
        );
        painter.rect_filled(
            rect,
            CornerRadius::same(12),
            INVERSE_SURFACE.gamma_multiply(alpha),
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(14.0),
            INVERSE_ON_SURFACE.gamma_multiply(alpha),
        );
    }
}

// -------------------------------------------------------------------- table

impl Wauther {
    fn draw_table(&mut self, ui: &mut Ui, area: Rect, tick: Tick, animating: &mut bool) {
        let rows = self.vault.rows(now());
        if rows.is_empty() {
            let painter = ui.painter();
            painter.text(
                area.center_top() + vec2(0.0, 40.0),
                Align2::CENTER_TOP,
                "No keys yet",
                FontId::proportional(18.0),
                ON_SURFACE_VARIANT,
            );
            painter.text(
                area.center_top() + vec2(0.0, 70.0),
                Align2::CENTER_TOP,
                "Press + to add one",
                FontId::proportional(14.0),
                OUTLINE,
            );
            return;
        }

        let mut events = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let count = rows.len();
                let step = ROW_HEIGHT + ROW_GAP;
                let height = step * count as f32 - ROW_GAP + FAB + 24.0;
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
                for (i, row) in rows.iter().enumerate() {
                    let top = rect.top() + step * i as f32;
                    let row_rect =
                        Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), ROW_HEIGHT));
                    let hover = self.hover.entry(row.name.clone()).or_insert(0.0);
                    let armed = self
                        .armed
                        .as_ref()
                        .is_some_and(|(name, _)| *name == row.name);
                    let event =
                        draw_row(ui, row_rect, row, row_radius(i, count), hover, armed, tick);
                    *animating |= *hover > 0.0 && *hover < 1.0;
                    if let Some(event) = event {
                        events.push((row.name.clone(), event));
                    }
                }
            });
        });

        for (name, event) in events {
            match event {
                RowEvent::Copy(code) => {
                    ui.ctx().copy_text(code);
                    self.say(format!("Copied code of {name}"), tick.time);
                }
                RowEvent::Arm => self.armed = Some((name, tick.time)),
                RowEvent::Delete => {
                    self.armed = None;
                    match self.vault.remove(&name) {
                        Ok(()) => self.say(format!("Deleted {name}"), tick.time),
                        Err(e) => self.say(e.to_string(), tick.time),
                    }
                }
            }
        }
        if let Some((name, _)) = &self.armed
            && self.hover.get(name).is_none_or(|t| *t < 0.01)
        {
            self.armed = None;
        }
    }
}

/// Outermost corners are the roundest, inner ones are tight.
fn row_radius(index: usize, count: usize) -> CornerRadius {
    let top = if index == 0 {
        OUTER_RADIUS
    } else {
        INNER_RADIUS
    };
    let bottom = if index + 1 == count {
        OUTER_RADIUS
    } else {
        INNER_RADIUS
    };
    CornerRadius {
        nw: top,
        ne: top,
        sw: bottom,
        se: bottom,
    }
}

fn draw_row(
    ui: &mut Ui,
    rect: Rect,
    row: &Row,
    corners: CornerRadius,
    hover: &mut f32,
    armed: bool,
    tick: Tick,
) -> Option<RowEvent> {
    let painter = ui.painter().clone();
    let over = tick.enabled && tick.pointer.is_some_and(|p| rect.contains(p));
    approach(hover, if over { 1.0 } else { 0.0 }, tick.dt, HOVER_SECS);
    let t = ease(*hover);
    painter.rect_filled(
        rect,
        corners,
        SURFACE_CONTAINER.lerp_to_gamma(SURFACE_CONTAINER_HIGH, t),
    );

    let id = Id::new(("row", &row.name));
    let body = ui.interact(
        rect,
        id,
        if tick.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let mut event = None;

    // The delete button slides in from the right, joined to the row by two concave corners.
    let width = DELETE_WIDTH * t;
    let button = Rect::from_min_max(pos2(rect.right() - width, rect.top()), rect.max);
    let on_button = tick.pointer.is_some_and(|p| button.contains(p)) && width > 1.0;
    if width > 1.0 {
        let (fill, ink) = if armed {
            (ERROR, ON_ERROR)
        } else {
            (ERROR_CONTAINER, ON_ERROR_CONTAINER)
        };
        painter.rect_filled(
            button,
            CornerRadius {
                nw: 0,
                sw: 0,
                ..corners
            },
            fill,
        );
        let radius = CONCAVE_RADIUS * t;
        Concave::new(button.left_top(), Toward::SouthWest, radius, fill).paint(&painter);
        Concave::new(button.left_bottom(), Toward::NorthWest, radius, fill).paint(&painter);
        if armed {
            draw_check(&painter, button.center(), ink);
        } else {
            draw_cross(&painter, button.center(), ink);
        }
        if tick.enabled && t > 0.6 {
            let delete = ui.interact(button, id.with("delete"), Sense::click());
            if delete.clicked() {
                event = Some(if armed {
                    RowEvent::Delete
                } else {
                    RowEvent::Arm
                });
            }
        }
    }
    if over {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    // The code, big and bold in the middle.
    let low = row.remaining <= LOW_SECONDS;
    let code_font = FontId::monospace(30.0);
    let center = rect.center();
    let half = match row.code.as_deref().map(spaced) {
        Some(code) => {
            let color = if low { ERROR } else { PRIMARY };
            let half = painter
                .layout_no_wrap(code.clone(), code_font.clone(), color)
                .size()
                .x
                / 2.0;
            bold(
                &painter,
                center,
                Align2::CENTER_CENTER,
                &code,
                code_font,
                color,
            );
            half
        }
        None => {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                "invalid key",
                FontId::proportional(18.0),
                ERROR,
            );
            60.0
        }
    };

    // The name, on the left, cut short before it reaches the code.
    let name_font = FontId::proportional(17.0);
    let left = rect.left() + 24.0;
    let room = (center.x - half - 12.0 - left).max(0.0);
    let name = fit_width(&row.name, room, |s| {
        painter
            .layout_no_wrap(s.to_owned(), name_font.clone(), ON_SURFACE)
            .size()
            .x
    });
    painter.text(
        pos2(left, center.y),
        Align2::LEFT_CENTER,
        name,
        name_font,
        ON_SURFACE,
    );

    // The pie: how much of the period is left.
    let pie = pos2(rect.right() - 36.0 - width, center.y);
    painter.circle_filled(pie, PIE_RADIUS, SECONDARY_CONTAINER);
    let fraction = pie_fraction(row.remaining, row.period, tick.subsec);
    painter.add(egui::Shape::mesh(pie_mesh(
        pie,
        PIE_RADIUS,
        fraction,
        if low { ERROR } else { PRIMARY },
    )));

    if event.is_none()
        && body.clicked()
        && !on_button
        && let Some(code) = &row.code
    {
        event = Some(RowEvent::Copy(code.clone()));
    }
    event
}

fn draw_cross(painter: &Painter, at: Pos2, color: Color32) {
    let d = 7.0;
    let stroke = egui::Stroke::new(2.5, color);
    painter.line_segment([at + vec2(-d, -d), at + vec2(d, d)], stroke);
    painter.line_segment([at + vec2(-d, d), at + vec2(d, -d)], stroke);
}

fn draw_check(painter: &Painter, at: Pos2, color: Color32) {
    let stroke = egui::Stroke::new(3.0, color);
    painter.line_segment([at + vec2(-9.0, 1.0), at + vec2(-3.0, 7.0)], stroke);
    painter.line_segment([at + vec2(-3.0, 7.0), at + vec2(9.0, -7.0)], stroke);
}

// ---------------------------------------------------------------------- fab

impl Wauther {
    /// The big "+" in the bottom-right corner, joined to the window edges by
    /// two concave corners.
    fn draw_fab(&mut self, ui: &mut Ui, full: Rect, tick: Tick, animating: &mut bool) {
        if self.prompt_t > 0.01 {
            return;
        }
        let painter = ui.painter().clone();
        let rect = Rect::from_min_size(full.right_bottom() - Vec2::splat(FAB), Vec2::splat(FAB));
        let over = tick.enabled && tick.pointer.is_some_and(|p| rect.contains(p));
        *animating |= approach(
            &mut self.fab_hover,
            if over { 1.0 } else { 0.0 },
            tick.dt,
            HOVER_SECS,
        );
        let fill =
            PRIMARY_CONTAINER.lerp_to_gamma(ON_PRIMARY_CONTAINER, 0.12 * ease(self.fab_hover));

        painter.rect_filled(
            rect,
            CornerRadius {
                nw: OUTER_RADIUS,
                ne: 0,
                sw: 0,
                se: 0,
            },
            fill,
        );
        Concave::new(rect.right_top(), Toward::NorthWest, CONCAVE_RADIUS, fill).paint(&painter);
        Concave::new(rect.left_bottom(), Toward::NorthWest, CONCAVE_RADIUS, fill).paint(&painter);

        let c = rect.center();
        let (long, thick) = (30.0, 6.0);
        let bar = CornerRadius::same(3);
        painter.rect_filled(
            Rect::from_center_size(c, vec2(long, thick)),
            bar,
            ON_PRIMARY_CONTAINER,
        );
        painter.rect_filled(
            Rect::from_center_size(c, vec2(thick, long)),
            bar,
            ON_PRIMARY_CONTAINER,
        );

        let response = ui.interact(
            rect,
            Id::new("fab"),
            if tick.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if over {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.open_prompt();
        }
    }
}

// ------------------------------------------------------------------- prompt

impl Wauther {
    fn draw_prompt(&mut self, ui: &mut Ui, full: Rect, tick: Tick) {
        if self.prompt_t <= 0.0 {
            return;
        }
        let p = ease(self.prompt_t);
        let painter = ui.painter().clone();
        let height = (full.height() * 0.5)
            .max(PROMPT_MIN_HEIGHT)
            .min(full.height());
        let rect = Rect::from_min_size(
            pos2(full.left(), full.top() - height * (1.0 - p)),
            vec2(full.width(), height),
        );

        painter.rect_filled(
            full,
            CornerRadius::ZERO,
            Color32::BLACK.gamma_multiply(0.5 * p),
        );
        if ui
            .interact(full, Id::new("scrim"), Sense::click())
            .clicked()
        {
            self.prompt_open = false;
        }
        ui.interact(rect, Id::new("prompt"), Sense::click());

        painter.rect_filled(rect, CornerRadius::ZERO, SURFACE_CONTAINER_HIGH);
        let corner = f32::from(OUTER_RADIUS);
        Concave::new(
            rect.left_bottom(),
            Toward::SouthEast,
            corner,
            SURFACE_CONTAINER_HIGH,
        )
        .paint(&painter);
        Concave::new(
            rect.right_bottom(),
            Toward::SouthWest,
            corner,
            SURFACE_CONTAINER_HIGH,
        )
        .paint(&painter);

        let inner = Rect::from_min_max(
            pos2(rect.left() + 24.0, rect.top() + 24.0),
            pos2(rect.right() - 24.0, rect.bottom() - 20.0),
        );
        self.draw_tabs(
            ui,
            Rect::from_min_size(inner.min, vec2(inner.width(), 40.0)),
        );

        let first = Rect::from_min_size(
            pos2(inner.left(), inner.top() + 56.0),
            vec2(inner.width(), 56.0),
        );
        let second = first.translate(vec2(0.0, 68.0));
        let first_id = Id::new(("field", self.form.tab == Tab::Uri, 0));
        let second_id = Id::new(("field", self.form.tab == Tab::Uri, 1));
        match self.form.tab {
            Tab::Manual => {
                field(
                    ui,
                    first,
                    first_id,
                    "Key name",
                    "e.g. GitHub",
                    &mut self.form.name,
                );
                field(
                    ui,
                    second,
                    second_id,
                    "Key",
                    "Base32 secret",
                    &mut self.form.key,
                );
            }
            Tab::Uri => {
                field(
                    ui,
                    first,
                    first_id,
                    "URI",
                    "otpauth://totp/…",
                    &mut self.form.uri,
                );
                field(
                    ui,
                    second,
                    second_id,
                    "Key name",
                    "Taken from the URI if empty",
                    &mut self.form.uri_name,
                );
            }
        }
        if self.focus_first && p > 0.9 {
            ui.memory_mut(|m| m.request_focus(first_id));
            self.focus_first = false;
        }

        if let Some(error) = &self.form.error {
            painter.text(
                pos2(inner.left() + 4.0, second.bottom() + 8.0),
                Align2::LEFT_TOP,
                error,
                FontId::proportional(13.0),
                ERROR,
            );
        }

        let add_rect = Rect::from_min_size(
            pos2(inner.right() - 96.0, inner.bottom() - 40.0),
            vec2(96.0, 40.0),
        );
        let cancel_rect = Rect::from_min_size(
            pos2(add_rect.left() - 104.0, add_rect.top()),
            vec2(96.0, 40.0),
        );
        let focused = ui.memory(|m| m.has_focus(first_id) || m.has_focus(second_id));
        let enter = focused && ui.input(|i| i.key_pressed(Key::Enter));
        if button(
            ui,
            cancel_rect,
            Id::new("cancel"),
            "Cancel",
            false,
            tick.pointer,
        ) {
            self.prompt_open = false;
        }
        if button(ui, add_rect, Id::new("add"), "Add", true, tick.pointer) || enter {
            if let Ok(name) = self.form.submit(&mut self.vault) {
                self.prompt_open = false;
                self.say(format!("Added {name}"), tick.time);
            }
        }
    }

    /// The two options: add by hand, or from an `otpauth://` URI.
    fn draw_tabs(&mut self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter().clone();
        let half = rect.width() / 2.0;
        for (i, (tab, label)) in [(Tab::Manual, "Add manually"), (Tab::Uri, "From URI")]
            .into_iter()
            .enumerate()
        {
            let x = rect.left() + half * i as f32;
            let segment = Rect::from_min_size(pos2(x, rect.top()), vec2(half, rect.height()));
            let corners = if i == 0 {
                CornerRadius {
                    nw: 20,
                    sw: 20,
                    ne: 0,
                    se: 0,
                }
            } else {
                CornerRadius {
                    nw: 0,
                    sw: 0,
                    ne: 20,
                    se: 20,
                }
            };
            let selected = self.form.tab == tab;
            if selected {
                painter.rect_filled(segment, corners, SECONDARY_CONTAINER);
            }
            painter.text(
                segment.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(15.0),
                if selected {
                    ON_SECONDARY_CONTAINER
                } else {
                    ON_SURFACE_VARIANT
                },
            );
            if ui
                .interact(segment, Id::new(("tab", i)), Sense::click())
                .clicked()
                && !selected
            {
                self.form.tab = tab;
                self.form.error = None;
                self.focus_first = true;
            }
        }
        // The outline goes on top of the selected fill, as a thin frame of bars.
        let line = OUTLINE;
        painter.rect_filled(
            Rect::from_min_size(
                rect.left_top() + vec2(20.0, 0.0),
                vec2(rect.width() - 40.0, 1.0),
            ),
            CornerRadius::ZERO,
            line,
        );
        painter.rect_filled(
            Rect::from_min_size(
                rect.left_bottom() + vec2(20.0, -1.0),
                vec2(rect.width() - 40.0, 1.0),
            ),
            CornerRadius::ZERO,
            line,
        );
        painter.rect_filled(
            Rect::from_min_size(rect.center_top() - vec2(0.5, 0.0), vec2(1.0, rect.height())),
            CornerRadius::ZERO,
            line,
        );
    }
}

/// A filled text field: label on top, text below, an indicator along the bottom.
fn field(ui: &mut Ui, rect: Rect, id: Id, label: &str, hint: &str, text: &mut String) {
    let focused = ui.memory(|m| m.has_focus(id));
    let painter = ui.painter().clone();
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: 12,
            ne: 12,
            sw: 4,
            se: 4,
        },
        SURFACE_CONTAINER_HIGHEST,
    );
    let (indicator, height) = if focused {
        (PRIMARY, 2.0)
    } else {
        (ON_SURFACE_VARIANT, 1.0)
    };
    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max),
        CornerRadius::ZERO,
        indicator,
    );
    painter.text(
        rect.left_top() + vec2(16.0, 8.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(12.0),
        if focused { PRIMARY } else { ON_SURFACE_VARIANT },
    );
    let edit = Rect::from_min_max(
        rect.left_top() + vec2(16.0, 24.0),
        rect.right_bottom() - vec2(16.0, 4.0),
    );
    ui.put(
        edit,
        TextEdit::singleline(text)
            .id(id)
            .frame(false)
            .hint_text(hint)
            .font(FontId::proportional(16.0))
            .text_color(ON_SURFACE)
            .desired_width(f32::INFINITY),
    );
}

/// A pill button. Returns whether it was clicked.
fn button(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    label: &str,
    filled: bool,
    pointer: Option<Pos2>,
) -> bool {
    let over = pointer.is_some_and(|p| rect.contains(p));
    let painter = ui.painter();
    let (ink, fill) = if filled {
        (ON_PRIMARY, Some(PRIMARY))
    } else {
        (PRIMARY, None)
    };
    let state = if over { 0.12 } else { 0.0 };
    if let Some(fill) = fill {
        painter.rect_filled(
            rect,
            CornerRadius::same(20),
            fill.lerp_to_gamma(ON_PRIMARY, state),
        );
    } else if over {
        painter.rect_filled(rect, CornerRadius::same(20), PRIMARY.gamma_multiply(0.12));
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(15.0),
        ink,
    );
    let response = ui.interact(rect, id, Sense::click());
    if over {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.clicked()
}

// ------------------------------------------------------------------ helpers

/// egui ships no bold face. Drawing twice, a hair apart, thickens the strokes.
fn bold(painter: &Painter, at: Pos2, anchor: Align2, text: &str, font: FontId, color: Color32) {
    for dx in [-0.45, 0.45] {
        painter.text(at + vec2(dx, 0.0), anchor, text, font.clone(), color);
    }
}

/// Moves `value` toward `target`, taking `seconds` to cross 0..=1. True while it still moves.
fn approach(value: &mut f32, target: f32, dt: f32, seconds: f32) -> bool {
    let step = dt / seconds;
    *value = if *value < target {
        (*value + step).min(target)
    } else {
        (*value - step).max(target)
    };
    (*value - target).abs() > f32::EPSILON
}

/// Smoothstep.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How bright dot `index` is at `time`, 0..=1. The dots take turns, a fifth of a cycle apart.
fn blink(time: f64, index: u8) -> f32 {
    let phase = (time / BLINK_PERIOD - f64::from(index) * 0.2).rem_euclid(1.0) as f32;
    let wave = 0.5 - 0.5 * (TAU * phase).cos();
    wave * wave
}

fn subsec() -> f32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.subsec_millis() as f32 / 1000.0)
}

/// "123456" → "123 456"
fn spaced(code: &str) -> String {
    let (a, b) = code.split_at(code.len() / 2);
    format!("{a} {b}")
}

/// What is left of the period, 0..=1. `subsec` is how far into the current second we are.
fn pie_fraction(remaining: u64, period: u64, subsec: f32) -> f32 {
    ((remaining as f32 - subsec) / period.max(1) as f32).clamp(0.0, 1.0)
}

/// A slice of a disc, starting at 12 o'clock and running clockwise over `fraction` of the circle.
fn pie_mesh(center: Pos2, radius: f32, fraction: f32, color: Color32) -> Mesh {
    const FULL_TURN_STEPS: f32 = 64.0;
    let mut mesh = Mesh::default();
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 || radius <= 0.0 {
        return mesh;
    }
    let steps = (fraction * FULL_TURN_STEPS).ceil().max(1.0) as u32;
    mesh.colored_vertex(center, color);
    for k in 0..=steps {
        let angle = -FRAC_PI_2 + TAU * fraction * k as f32 / steps as f32;
        mesh.colored_vertex(center + radius * vec2(angle.cos(), angle.sin()), color);
    }
    for k in 0..steps {
        mesh.add_triangle(0, k + 1, k + 2);
    }
    mesh
}

/// `text` shortened, with an ellipsis, until `measure` says it fits in `max` wide.
fn fit_width(text: &str, max: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max {
        return text.to_owned();
    }
    let mut kept: Vec<char> = text.chars().collect();
    while kept.pop().is_some() {
        let candidate = format!("{}…", kept.iter().collect::<String>());
        if measure(&candidate) <= max {
            return candidate;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn vault() -> (Vault, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Vault::open(dir.path().join("keys.json")).unwrap(), dir)
    }

    fn area(mesh: &Mesh) -> f32 {
        mesh.indices
            .chunks_exact(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].pos);
                ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn only_the_outermost_corners_are_round() {
        let first = row_radius(0, 3);
        let middle = row_radius(1, 3);
        let last = row_radius(2, 3);

        assert_eq!((first.nw, first.ne, first.sw, first.se), (28, 28, 6, 6));
        assert_eq!((middle.nw, middle.ne, middle.sw, middle.se), (6, 6, 6, 6));
        assert_eq!((last.nw, last.ne, last.sw, last.se), (6, 6, 28, 28));
    }

    #[test]
    fn a_lone_row_is_round_all_over() {
        let only = row_radius(0, 1);

        assert_eq!((only.nw, only.ne, only.sw, only.se), (28, 28, 28, 28));
    }

    #[test]
    fn approach_stops_at_the_target() {
        let mut v = 0.0;

        assert!(approach(&mut v, 1.0, 0.1, 0.2));
        assert!((v - 0.5).abs() < 1e-6);
        assert!(approach(&mut v, 1.0, 0.1, 0.2) || (v - 1.0).abs() < 1e-6);
        assert!(!approach(&mut v, 1.0, 0.1, 0.2));
        assert_eq!(v, 1.0);
        assert!(!approach(&mut v, 1.0, 0.1, 0.2));
    }

    #[test]
    fn approach_goes_down_too() {
        let mut v = 1.0;

        approach(&mut v, 0.0, 0.5, 0.2);

        assert_eq!(v, 0.0);
    }

    #[test]
    fn ease_is_smooth_and_bounded() {
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(0.5), 0.5);
        assert_eq!(ease(1.0), 1.0);
        assert_eq!(ease(2.0), 1.0);
    }

    #[test]
    fn the_dots_take_turns() {
        let t = BLINK_PERIOD * 0.5;

        let lit: Vec<f32> = (0..3).map(|i| blink(t, i)).collect();

        assert!(lit[0] > 0.99);
        assert!(lit[0] > lit[1] && lit[1] > lit[2]);
        assert!(blink(0.0, 0) < 0.01);
    }

    #[test]
    fn blink_stays_in_range() {
        for i in 0..3 {
            for step in 0..200 {
                let v = blink(f64::from(step) * 0.05, i);
                assert!((0.0..=1.0).contains(&v));
            }
        }
    }

    #[test]
    fn codes_are_split_in_the_middle() {
        assert_eq!(spaced("123456"), "123 456");
        assert_eq!(spaced("12345678"), "1234 5678");
    }

    #[test]
    fn the_pie_empties_over_the_period() {
        assert_eq!(pie_fraction(30, 30, 0.0), 1.0);
        assert_eq!(pie_fraction(15, 30, 0.0), 0.5);
        assert!((pie_fraction(15, 30, 0.5) - (14.5 / 30.0)).abs() < 1e-6);
        assert_eq!(pie_fraction(0, 30, 0.9), 0.0);
        assert_eq!(pie_fraction(5, 0, 0.0), 1.0);
    }

    #[test]
    fn the_pie_mesh_covers_its_fraction_of_the_disc() {
        let r = 10.0_f32;

        let half = area(&pie_mesh(pos2(0.0, 0.0), r, 0.5, Color32::WHITE));
        let full = area(&pie_mesh(pos2(0.0, 0.0), r, 1.0, Color32::WHITE));

        let disc = std::f32::consts::PI * r * r;
        assert!((full - disc).abs() / disc < 0.01);
        assert!((half - disc / 2.0).abs() / disc < 0.01);
    }

    #[test]
    fn the_pie_starts_at_twelve_and_goes_clockwise() {
        let mesh = pie_mesh(pos2(0.0, 0.0), 10.0, 0.25, Color32::WHITE);

        let (first, last) = (mesh.vertices[1].pos, mesh.vertices.last().unwrap().pos);

        assert!(first.x.abs() < 1e-4 && (first.y + 10.0).abs() < 1e-4);
        assert!((last.x - 10.0).abs() < 1e-4 && last.y.abs() < 1e-4);
    }

    #[test]
    fn an_empty_pie_has_no_mesh() {
        assert!(pie_mesh(pos2(0.0, 0.0), 10.0, 0.0, Color32::WHITE).is_empty());
        assert!(pie_mesh(pos2(0.0, 0.0), 0.0, 0.5, Color32::WHITE).is_empty());
    }

    #[test]
    fn fit_width_shortens_with_an_ellipsis() {
        let width = |s: &str| s.chars().count() as f32;

        assert_eq!(fit_width("github", 6.0, width), "github");
        assert_eq!(fit_width("github", 4.0, width), "git…");
        assert_eq!(fit_width("github", 0.5, width), "");
    }

    #[test]
    fn the_form_adds_by_hand() {
        let (mut vault, _dir) = vault();
        let mut form = Form {
            name: " GitHub ".to_owned(),
            key: format!(" {SECRET} "),
            ..Form::default()
        };

        let name = form.submit(&mut vault).unwrap();

        assert_eq!(name, "GitHub");
        assert!(vault.contains("GitHub"));
        assert!(form.name.is_empty() && form.key.is_empty());
    }

    #[test]
    fn the_form_adds_from_a_uri_and_names_it_after_the_uri() {
        let (mut vault, _dir) = vault();
        let mut form = Form {
            tab: Tab::Uri,
            uri: format!("otpauth://totp/Mail:me?secret={SECRET}"),
            ..Form::default()
        };

        let name = form.submit(&mut vault).unwrap();

        assert_eq!(name, "Mail:me");
        assert_eq!(form.tab, Tab::Uri);
    }

    #[test]
    fn a_given_name_beats_the_uri() {
        let (mut vault, _dir) = vault();
        let mut form = Form {
            tab: Tab::Uri,
            uri: format!("otpauth://totp/Mail:me?secret={SECRET}"),
            uri_name: "mine".to_owned(),
            ..Form::default()
        };

        assert_eq!(form.submit(&mut vault).unwrap(), "mine");
    }

    #[test]
    fn a_failed_submit_keeps_the_input_and_says_why() {
        let (mut vault, _dir) = vault();
        vault.add("a", SECRET).unwrap();
        let mut form = Form {
            name: "a".to_owned(),
            key: SECRET.to_owned(),
            ..Form::default()
        };

        assert!(form.submit(&mut vault).is_err());

        assert_eq!(form.name, "a");
        assert!(form.error.is_some());
        assert_eq!(vault.len(), 1);
    }

    #[test]
    fn an_empty_name_is_refused() {
        let (mut vault, _dir) = vault();
        let mut form = Form {
            key: SECRET.to_owned(),
            ..Form::default()
        };

        assert!(form.submit(&mut vault).is_err());
        assert!(vault.is_empty());
    }
}
