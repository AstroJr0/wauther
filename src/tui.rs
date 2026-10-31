//! The terminal UI: a list of codes with a countdown bar each.
//!
//! Mouse: left click copies the code, right click opens a menu (copy code,
//! delete, copy key). Keyboard: see the footer.

use std::collections::BTreeSet;
use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Flex, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::core::{Error, Key, Row, Vault, now};

// Material 3 dark scheme.
const SURFACE: Color = Color::Rgb(0x14, 0x12, 0x18);
const SURFACE_LOW: Color = Color::Rgb(0x1D, 0x1B, 0x20);
const SURFACE_HIGH: Color = Color::Rgb(0x2B, 0x29, 0x30);
const SURFACE_HIGHEST: Color = Color::Rgb(0x36, 0x34, 0x3B);
const ON_SURFACE: Color = Color::Rgb(0xE6, 0xE0, 0xE9);
const ON_SURFACE_VARIANT: Color = Color::Rgb(0xCA, 0xC4, 0xD0);
const OUTLINE: Color = Color::Rgb(0x93, 0x8F, 0x99);
const OUTLINE_VARIANT: Color = Color::Rgb(0x49, 0x45, 0x4F);
const PRIMARY: Color = Color::Rgb(0xD0, 0xBC, 0xFF);
const SECONDARY_CONTAINER: Color = Color::Rgb(0x4A, 0x44, 0x58);
const ON_SECONDARY_CONTAINER: Color = Color::Rgb(0xE8, 0xDE, 0xF8);
const ERROR: Color = Color::Rgb(0xF2, 0xB8, 0xB5);
const INVERSE_SURFACE: Color = Color::Rgb(0xE6, 0xE0, 0xE9);
const INVERSE_ON_SURFACE: Color = Color::Rgb(0x32, 0x2F, 0x35);

const ROW_HEIGHT: u16 = 3;
const BAR_WIDTH: u64 = 10;
const LOW_SECONDS: u64 = 5;
const TOAST_TIME: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(250);

/// What the TUI shows.
#[derive(Debug)]
pub enum Start {
    /// The saved keys.
    Vault(Vault),
    /// One key that is never saved.
    Temp(Temp),
}

/// A key that lives only as long as the TUI does.
#[derive(Debug)]
pub struct Temp {
    pub name: String,
    key: Key,
}

impl Temp {
    #[must_use]
    pub fn new(name: Option<String>, key: Key) -> Self {
        Self {
            name: name.unwrap_or_else(|| "temp".to_owned()),
            key,
        }
    }
}

/// Runs the TUI until the user quits.
///
/// # Errors
///
/// If the terminal can't be driven.
pub fn run(start: Start) -> io::Result<()> {
    let source = match start {
        Start::Vault(vault) => Source::Vault(vault),
        Start::Temp(temp) => Source::Temp(temp),
    };
    let mut app = App::new(source, System::new());
    let mut terminal = ratatui::init();
    let result =
        execute!(io::stdout(), EnableMouseCapture).and_then(|()| app.event_loop(&mut terminal));
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

// ---------------------------------------------------------------- clipboard

trait Clipboard {
    fn set(&mut self, text: &str) -> Result<(), String>;
}

/// The system clipboard. Kept alive for the whole run: on X11 and Wayland the
/// copied text is gone once its owner is.
struct System(Option<arboard::Clipboard>);

impl System {
    fn new() -> Self {
        Self(arboard::Clipboard::new().ok())
    }
}

impl Clipboard for System {
    fn set(&mut self, text: &str) -> Result<(), String> {
        self.0
            .as_mut()
            .ok_or("no clipboard available")?
            .set_text(text)
            .map_err(|e| e.to_string())
    }
}

// -------------------------------------------------------------------- input

/// A one-line text field. `cursor` counts characters, not bytes.
#[derive(Debug, Default, Clone)]
struct Input {
    text: String,
    cursor: usize,
}

impl Input {
    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(i, _)| i)
    }

    fn edit(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.text.insert(self.byte(self.cursor), c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.byte(self.cursor));
            }
            KeyCode::Delete if self.cursor < self.len() => {
                self.text.remove(self.byte(self.cursor));
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            _ => {}
        }
    }

    /// The part of the text that fits in `width` cells, and where the cursor is in it.
    fn window(&self, width: usize) -> (String, u16) {
        let room = width.saturating_sub(1);
        let start = self.cursor.saturating_sub(room);
        let shown = self.text.chars().skip(start).take(room).collect();
        (shown, u16::try_from(self.cursor - start).unwrap_or(0))
    }
}

// ------------------------------------------------------------------- prompt

const TABS: [&str; 2] = ["Key", "URI"];
const LABELS: [[&str; 2]; 2] = [["Name", "Secret"], ["URI", "Name (optional)"]];

/// The "add" dialog: two tabs you swipe between.
#[derive(Debug, Default)]
struct Prompt {
    tab: usize,
    focus: usize,
    fields: [[Input; 2]; 2],
    error: Option<String>,
}

impl Prompt {
    fn go(&mut self, tab: usize) {
        if tab < TABS.len() && tab != self.tab {
            self.tab = tab;
            self.focus = 0;
            self.error = None;
        }
    }

    fn submit(&self, vault: &mut Vault) -> Result<String, Error> {
        let [first, second] = &self.fields[self.tab];
        if self.tab == 0 {
            vault.add(&first.text, second.text.trim())?;
            Ok(first.text.trim().to_owned())
        } else {
            let name = second.text.trim();
            vault.add_uri(first.text.trim(), (!name.is_empty()).then_some(name))
        }
    }
}

// ---------------------------------------------------------------------- app

#[derive(Debug)]
enum Source {
    Vault(Vault),
    Temp(Temp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Code,
    Delete,
    Key,
}

impl Action {
    const fn label(self) -> &'static str {
        match self {
            Self::Code => "Copy code",
            Self::Delete => "Delete",
            Self::Key => "Copy key",
        }
    }

    const fn hotkey(self) -> char {
        match self {
            Self::Code => 'c',
            Self::Delete => 'd',
            Self::Key => 'k',
        }
    }
}

#[derive(Debug, Default)]
enum Mode {
    #[default]
    List,
    Menu(Menu),
    Confirm(Vec<String>),
    Add(Box<Prompt>),
}

#[derive(Debug)]
struct Menu {
    at: Position,
    choice: usize,
}

#[derive(Debug)]
struct Toast {
    text: String,
    until: Instant,
}

/// Where things were drawn last, so clicks know what they hit.
#[derive(Debug, Default)]
struct Hits {
    rows: Vec<(Rect, usize)>,
    menu: Rect,
    items: Vec<(Rect, Action)>,
    tabs: [Rect; 2],
}

struct App<C> {
    source: Source,
    clipboard: C,
    selected: usize,
    offset: usize,
    marked: BTreeSet<String>,
    mode: Mode,
    toast: Option<Toast>,
    hits: Hits,
    quit: bool,
}

impl<C: Clipboard> App<C> {
    fn new(source: Source, clipboard: C) -> Self {
        Self {
            source,
            clipboard,
            selected: 0,
            offset: 0,
            marked: BTreeSet::new(),
            mode: Mode::List,
            toast: None,
            hits: Hits::default(),
            quit: false,
        }
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            if self
                .toast
                .as_ref()
                .is_some_and(|t| t.until <= Instant::now())
            {
                self.toast = None;
            }
            let unix = now();
            terminal.draw(|frame| self.draw(frame, unix))?;
            if event::poll(TICK)? {
                let unix = now();
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key, unix),
                    Event::Mouse(mouse) => self.mouse(mouse, unix),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // -- the source

    fn len(&self) -> usize {
        match &self.source {
            Source::Vault(vault) => vault.len(),
            Source::Temp(_) => 1,
        }
    }

    fn is_temp(&self) -> bool {
        matches!(self.source, Source::Temp(_))
    }

    fn rows(&self, unix: u64) -> Vec<Row> {
        match &self.source {
            Source::Vault(vault) => vault.rows(unix),
            Source::Temp(temp) => vec![Row {
                name: temp.name.clone(),
                code: Some(temp.key.code_at(unix)),
                remaining: temp.key.remaining_at(unix),
                period: temp.key.period(),
            }],
        }
    }

    fn lookup(&self, index: usize) -> Option<(String, Result<Key, Error>)> {
        match &self.source {
            Source::Vault(vault) => {
                let name = vault.names().nth(index)?;
                Some((name.to_owned(), vault.key(name)))
            }
            Source::Temp(temp) => (index == 0).then(|| (temp.name.clone(), Ok(temp.key.clone()))),
        }
    }

    fn actions(&self) -> &'static [Action] {
        if self.is_temp() {
            &[Action::Code, Action::Key]
        } else {
            &[Action::Code, Action::Delete, Action::Key]
        }
    }

    // -- things the user can do

    fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            until: Instant::now() + TOAST_TIME,
        });
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.len().saturating_sub(1));
    }

    fn step(&mut self, down: bool) {
        self.select(if down {
            self.selected + 1
        } else {
            self.selected.saturating_sub(1)
        });
    }

    fn copy(&mut self, action: Action, unix: u64) {
        let Some((name, key)) = self.lookup(self.selected) else {
            return;
        };
        let key = match key {
            Ok(key) => key,
            Err(e) => return self.toast(e.to_string()),
        };
        let (text, what) = match action {
            Action::Code => (key.code_at(unix), "code"),
            Action::Key => (key.to_stored(), "key"),
            Action::Delete => return,
        };
        match self.clipboard.set(&text) {
            Ok(()) => self.toast(format!("Copied {what} of {name}")),
            Err(e) => self.toast(format!("Clipboard: {e}")),
        }
    }

    fn run(&mut self, action: Action, unix: u64) {
        match action {
            Action::Delete => self.ask_delete(false),
            action => self.copy(action, unix),
        }
    }

    fn ask_delete(&mut self, marked: bool) {
        if self.is_temp() {
            return;
        }
        let names = if marked && !self.marked.is_empty() {
            self.marked.iter().cloned().collect()
        } else {
            self.lookup(self.selected)
                .map(|(name, _)| name)
                .into_iter()
                .collect()
        };
        self.mode = Mode::Confirm(names);
    }

    fn delete(&mut self, names: &[String]) {
        let Source::Vault(vault) = &mut self.source else {
            return;
        };
        match vault.remove_many(names.iter().map(String::as_str)) {
            Ok(()) => {
                self.marked.clear();
                self.select(self.selected);
                let n = names.len();
                self.toast(format!("Deleted {n} key{}", if n == 1 { "" } else { "s" }));
            }
            Err(e) => self.toast(e.to_string()),
        }
    }

    fn toggle_mark(&mut self) {
        if self.is_temp() {
            return;
        }
        if let Some((name, _)) = self.lookup(self.selected)
            && !self.marked.remove(&name)
        {
            self.marked.insert(name);
        }
        self.step(true);
    }

    // -- keyboard

    fn key(&mut self, key: KeyEvent, unix: u64) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::take(&mut self.mode) {
            Mode::List => self.key_list(key, unix),
            Mode::Menu(menu) => self.key_menu(menu, key, unix),
            Mode::Confirm(names) => self.key_confirm(names, key),
            Mode::Add(prompt) => self.key_add(prompt, key),
        }
    }

    fn key_list(&mut self, key: KeyEvent, unix: u64) {
        match key.code {
            KeyCode::Esc if !self.marked.is_empty() => self.marked.clear(),
            KeyCode::Esc | KeyCode::Char('q') => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.step(true),
            KeyCode::Up | KeyCode::Char('k') => self.step(false),
            KeyCode::Home | KeyCode::Char('g') => self.select(0),
            KeyCode::End | KeyCode::Char('G') => self.select(usize::MAX),
            KeyCode::Enter | KeyCode::Char('c') => self.copy(Action::Code, unix),
            KeyCode::Char('y') => self.copy(Action::Key, unix),
            KeyCode::Char('a') if !self.is_temp() => self.mode = Mode::Add(Box::default()),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('d') => self.ask_delete(false),
            KeyCode::Char('D') => self.ask_delete(true),
            _ => {}
        }
    }

    fn key_menu(&mut self, mut menu: Menu, key: KeyEvent, unix: u64) {
        let actions = self.actions();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {}
            KeyCode::Up => {
                menu.choice = menu.choice.saturating_sub(1);
                self.mode = Mode::Menu(menu);
            }
            KeyCode::Down => {
                menu.choice = (menu.choice + 1).min(actions.len() - 1);
                self.mode = Mode::Menu(menu);
            }
            KeyCode::Enter => self.run(actions[menu.choice], unix),
            KeyCode::Char(c) => {
                match actions
                    .iter()
                    .find(|a| a.hotkey() == c.to_ascii_lowercase())
                {
                    Some(&action) => self.run(action, unix),
                    None => self.mode = Mode::Menu(menu),
                }
            }
            _ => self.mode = Mode::Menu(menu),
        }
    }

    fn key_confirm(&mut self, names: Vec<String>, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.delete(&names),
            KeyCode::Char('n' | 'N' | 'q') | KeyCode::Esc => {}
            _ => self.mode = Mode::Confirm(names),
        }
    }

    fn key_add(&mut self, mut prompt: Box<Prompt>, key: KeyEvent) {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Right if alt => prompt.go(prompt.tab + 1),
            KeyCode::Left if alt => prompt.go(prompt.tab.saturating_sub(1)),
            KeyCode::PageDown => prompt.go(prompt.tab + 1),
            KeyCode::PageUp => prompt.go(prompt.tab.saturating_sub(1)),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                prompt.focus = 1 - prompt.focus;
            }
            KeyCode::Enter => {
                let Source::Vault(vault) = &mut self.source else {
                    return;
                };
                match prompt.submit(vault) {
                    Ok(name) => {
                        self.toast(format!("Added {name}"));
                        return;
                    }
                    Err(e) => prompt.error = Some(e.to_string()),
                }
            }
            _ => {
                prompt.error = None;
                let (tab, focus) = (prompt.tab, prompt.focus);
                prompt.fields[tab][focus].edit(key);
            }
        }
        self.mode = Mode::Add(prompt);
    }

    // -- mouse

    fn mouse(&mut self, mouse: MouseEvent, unix: u64) {
        let at = Position::new(mouse.column, mouse.row);
        match (mouse.kind, std::mem::take(&mut self.mode)) {
            (MouseEventKind::Down(button), Mode::Menu(menu)) => {
                if button == MouseButton::Left
                    && let Some(&(_, action)) = self.hits.items.iter().find(|(r, _)| r.contains(at))
                {
                    self.run(action, unix);
                } else if self.hits.menu.contains(at) {
                    self.mode = Mode::Menu(menu);
                } else {
                    self.click(button, at, unix);
                }
            }
            (MouseEventKind::Down(MouseButton::Left), Mode::Add(mut prompt)) => {
                if let Some(tab) = self.hits.tabs.iter().position(|r| r.contains(at)) {
                    prompt.go(tab);
                }
                self.mode = Mode::Add(prompt);
            }
            (MouseEventKind::Down(button), Mode::List) => self.click(button, at, unix),
            (MouseEventKind::ScrollDown, Mode::List) => self.step(true),
            (MouseEventKind::ScrollUp, Mode::List) => self.step(false),
            (_, mode) => self.mode = mode,
        }
    }

    fn click(&mut self, button: MouseButton, at: Position, unix: u64) {
        let Some(&(_, index)) = self.hits.rows.iter().find(|(r, _)| r.contains(at)) else {
            return;
        };
        self.select(index);
        match button {
            MouseButton::Left => self.copy(Action::Code, unix),
            MouseButton::Right => self.mode = Mode::Menu(Menu { at, choice: 0 }),
            MouseButton::Middle => {}
        }
    }

    // -- drawing

    fn draw(&mut self, frame: &mut Frame, unix: u64) {
        let area = frame.area();
        self.hits = Hits::default();
        frame.render_widget(Block::new().style(Style::new().bg(SURFACE)), area);
        let [title, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);
        self.draw_title(frame, title);
        self.draw_list(frame, body.inner(Margin::new(1, 0)), unix);
        let hint = match (&self.mode, self.is_temp()) {
            (Mode::List, false) => {
                "⏎ copy · y key · a add · d del · space mark · D del marked · q quit"
            }
            (Mode::List, true) => "⏎ copy · y key · q quit",
            _ => "esc cancel",
        };
        frame.render_widget(
            Paragraph::new(hint)
                .style(Style::new().fg(OUTLINE))
                .centered(),
            footer,
        );
        match &self.mode {
            Mode::List => {}
            Mode::Menu(menu) => {
                let (menu_area, items) = draw_menu(frame, menu, self.actions());
                self.hits.menu = menu_area;
                self.hits.items = items;
            }
            Mode::Confirm(names) => draw_confirm(frame, names),
            Mode::Add(prompt) => self.hits.tabs = draw_prompt(frame, prompt),
        }
        if let Some(toast) = &self.toast {
            draw_toast(frame, area, &toast.text);
        }
    }

    fn draw_title(&self, frame: &mut Frame, area: Rect) {
        let right = match &self.source {
            Source::Vault(vault) => vault.path().display().to_string(),
            Source::Temp(_) => "temp · nothing is saved".to_owned(),
        };
        let right = fit(&right, usize::from(area.width).saturating_sub(12));
        frame.render_widget(
            Paragraph::new(Span::styled(
                " wauther",
                Style::new().fg(PRIMARY).add_modifier(Modifier::BOLD),
            )),
            area,
        );
        frame.render_widget(
            Paragraph::new(format!("{right} "))
                .style(Style::new().fg(OUTLINE))
                .right_aligned(),
            area,
        );
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect, unix: u64) {
        let rows = self.rows(unix);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new("No keys yet. Press a to add one.")
                    .style(Style::new().fg(ON_SURFACE_VARIANT))
                    .centered(),
                area,
            );
            return;
        }
        let visible = usize::from((area.height / ROW_HEIGHT).max(1));
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + visible {
            self.offset = self.selected + 1 - visible;
        }
        let slots = Layout::vertical(std::iter::repeat_n(Constraint::Length(ROW_HEIGHT), visible))
            .split(area);
        for (slot, (index, row)) in slots.iter().zip(rows.iter().enumerate().skip(self.offset)) {
            let marked = self.marked.contains(&row.name);
            draw_row(frame, *slot, row, index == self.selected, marked);
            self.hits.rows.push((*slot, index));
        }
    }
}

fn draw_row(frame: &mut Frame, area: Rect, row: &Row, selected: bool, marked: bool) {
    let (border, bg) = if selected {
        (PRIMARY, SURFACE_HIGH)
    } else {
        (OUTLINE_VARIANT, SURFACE_LOW)
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .style(Style::new().bg(bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let low = row.code.is_some() && row.remaining <= LOW_SECONDS;
    let accent = if low { ERROR } else { PRIMARY };
    let code = row
        .code
        .as_deref()
        .map_or_else(|| "invalid".to_owned(), spaced);
    let tail = format!(
        "{code}  {} {:>2}s",
        bar(row.remaining, row.period),
        row.remaining
    );
    let marker = if marked { "● " } else { "  " };
    let room = usize::from(inner.width).saturating_sub(tail.chars().count() + marker.len() + 1);
    let name = fit(&row.name, room);
    let pad = usize::from(inner.width)
        .saturating_sub(marker.chars().count() + name.chars().count() + tail.chars().count());
    let code_style = if row.code.is_some() {
        Style::new().fg(accent).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(ERROR)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(marker, Style::new().fg(PRIMARY)),
            Span::styled(name, Style::new().fg(ON_SURFACE)),
            Span::raw(" ".repeat(pad)),
            Span::styled(tail, code_style),
        ])),
        inner,
    );
}

fn draw_menu(frame: &mut Frame, menu: &Menu, actions: &[Action]) -> (Rect, Vec<(Rect, Action)>) {
    let screen = frame.area();
    let (w, h) = (20, u16::try_from(actions.len()).unwrap_or(3) + 2);
    let x = menu.at.x.min(screen.right().saturating_sub(w));
    let y = menu.at.y.min(screen.bottom().saturating_sub(h));
    let area = Rect::new(x, y, w, h).intersection(screen);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(OUTLINE))
        .style(Style::new().bg(SURFACE_HIGHEST));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut items = Vec::new();
    for (i, (line, &action)) in inner.rows().zip(actions).enumerate() {
        let style = match (i == menu.choice, action) {
            (true, _) => Style::new()
                .bg(SECONDARY_CONTAINER)
                .fg(ON_SECONDARY_CONTAINER),
            (false, Action::Delete) => Style::new().fg(ERROR),
            (false, _) => Style::new().fg(ON_SURFACE),
        };
        let text = format!(
            " {}  {}",
            action.hotkey().to_ascii_uppercase(),
            action.label()
        );
        frame.render_widget(Paragraph::new(text).style(style), line);
        items.push((line, action));
    }
    (area, items)
}

fn draw_confirm(frame: &mut Frame, names: &[String]) {
    const SHOWN: usize = 5;
    let mut lines = vec![
        Line::styled(
            format!(
                "Delete {} key{}?",
                names.len(),
                if names.len() == 1 { "" } else { "s" }
            ),
            Style::new().fg(ON_SURFACE).add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
    ];
    lines.extend(names.iter().take(SHOWN).map(|n| {
        Line::styled(
            format!("  {}", fit(n, 36)),
            Style::new().fg(ON_SURFACE_VARIANT),
        )
    }));
    if names.len() > SHOWN {
        lines.push(Line::styled(
            format!("  …and {} more", names.len() - SHOWN),
            Style::new().fg(OUTLINE),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("y delete · n cancel", Style::new().fg(ERROR)));
    let h = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let area = centered(frame.area(), 44, h);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ERROR))
                .style(Style::new().bg(SURFACE_HIGHEST))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

/// Draws the add dialog and returns where its two tab titles are.
fn draw_prompt(frame: &mut Frame, prompt: &Prompt) -> [Rect; 2] {
    let area = centered(frame.area(), 56, 12);
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(PRIMARY))
        .title(" Add key ")
        .style(Style::new().bg(SURFACE_HIGHEST));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [tabs, _, first, second, error, hint] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let [a, b, _] = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Length(7),
        Constraint::Min(0),
    ])
    .areas(tabs);
    let tab_rects = [a, b];
    for (i, (rect, title)) in tab_rects.iter().zip(TABS).enumerate() {
        let style = if i == prompt.tab {
            Style::new()
                .fg(PRIMARY)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::new().fg(ON_SURFACE_VARIANT)
        };
        frame.render_widget(Paragraph::new(title).style(style).centered(), *rect);
    }

    for (i, rect) in [first, second].into_iter().enumerate() {
        let focused = i == prompt.focus;
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(if focused { PRIMARY } else { OUTLINE }))
            .title(LABELS[prompt.tab][i]);
        let field = block.inner(rect);
        frame.render_widget(block, rect);
        let input = &prompt.fields[prompt.tab][i];
        let (shown, cursor) = input.window(usize::from(field.width));
        frame.render_widget(
            Paragraph::new(shown).style(Style::new().fg(ON_SURFACE)),
            field,
        );
        if focused {
            frame.set_cursor_position(Position::new(field.x + cursor, field.y));
        }
    }

    if let Some(message) = &prompt.error {
        frame.render_widget(
            Paragraph::new(fit(message, usize::from(error.width))).style(Style::new().fg(ERROR)),
            error,
        );
    }
    frame.render_widget(
        Paragraph::new("⏎ add · tab field · alt+←→ swipe")
            .style(Style::new().fg(OUTLINE))
            .centered(),
        hint,
    );
    tab_rects
}

fn draw_toast(frame: &mut Frame, screen: Rect, text: &str) {
    let width = u16::try_from(text.chars().count() + 4).unwrap_or(u16::MAX);
    let area = centered(screen, width, 3);
    let area = Rect {
        y: screen.bottom().saturating_sub(5),
        ..area
    }
    .intersection(screen);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::new().fg(INVERSE_ON_SURFACE).bg(INVERSE_SURFACE))
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(INVERSE_SURFACE))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
        area,
    );
}

// ------------------------------------------------------------------ helpers

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    area
}

/// "123456" → "123 456"
fn spaced(code: &str) -> String {
    let (a, b) = code.split_at(code.len() / 2);
    format!("{a} {b}")
}

/// A countdown bar that empties as the code runs out.
fn bar(remaining: u64, period: u64) -> String {
    let filled = (remaining * BAR_WIDTH)
        .div_ceil(period.max(1))
        .min(BAR_WIDTH);
    let filled = usize::try_from(filled).unwrap_or(0);
    let empty = usize::try_from(BAR_WIDTH).unwrap_or(0) - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// `text` cut to `max` characters, with an ellipsis if anything was lost.
fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyEventState;

    use super::*;

    /// "12345678901234567890" in base32. At t = 59 the code is 287082 (RFC 6238).
    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[derive(Default)]
    struct Fake {
        copied: Vec<String>,
        broken: bool,
    }

    impl Clipboard for Fake {
        fn set(&mut self, text: &str) -> Result<(), String> {
            if self.broken {
                return Err("nope".to_owned());
            }
            self.copied.push(text.to_owned());
            Ok(())
        }
    }

    fn vault_app(names: &[&str]) -> (App<Fake>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path().join("keys.json")).unwrap();
        for name in names {
            vault.add(name, SECRET).unwrap();
        }
        (App::new(Source::Vault(vault), Fake::default()), dir)
    }

    fn temp_app() -> App<Fake> {
        let temp = Temp::new(None, Key::parse(SECRET).unwrap());
        App::new(Source::Temp(temp), Fake::default())
    }

    fn press(app: &mut App<Fake>, code: KeyCode) {
        app.key(
            KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            },
            59,
        );
    }

    fn type_text(app: &mut App<Fake>, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn click(app: &mut App<Fake>, button: MouseButton, x: u16, y: u16) {
        app.mouse(
            MouseEvent {
                kind: MouseEventKind::Down(button),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            },
            59,
        );
    }

    fn draw(app: &mut App<Fake>, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.draw(frame, 59)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn toast(app: &App<Fake>) -> &str {
        app.toast.as_ref().map_or("", |t| t.text.as_str())
    }

    #[test]
    fn enter_copies_the_code() {
        let (mut app, _dir) = vault_app(&["a"]);

        press(&mut app, KeyCode::Enter);

        assert_eq!(app.clipboard.copied, ["287082"]);
        assert_eq!(toast(&app), "Copied code of a");
    }

    #[test]
    fn y_copies_the_key() {
        let (mut app, _dir) = vault_app(&["a"]);

        press(&mut app, KeyCode::Char('y'));

        assert_eq!(app.clipboard.copied, [SECRET]);
    }

    #[test]
    fn a_broken_clipboard_is_reported() {
        let (mut app, _dir) = vault_app(&["a"]);
        app.clipboard.broken = true;

        press(&mut app, KeyCode::Enter);

        assert_eq!(toast(&app), "Clipboard: nope");
    }

    #[test]
    fn selection_stays_inside_the_list() {
        let (mut app, _dir) = vault_app(&["a", "b", "c"]);

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selected, 0);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected, 2);
        press(&mut app, KeyCode::Home);
        assert_eq!(app.selected, 0);
        press(&mut app, KeyCode::End);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn q_quits() {
        let (mut app, _dir) = vault_app(&["a"]);

        press(&mut app, KeyCode::Char('q'));

        assert!(app.quit);
    }

    #[test]
    fn esc_clears_marks_before_it_quits() {
        let (mut app, _dir) = vault_app(&["a", "b"]);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Esc);
        assert!(!app.quit);
        assert!(app.marked.is_empty());

        press(&mut app, KeyCode::Esc);
        assert!(app.quit);
    }

    #[test]
    fn left_click_copies_the_code_of_the_row() {
        let (mut app, _dir) = vault_app(&["a", "b"]);
        draw(&mut app, 60, 20);
        let (rect, _) = app.hits.rows[1];

        click(&mut app, MouseButton::Left, rect.x + 2, rect.y + 1);

        assert_eq!(app.selected, 1);
        assert_eq!(app.clipboard.copied, ["287082"]);
    }

    #[test]
    fn right_click_opens_a_menu_whose_items_work() {
        let (mut app, _dir) = vault_app(&["a"]);
        draw(&mut app, 60, 20);
        let (rect, _) = app.hits.rows[0];

        click(&mut app, MouseButton::Right, rect.x + 2, rect.y + 1);
        assert!(matches!(app.mode, Mode::Menu(_)));
        draw(&mut app, 60, 20);
        let (item, _) = *app
            .hits
            .items
            .iter()
            .find(|(_, a)| *a == Action::Key)
            .unwrap();
        click(&mut app, MouseButton::Left, item.x, item.y);

        assert!(matches!(app.mode, Mode::List));
        assert_eq!(app.clipboard.copied, [SECRET]);
    }

    #[test]
    fn menu_hotkeys_and_arrows_work() {
        let (mut app, _dir) = vault_app(&["a"]);
        draw(&mut app, 60, 20);
        let (rect, _) = app.hits.rows[0];
        click(&mut app, MouseButton::Right, rect.x + 1, rect.y + 1);

        press(&mut app, KeyCode::Char('c'));

        assert_eq!(app.clipboard.copied, ["287082"]);

        click(&mut app, MouseButton::Right, rect.x + 1, rect.y + 1);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);

        assert!(matches!(app.mode, Mode::Confirm(_)));
    }

    #[test]
    fn a_click_outside_the_menu_closes_it() {
        let (mut app, _dir) = vault_app(&["a"]);
        draw(&mut app, 60, 20);
        let (rect, _) = app.hits.rows[0];
        click(&mut app, MouseButton::Right, rect.x + 1, rect.y + 1);
        draw(&mut app, 60, 20);

        click(&mut app, MouseButton::Left, 0, 19);

        assert!(matches!(app.mode, Mode::List));
        assert!(app.clipboard.copied.is_empty());
    }

    #[test]
    fn delete_asks_first() {
        let (mut app, _dir) = vault_app(&["a", "b"]);

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('n'));
        assert_eq!(app.len(), 2);

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.len(), 1);
        assert_eq!(toast(&app), "Deleted 1 key");
    }

    #[test]
    fn delete_is_saved() {
        let (mut app, dir) = vault_app(&["a", "b"]);

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Enter);

        let vault = Vault::open(dir.path().join("keys.json")).unwrap();
        assert_eq!(vault.names().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn marked_keys_are_deleted_together() {
        let (mut app, _dir) = vault_app(&["a", "b", "c"]);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));

        press(&mut app, KeyCode::Char('D'));
        let Mode::Confirm(names) = &app.mode else {
            panic!("no confirmation");
        };
        assert_eq!(names, &["a", "c"]);
        press(&mut app, KeyCode::Char('y'));

        let Source::Vault(vault) = &app.source else {
            unreachable!()
        };
        assert_eq!(vault.names().collect::<Vec<_>>(), ["b"]);
        assert!(app.marked.is_empty());
    }

    #[test]
    fn deleting_the_last_row_moves_the_selection_up() {
        let (mut app, _dir) = vault_app(&["a", "b"]);
        press(&mut app, KeyCode::End);

        press(&mut app, KeyCode::Char('d'));
        press(&mut app, KeyCode::Char('y'));

        assert_eq!(app.selected, 0);
    }

    #[test]
    fn the_prompt_adds_a_key() {
        let (mut app, _dir) = vault_app(&[]);

        press(&mut app, KeyCode::Char('a'));
        type_text(&mut app, "github");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, SECRET);
        press(&mut app, KeyCode::Enter);

        assert!(matches!(app.mode, Mode::List));
        let Source::Vault(vault) = &app.source else {
            unreachable!()
        };
        assert!(vault.contains("github"));
        assert_eq!(toast(&app), "Added github");
    }

    #[test]
    fn the_prompt_keeps_open_on_an_error() {
        let (mut app, _dir) = vault_app(&["a"]);

        press(&mut app, KeyCode::Char('a'));
        type_text(&mut app, "a");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, SECRET);
        press(&mut app, KeyCode::Enter);

        let Mode::Add(prompt) = &app.mode else {
            panic!("prompt closed");
        };
        assert!(prompt.error.is_some());
        assert_eq!(app.len(), 1);
    }

    #[test]
    fn the_prompt_swipes_to_the_uri_tab() {
        let (mut app, _dir) = vault_app(&[]);
        press(&mut app, KeyCode::Char('a'));

        app.key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT), 59);
        type_text(&mut app, &format!("otpauth://totp/Mail:me?secret={SECRET}"));
        press(&mut app, KeyCode::Enter);

        let Source::Vault(vault) = &app.source else {
            unreachable!()
        };
        assert!(vault.contains("Mail:me"));
    }

    #[test]
    fn clicking_a_tab_swipes() {
        let (mut app, _dir) = vault_app(&[]);
        press(&mut app, KeyCode::Char('a'));
        draw(&mut app, 80, 24);
        let uri = app.hits.tabs[1];

        click(&mut app, MouseButton::Left, uri.x + 1, uri.y);

        let Mode::Add(prompt) = &app.mode else {
            panic!("prompt closed");
        };
        assert_eq!(prompt.tab, 1);
    }

    #[test]
    fn esc_closes_the_prompt() {
        let (mut app, _dir) = vault_app(&[]);
        press(&mut app, KeyCode::Char('a'));

        press(&mut app, KeyCode::Esc);

        assert!(matches!(app.mode, Mode::List));
        assert!(!app.quit);
    }

    #[test]
    fn temp_shows_one_key_and_cannot_add_or_delete() {
        let mut app = temp_app();

        press(&mut app, KeyCode::Char('a'));
        assert!(matches!(app.mode, Mode::List));
        press(&mut app, KeyCode::Char('d'));
        assert!(matches!(app.mode, Mode::List));
        press(&mut app, KeyCode::Char('D'));
        assert!(matches!(app.mode, Mode::List));
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.len(), 1);
        assert_eq!(app.clipboard.copied, ["287082"]);
        assert_eq!(app.actions(), [Action::Code, Action::Key]);
    }

    #[test]
    fn temp_is_named_temp_unless_told_otherwise() {
        let key = Key::parse(SECRET).unwrap();

        assert_eq!(Temp::new(None, key.clone()).name, "temp");
        assert_eq!(Temp::new(Some("x".to_owned()), key).name, "x");
    }

    #[test]
    fn the_screen_shows_names_and_codes() {
        let (mut app, _dir) = vault_app(&["github"]);

        let screen = draw(&mut app, 60, 12);

        assert!(screen.contains("github"));
        assert!(screen.contains("287 082"));
    }

    #[test]
    fn an_empty_vault_says_so() {
        let (mut app, _dir) = vault_app(&[]);

        assert!(draw(&mut app, 60, 12).contains("No keys yet"));
    }

    #[test]
    fn drawing_survives_tiny_and_huge_screens() {
        let (mut app, _dir) = vault_app(&["a", "b", "c", "d", "e", "f"]);
        press(&mut app, KeyCode::End);

        for (w, h) in [(0, 0), (1, 1), (10, 3), (20, 6), (200, 80)] {
            draw(&mut app, w, h);
            press(&mut app, KeyCode::Char('a'));
            draw(&mut app, w, h);
            press(&mut app, KeyCode::Esc);
            press(&mut app, KeyCode::Char('d'));
            draw(&mut app, w, h);
            press(&mut app, KeyCode::Esc);
        }
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_visible() {
        let (mut app, _dir) = vault_app(&["a", "b", "c", "d", "e"]);
        press(&mut app, KeyCode::End);

        let screen = draw(&mut app, 60, 10);

        assert!(app.offset > 0);
        assert!(screen.contains('e'));
        assert!(app.hits.rows.iter().any(|&(_, i)| i == 4));
    }

    #[test]
    fn input_edits_in_the_middle() {
        let mut input = Input::default();
        for c in "héllo".chars() {
            input.edit(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }

        for code in [KeyCode::Left, KeyCode::Left, KeyCode::Backspace] {
            input.edit(KeyEvent::new(code, KeyModifiers::NONE));
        }
        assert_eq!(input.text, "hélo");
        input.edit(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(input.text, "héo");
        input.edit(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        input.edit(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(input.text, "xhéo");
        input.edit(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(input.cursor, 4);
    }

    #[test]
    fn input_window_follows_the_cursor() {
        let mut input = Input::default();
        input.text = "abcdefghij".to_owned();
        input.cursor = 10;

        assert_eq!(input.window(5), ("ghij".to_owned(), 4));

        input.cursor = 2;
        assert_eq!(input.window(5), ("abcd".to_owned(), 2));
    }

    #[test]
    fn the_bar_empties_with_time() {
        assert_eq!(bar(30, 30), "██████████");
        assert_eq!(bar(15, 30), "█████░░░░░");
        assert_eq!(bar(1, 30), "█░░░░░░░░░");
        assert_eq!(bar(0, 30), "░░░░░░░░░░");
        assert_eq!(bar(5, 0), "██████████");
    }

    #[test]
    fn codes_are_split_in_the_middle() {
        assert_eq!(spaced("123456"), "123 456");
        assert_eq!(spaced("12345678"), "1234 5678");
    }

    #[test]
    fn fit_cuts_with_an_ellipsis() {
        assert_eq!(fit("abc", 3), "abc");
        assert_eq!(fit("abcd", 3), "ab…");
        assert_eq!(fit("abcd", 0), "…");
    }
}
