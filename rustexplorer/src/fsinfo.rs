//! Сбор информации о файловой системе: элементы каталога, размеры, спец-папки.
//! Работает на Linux/macOS/Windows (через std + dirs).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Один элемент каталога — то, что показывает «Проводник» в списке файлов.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Сортировка элементов: каталоги первыми, затем по имени (как в explorer.exe).
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SortMode {
    Name,
    Size,
    Date,
}

/// Прочитать содержимое каталога. Ошибки доступа к отдельным файлам не валят
/// весь список — просто пропускаем их (устойчивость выше, чем у naive-реализаций).
pub fn read_dir_sorted(dir: &Path, sort: SortMode, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    for de in fs::read_dir(dir)? {
        let Ok(de) = de else { continue }; // недоступный элемент — пропускаем
        let path = de.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => continue,
        };
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = match de.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push(Entry {
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified: meta.modified().ok(),
            name,
            path,
        });
    }

    entries.sort_by(|a, b| match a.is_dir.cmp(&b.is_dir).reverse() {
        std::cmp::Ordering::Equal => match sort {
            SortMode::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortMode::Size => b.size.cmp(&a.size),
            SortMode::Date => b.modified.cmp(&a.modified),
        },
        ord => ord,
    });
    Ok(entries)
}

/// Человекочитаемый размер: 1.5 MB и т.п. (без аллокаций Regex, быстрый путь).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Спец-папки пользователя (аналог «Быстрый доступ» в проводнике Windows).
pub fn quick_access() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let add = |out: &mut Vec<(String, PathBuf)>, label: &str, dir: Option<PathBuf>| {
        if let Some(d) = dir {
            out.push((label.to_string(), d));
        }
    };
    add(&mut out, "~ Home", dirs::home_dir());
    add(&mut out, "Desktop", dirs::desktop_dir());
    add(&mut out, "Documents", dirs::document_dir());
    add(&mut out, "Downloads", dirs::download_dir());
    add(&mut out, "Pictures", dirs::picture_dir());
    add(&mut out, "Music", dirs::audio_dir());
    add(&mut out, "Videos", dirs::video_dir());
    out
}

/// Рекурсивный подсчёт размера дерева (аналог свойств папки в Windows).
/// Итеративный обход стеком — без риска переполнения на глубоких деревьях.
pub fn tree_size(root: &Path) -> (u64, u64) {
    let mut total = 0u64;
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for de in rd.flatten() {
            let Ok(meta) = de.metadata() else { continue };
            if meta.is_dir() {
                stack.push(de.path());
            } else {
                total += meta.len();
                count += 1;
            }
        }
    }
    (total, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn dirs_first_sort() {
        let tmp = std::env::temp_dir().join("rustexplorer_test_dirsfirst");
        let _ = fs::create_dir_all(tmp.join("zdir"));
        fs::write(tmp.join("afile.txt"), b"x").unwrap();
        let e = read_dir_sorted(&tmp, SortMode::Name, true).unwrap();
        assert_eq!(e[0].name, "zdir");
        assert_eq!(e[1].name, "afile.txt");
        let _ = fs::remove_dir_all(&tmp);
    }
}
