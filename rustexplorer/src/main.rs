//! rustexplorer — лёгкая кроссплатформенная альтернатива explorer.exe на Rust.
//!
//! Возможности:
//! * `rustexplorer browse [PATH]`   — интерактивный TUI-проводник (аналог окна Проводника)
//! * `rustexplorer ls PATH`         — список каталога (как панель файлов)
//! * `rustexplorer tree PATH`       — рекурсивный вывод дерева
//! * `rustexplorer du PATH`         — размер дерева (аналог «Свойства папки»)
//! * `rustexplorer cp SRC DST_DIR`  — копирование файла/дерева
//! * `rustexplorer mv SRC DST_DIR`  — перемещение
//! * `rustexplorer rm PATH...`      — удаление (-p — без корзины)
//! * `rustexplorer trash PATH...`   — в корзину
//! * `rustexplorer mkdir PATH...` / `touch PATH...` — создание
//! * `rustexplorer quick`           — «Быстрый доступ» (спец-папки пользователя)

mod fsinfo;
mod ops;
mod tui;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use fsinfo::{human_size, quick_access, read_dir_sorted, SortMode};

#[derive(Parser)]
#[command(name = "rustexplorer", version, about = "Лёгкий проводник файловой системы на Rust (аналог explorer.exe)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Интерактивный TUI-проводник
    Browse {
        /// Начальный каталог (по умолчанию — домашняя папка)
        path: Option<PathBuf>,
        /// Сразу показывать скрытые файлы
        #[arg(short = 'A', long)]
        hidden: bool,
    },
    /// Список содержимого каталога
    Ls {
        path: PathBuf,
        #[arg(short, long, value_enum, default_value_t = SortMode::Name)]
        sort: SortMode,
        /// Показывать скрытые файлы
        #[arg(short = 'A', long)]
        all: bool,
    },
    /// Рекурсивное дерево каталога
    Tree {
        path: PathBuf,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// Размер дерева (байты + количество файлов)
    Du { path: PathBuf },
    /// Копировать файл или дерево в каталог
    Cp { src: PathBuf, dst: PathBuf },
    /// Переместить файл/дерево в каталог
    Mv { src: PathBuf, dst: PathBuf },
    /// Удалить (в корзину по умолчанию)
    Rm {
        paths: Vec<PathBuf>,
        /// Удалить навсегда, минуя корзину
        #[arg(short, long)]
        perm: bool,
    },
    /// Отправить в корзину
    Trash { paths: Vec<PathBuf> },
    /// Создать каталоги
    Mkdir { paths: Vec<PathBuf> },
    /// Создать пустые файлы
    Touch { paths: Vec<PathBuf> },
    /// Показать спец-папки («Быстрый доступ»)
    Quick,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("rustexplorer: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow_lite::Result<()> {
    use anyhow_lite::Ctx;
    match cli.cmd {
        Cmd::Browse { path, hidden } => {
            let cwd = match path {
                Some(p) => p,
                None => dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            };
            let mut app = tui::Explorer::new(cwd);
            if hidden {
                app.set_show_hidden(true);
            }
            app.run().ctx("не удалось запустить TUI")?;
        }
        Cmd::Ls { path, sort, all } => {
            let entries = read_dir_sorted(&path, sort, all).ctx("чтение каталога")?;
            for e in &entries {
                let date = e
                    .modified
                    .map(|t| {
                        chrono::DateTime::<chrono::Local>::from(t)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "—".into());
                println!(
                    "{} {:<40} {:>10}  {}",
                    if e.is_dir { "d" } else { "-" },
                    e.name,
                    human_size(e.size),
                    date
                );
            }
            println!("итого: {} элементов", entries.len());
        }
        Cmd::Tree { path, depth } => {
            print_tree(&path, 0, depth);
        }
        Cmd::Du { path } => {
            let (bytes, files) = fsinfo::tree_size(&path);
            println!(
                "{}: {} байт ({}) в {} файлах",
                path.display(),
                bytes,
                human_size(bytes),
                files
            );
        }
        Cmd::Cp { src, dst } => {
            let n = ops::copy_tree(&src, &dst).ctx("копирование")?;
            println!("Скопировано {} байт в {}", n, dst.display());
        }
        Cmd::Mv { src, dst } => {
            ops::move_path(&src, &dst).ctx("перемещение")?;
            println!("Перемещено {} -> {}", src.display(), dst.display());
        }
        Cmd::Rm { paths, perm } => {
            for p in paths {
                let res = if perm {
                    ops::remove_recursive(&p).ctx("удаление")
                } else {
                    ops::to_trash(&p).map(|_| ()).ctx("в корзину")
                };
                res?;
                println!("Удалено: {}", p.display());
            }
        }
        Cmd::Trash { paths } => {
            for p in paths {
                let t = ops::to_trash(&p).ctx("корзина")?;
                println!("В корзину: {} -> {}", p.display(), t.display());
            }
        }
        Cmd::Mkdir { paths } => {
            for p in paths {
                std::fs::create_dir_all(&p).ctx("mkdir")?;
                println!("Создан каталог: {}", p.display());
            }
        }
        Cmd::Touch { paths } => {
            for p in paths {
                ops::create_node(&p, false).ctx("touch")?;
                println!("Создан файл: {}", p.display());
            }
        }
        Cmd::Quick => {
            for (label, path) in quick_access() {
                println!("{:<12} {}", label, path.display());
            }
        }
    }
    Ok(())
}

fn print_tree(path: &std::path::Path, level: usize, max_depth: usize) {
    let indent = "  ".repeat(level);
    println!("{indent}{}", path.file_name().unwrap_or(path.as_os_str()).to_string_lossy());
    if level >= max_depth {
        return;
    }
    if let Ok(entries) = read_dir_sorted(path, SortMode::Name, false) {
        for e in entries.iter().filter(|e| e.is_dir) {
            print_tree(&e.path, level + 1, max_depth);
        }
    }
}

/// Мини-хелпер над io::Error, чтобы не тянуть полноценный anyhow:
/// добавляет контекст к строке ошибки (zero-dependency подход — меньше бинарник).
mod anyhow_lite {
    use std::fmt;

    #[derive(Debug)]
    pub struct Error(String);

    impl fmt::Display for Error {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl std::error::Error for Error {}

    impl From<std::io::Error> for Error {
        fn from(e: std::io::Error) -> Self {
            Error(e.to_string())
        }
    }

    pub type Result<T> = std::result::Result<T, Error>;

    pub trait Ctx<T> {
        fn ctx(self, label: &str) -> Result<T>;
    }

    impl<T> Ctx<T> for std::result::Result<T, std::io::Error> {
        fn ctx(self, label: &str) -> Result<T> {
            self.map_err(|e| Error(format!("{label}: {e}")))
        }
    }
}
