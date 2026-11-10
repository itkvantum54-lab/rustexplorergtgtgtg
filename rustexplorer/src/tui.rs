//! Интерактивный TUI-проводник (аналог окна explorer.exe) на crossterm.
//! Управление: стрелки/ Vim hjkl, Enter — открыть, Backspace — вверх,
//! Del — корзина, Shift+Del — удалить навсегда, c/m — копировать/переместить
//! выделенное в текущий каталог, / — фильтр, n — новый файл/папка, q — выход.

use std::io;
use std::path::{Path, PathBuf};

use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    style::{Attribute, Print, SetAttribute, SetForegroundColor, Color},
    terminal::{self, Clear},
};

use crate::fsinfo::{human_size, read_dir_sorted, Entry, SortMode};
use crate::ops;

pub struct Explorer {
    cwd: PathBuf,
    entries: Vec<Entry>,
    cursor: usize,
    offset: usize, // первый видимый элемент (скроллинг)
    sort: SortMode,
    show_hidden: bool,
    filter: String,
    status: String,
    clipboard: Option<(PathBuf, ClipOp)>,
}

enum ClipOp {
    Copy,
    Move,
}

impl Explorer {
    pub fn set_show_hidden(&mut self, v: bool) {
        self.show_hidden = v;
        self.refresh();
    }

    pub fn new(cwd: PathBuf) -> Self {
        let mut e = Self {
            cwd,
            entries: Vec::new(),
            cursor: 0,
            offset: 0,
            sort: SortMode::Name,
            show_hidden: false,
            filter: String::new(),
            status: String::from("Стрелки/hjkl — навигация, Enter — открыть, q — выход"),
            clipboard: None,
        };
        e.refresh();
        e
    }

    fn refresh(&mut self) {
        match read_dir_sorted(&self.cwd, self.sort, self.show_hidden) {
            Ok(list) => self.entries = list,
            Err(err) => self.status = format!("Ошибка чтения {}: {err}", self.cwd.display()),
        }
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
    }

    fn filtered_idx(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| f.is_empty() || e.name.to_lowercase().contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    fn move_cursor(&mut self, delta: isize) {
        let idx = self.filtered_idx();
        if idx.is_empty() {
            return;
        }
        let pos = idx.iter().position(|&i| i == self.cursor).unwrap_or(0);
        let newpos = (pos as isize + delta)
            .clamp(0, idx.len() as isize - 1) as usize;
        self.cursor = idx[newpos];
    }

    fn open_selected(&mut self) {
        let Some(entry) = self.entries.get(self.cursor) else { return };
        if entry.is_dir {
            self.cwd = entry.path.clone();
            self.cursor = 0;
            self.offset = 0;
            self.refresh();
        } else {
            self.status = format!(
                "Файл: {} ({})",
                entry.path.display(),
                human_size(entry.size)
            );
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.cwd.parent() {
            self.cwd = parent.to_path_buf();
            self.cursor = 0;
            self.refresh();
        }
    }

    fn trash_selected(&mut self, permanent: bool) {
        let Some(entry) = self.entries.get(self.cursor).cloned() else { return };
        let res = if permanent {
            ops::remove_recursive(&entry.path).map(|_| format!("Удалено: {}", entry.name))
        } else {
            ops::to_trash(&entry.path)
                .map(|p| format!("В корзину: {}", p.file_name().unwrap().to_string_lossy()))
        };
        self.status = match res {
            Ok(msg) => msg,
            Err(err) => format!("Ошибка: {err}"),
        };
        self.refresh();
    }

    fn copy_clip_from_selection(&mut self, op: ClipOp) {
        if let Some(entry) = self.entries.get(self.cursor) {
            let label = match op {
                ClipOp::Copy => "Копирование",
                ClipOp::Move => "Перемещение",
            };
            self.clipboard = Some((entry.path.clone(), op));
            self.status = format!("{label}: {} — нажмите Enter в папке-приёмнике", entry.name);
        }
    }

    fn paste_clipboard(&mut self) {
        let Some((src, op)) = self.clipboard.take() else { return };
        let res = match op {
            ClipOp::Copy => ops::copy_tree(&src, &self.cwd.join(src.file_name().unwrap()))
                .map(|n| format!("Скопировано {n} байт в {}", self.cwd.display())),
            ClipOp::Move => {
                let dst = self.cwd.join(src.file_name().unwrap());
                if src.is_dir() {
                    ops::copy_tree(&src, &dst).and_then(|n| {
                        ops::remove_recursive(&src)?;
                        Ok(n)
                    }).map(|n| format!("Перемещено {n} байт в {}", self.cwd.display()))
                } else {
                    ops::move_path(&src, &self.cwd).map(|_| format!("Перемещено в {}", self.cwd.display()))
                }
            }
        };
        self.status = match res {
            Ok(msg) => msg,
            Err(err) => format!("Ошибка вставки: {err}"),
        };
        self.refresh();
    }

    fn create_new(&mut self, as_dir: bool) {
        let base = self.cwd.join(if as_dir { "Новая папка" } else { "новый.txt" });
        match ops::create_node(&base, as_dir) {
            Ok(p) => {
                self.status = format!("Создано: {}", p.display());
                self.refresh();
            }
            Err(err) => self.status = format!("Ошибка создания: {err}"),
        }
    }

    fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            SortMode::Name => SortMode::Size,
            SortMode::Size => SortMode::Date,
            SortMode::Date => SortMode::Name,
        };
        self.refresh();
        self.status = format!("Сортировка: {:?}", self.sort);
    }

    fn draw(&mut self, term_w: u16, term_h: u16) -> io::Result<()> {
        let mut out = io::stdout();
        execute!(out, Clear(terminal::ClearType::All), MoveTo(0, 0))?;

        // Шапка: путь + сортировка + скрытые
        let title = format!(
            " rustexplorer — {} | sort:{:?} hidden:{} | {}",
            shorten(&self.cwd, 40),
            self.sort,
            if self.show_hidden { "on" } else { "off" },
            if self.filter.is_empty() {
                String::new()
            } else {
                format!("filter:\"{}\"", self.filter)
            }
        );
        execute!(
            out,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Rgb { r: 255, g: 200, b: 80 }),
            Print(truncate(&title, term_w)),
            SetAttribute(Attribute::Reset),
            SetForegroundColor(Color::Reset),
            MoveTo(0, 1),
            Print("-".repeat(term_w as usize)),
        )?;

        // Список
        let list_h = term_h.saturating_sub(3) as usize; // шапка+разделитель+строка состояния
        let idx = self.filtered_idx();
        // держим курсор в видимой области
        if let Some(pos) = idx.iter().position(|&i| i == self.cursor) {
            if pos < self.offset {
                self.offset = pos;
            } else if pos >= self.offset + list_h {
                self.offset = pos + 1 - list_h;
            }
        }
        for (row, &ei) in idx[self.offset..]
            .iter()
            .take(list_h)
            .enumerate()
        {
            let e = &self.entries[ei];
            let selected = ei == self.cursor;
            let kind = if e.is_dir { "/" } else { " " };
            let date = e
                .modified
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "—".into());
            let line = format!(
                "{:>2} {}{:<width$} {:>9}  {}",
                "",
                kind,
                truncate(
                    &e.name,
                    (term_w as usize).saturating_sub(32).min(u16::MAX as usize) as u16
                ),
                human_size(e.size),
                date,
                width = (term_w as usize).saturating_sub(32).max(10),
            );
            execute!(out, MoveTo(0, 2 + row as u16))?;
            if selected {
                execute!(
                    out,
                    SetAttribute(Attribute::Reverse),
                    Print(truncate(&line, term_w)),
                    SetAttribute(Attribute::Reset)
                )?;
            } else {
                if e.is_dir {
                    execute!(out, SetForegroundColor(Color::Rgb { r: 96, g: 174, b: 255 }))?;
                }
                execute!(out, Print(truncate(&line, term_w)))?;
                if e.is_dir {
                    execute!(out, SetForegroundColor(Color::Reset))?;
                }
            }
        }

        // Строка состояния
        execute!(
            out,
            MoveTo(0, term_h.saturating_sub(1)),
            SetAttribute(Attribute::Bold),
            Print(truncate(
                &format!(" {} | el:{} | q-выход", self.status, idx.len()),
                term_w
            )),
            SetAttribute(Attribute::Reset),
        )?;
        Ok(())
    }

    /// Главный цикл: ввод -> обработка -> отрисовка.
    pub fn run(mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        execute!(out, crossterm::terminal::EnterAlternateScreen)?;
        let result = self.loop_();
        execute!(out, crossterm::terminal::LeaveAlternateScreen)?;
        terminal::disable_raw_mode()?;
        result
    }

    fn loop_(&mut self) -> io::Result<()> {
        loop {
            let (w, h) = terminal::size()?;
            self.draw(w, h)?;
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press && k.kind != KeyEventKind::Repeat {
                    continue;
                }
                match k.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => {
                        self.filter.clear();
                    }
                    KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
                    KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
                    KeyCode::Enter => {
                        if self.clipboard.is_some() {
                            self.paste_clipboard();
                        } else {
                            self.open_selected();
                        }
                    }
                    KeyCode::Backspace => self.go_up(),
                    KeyCode::Delete => self.trash_selected(k.modifiers.contains(KeyModifiers::SHIFT)),
                    KeyCode::Char('h') => {
                        self.show_hidden = !self.show_hidden;
                        self.refresh();
                    }
                    KeyCode::Char('s') => self.cycle_sort(),
                    KeyCode::Char('c') => self.copy_clip_from_selection(ClipOp::Copy),
                    KeyCode::Char('m') => self.copy_clip_from_selection(ClipOp::Move),
                    KeyCode::Char('n') => self.create_new(false),
                    KeyCode::Char('N') => self.create_new(true),
                    KeyCode::Char('/') => {
                        self.filter.clear();
                        self.status = "Фильтр (буквы, Esc — сброс)…".into();
                        loop {
                            if let Event::Key(k2) = event::read()? {
                                match k2.code {
                                    KeyCode::Enter => break,
                                    KeyCode::Esc => {
                                        self.filter.clear();
                                        break;
                                    }
                                    KeyCode::Backspace => {
                                        self.filter.pop();
                                    }
                                    KeyCode::Char(ch) => self.filter.push(ch),
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn truncate(s: &str, w: u16) -> String {
    let w = w as usize;
    if s.chars().count() <= w {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(w.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn shorten(p: &Path, max: usize) -> String {
    let s = p.display().to_string();
    if s.len() <= max {
        s
    } else {
        format!("…{}", &s[s.len() - max + 1..])
    }
}
