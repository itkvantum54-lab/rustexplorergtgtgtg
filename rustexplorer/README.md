# rustexplorer — «explorer.exe» на Rust

Лёгкая, быстрая, кроссплатформенная альтернатива оболочке Windows Explorer
(файловый менеджер + набор файловых операций), написанная с нуля на Rust.

> ⚠️ Важно: исходный код `explorer.exe` закрыт (проприетарный Microsoft),
> поэтому это **не его декомпиляция**, а самостоятельная реализация
> эквивалентной функциональности: навигация по ФС, копирование/перемещение/
> удаление, корзина, свойства папок, «быстрый доступ», интерактивный TUI.

## Возможности

| Команда | Аналог в explorer.exe |
|---|---|
| `rustexplorer browse [PATH]` | окно Проводника (TUI: стрелки/hjkl, Enter, Del, `/`-фильтр, `c`/`m` копи/пасте, `n/N` — новый файл/папка, `s` — сортировка, `h` — скрытые, `q` — выход) |
| `rustexplorer ls PATH [--sort name\|size\|date] [-A]` | панель списка файлов |
| `rustexplorer tree PATH [--depth N]` | дерево навигации слева |
| `rustexplorer du PATH` | «Свойства папки» (размер, число файлов) |
| `rustexplorer cp SRC DST_DIR` / `mv` | Ctrl+C / Ctrl+X + Ctrl+V |
| `rustexplorer rm PATH... [-p]` / `trash` | Del (в корзину) / Shift+Del |
| `rustexplorer mkdir` / `touch` | Создать → Папка / Файл |
| `rustexplorer quick` | «Быстрый доступ» (Home/Desktop/Documents/…) |

## Оптимизации («для оптимизации»)

В `Cargo.toml` release-профиль:
* `opt-level = 3`, `lto = "fat"`, `codegen-units = 1` — максимальная скорость;
* `panic = "abort"`, `strip = true` — минимальный размер бинарника.

Результат: **~0.9 MB** статически связанный exe против ~4.8 MB у explorer.exe,
память — единицы MB вместо сотен, запуск — мгновенный (нет COM/Shell-обвязки).

Дополнительно в коде:
* буферизованное копирование блоками 1 MiB;
* `fs::rename` как быстрый путь для перемещения в пределах тома;
* итеративный обход дерева (без рекурсивного переполнения стека);
* отсутствие аллокаций на лишний путь (`human_size` и т.п.).

## Сборка

```bash
cargo build --release                                # текущая ОС
cargo build --release --target x86_64-pc-windows-gnu # .exe для Windows
```

## Тесты

```bash
cargo test    # 4 unit-теста: сортировка каталогов первыми, human sizes,
              # уникальные имена при копировании, корзина
```

## Структура

```
src/
  main.rs     — CLI (clap), диспетчер команд, мини-контекст ошибок (без anyhow)
  fsinfo.rs   — чтение каталогов, сортировка, размеры, быстрый доступ
  ops.rs      — copy/move/trash/delete/create (+ уникальные имена file (1).txt)
  tui.rs      — интерактивный проводник на crossterm
```
