//! Файловые операции: копирование, перемещение, удаление, создание.
//! Аналог базовых команд shell-оболочки explorer.exe.

use std::ffi::OsString;
use std::fs;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const BUF_SIZE: usize = 1 << 20; // 1 MiB — баланс между syscall-оверхедом и памятью

/// Уникальное имя вместо перезаписи: file.txt -> file (1).txt (как в Windows).
pub fn unique_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..10_000 {
        let cand = parent.join(format!("{stem} ({i}){ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    path.to_path_buf() // маловероятно
}

/// Копирование файла с буферизацией; возвращает число скопированных байт.
pub fn copy_file(src: &Path, dst_dir: &Path) -> io::Result<u64> {
    fs::create_dir_all(dst_dir)?;
    let dst = unique_name(&dst_dir.join(src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "нет имени файла")
    })?));
    let mut r = BufReader::with_capacity(BUF_SIZE, fs::File::open(src)?);
    let mut w = BufWriter::with_capacity(BUF_SIZE, fs::File::create(&dst)?);
    let mut buf = vec![0u8; BUF_SIZE];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n])?;
        total += n as u64;
    }
    w.flush()?;
    Ok(total)
}

/// Рекурсивное копирование дерева каталогов.
pub fn copy_tree(src: &Path, dst: &Path) -> io::Result<u64> {
    if !src.is_dir() {
        return copy_file(src, dst.parent().unwrap_or(Path::new(".")));
    }
    fs::create_dir_all(dst)?;
    let mut total = 0u64;
    for de in fs::read_dir(src)? {
        let de = de?;
        let p = de.path();
        let target = dst.join(de.file_name());
        if p.is_dir() {
            total += copy_tree(&p, &target)?;
        } else {
            let mut r = BufReader::with_capacity(BUF_SIZE, fs::File::open(&p)?);
            let mut w = BufWriter::with_capacity(BUF_SIZE, fs::File::create(&target)?);
            let mut buf = vec![0u8; BUF_SIZE];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                w.write_all(&buf[..n])?;
                total += n as u64;
            }
        }
    }
    Ok(total)
}

/// Перемещение: быстрое rename если на том же томе, иначе copy+delete.
pub fn move_path(src: &Path, dst_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dst_dir)?;
    let dst = dst_dir.join(src.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "нет имени")
    })?);
    if fs::rename(src, &dst).is_ok() {
        return Ok(());
    }
    // другой том — медленный путь
    if src.is_dir() {
        copy_tree(src, &dst)?;
        remove_recursive(src)
    } else {
        copy_file(src, dst_dir)?;
        fs::remove_file(src)
    }
}

/// Удаление файла или дерева (аналог Shift+Delete без корзины).
pub fn remove_recursive(p: &Path) -> io::Result<()> {
    if p.is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
}

/// Перевод файла в «корзину»: simple trash = суффикс `.ruxtrash` рядом с оригиналом.
/// На Windows при наличии windows-sys можно расширить до IFileOperation (Recycle Bin API).
pub fn to_trash(p: &Path) -> io::Result<PathBuf> {
    let parent = p.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "нет родительского каталога")
    })?;
    let name = p.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "нет имени")
    })?;
    let mut tname = OsString::from(name);
    tname.push(".ruxtrash");
    let dst = unique_name(&parent.join(tname));
    fs::rename(p, &dst)?;
    Ok(dst)
}

/// Создать файл / каталог (F2-new в проводнике).
pub fn create_node(base: &Path, as_dir: bool) -> io::Result<PathBuf> {
    let p = unique_name(base);
    if as_dir {
        fs::create_dir(&p)?;
    } else {
        fs::File::create(&p)?;
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_unique() {
        let tmp = std::env::temp_dir().join("rustexplorer_ops_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("a.txt"), b"hello").unwrap();
        let sub = tmp.join("sub");
        let n = copy_file(&tmp.join("a.txt"), &sub).unwrap();
        assert_eq!(n, 5);
        // повторное копирование даёт a (1).txt, а не перезапись
        copy_file(&tmp.join("a.txt"), &sub).unwrap();
        assert!(sub.join("a (1).txt").exists());
        assert_eq!(fs::read(sub.join("a.txt")).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn trash_renames() {
        let tmp = std::env::temp_dir().join("rustexplorer_trash_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("x.dat");
        fs::write(&f, b"1").unwrap();
        let t = to_trash(&f).unwrap();
        assert!(t.to_string_lossy().ends_with(".ruxtrash"));
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&tmp);
    }
}
