# one_big_script

Rust-версия `OneBigScript_v13.py`.

Проект читает данные из DBF/XLSX, преобразует их по бизнес-правилам и формирует итоговые XLSX-файлы для нескольких режимов работы:
- `Статика` (`PPL`)
- `Сводка` (`SS`)
- `ГДИ`
- `Ведомость`
- `из ГДМ` — прогнозные таблицы по выгрузкам тНавигатора

Часть логики уже полностью нативная, часть маршрутов пока работает в режиме совместимости с прежним Python-пайплайном.

## С чего начинать знакомство с кодом

Для первого чтения удобнее идти в таком порядке:
1. [`src/main.rs`](src/main.rs) — вход в GUI-приложение.
2. [`src/cli.rs`](src/cli.rs) — базовые CLI-параметры GUI.
3. [`src/ui/app.rs`](src/ui/app.rs) — оболочка интерфейса, формы модулей и запуск фоновых задач.
4. [`src/modes/mod.rs`](src/modes/mod.rs) — список бизнес-модулей.
5. [`src/modes/ppl/`](src/modes/ppl), [`src/modes/ss/`](src/modes/ss), [`src/modes/gdi/`](src/modes/gdi), [`src/modes/ved/`](src/modes/ved) — основная бизнес-логика.
6. [`src/paths/`](src/paths) — разрешение путей к DBF/XLSX и выходным каталогам.
7. [`src/domain.rs`](src/domain.rs) — коды месторождений и человекочитаемые названия.

Более подробная карта проекта вынесена в [`PROJECT_MAP.md`](PROJECT_MAP.md).

## Архитектура в двух словах

- `main.rs` поднимает `eframe`/`egui`-приложение.
- `ui/app.rs` держит состояние форм и отправляет тяжёлую обработку в фоновые потоки.
- `modes/*` содержат расчёты, фильтрацию записей, чтение DBF/XLSX и генерацию итоговых Excel-файлов.
- `paths/` изолирует логику поиска входных файлов для Windows/Linux и режимов `test/debug`.
- `bin/batch_run.rs` даёт headless-режим для ручной проверки, smoke-тестов и интеграции в скрипты.
- `vendor/rust_xlsxwriter/` — форк библиотеки записи xlsx, см. [`ФОРК.md`](vendor/rust_xlsxwriter/ФОРК.md).

Зависимости идут в одну сторону: `ui` и `cli` знают про `modes`, обратной связи нет.

Типовой поток данных:

`DBF/XLSX -> paths::* -> modes::*::execute -> преобразование -> XLSX output`

## Что уже перенесено нативно

- launcher / главное окно
- `Статика` (`PPL`)
- `Сводка` (`SS`)
- `ГДИ` для DBF-маршрута
- `Ведомость`
- `из ГДМ`

## Сборка и запуск

Сборка debug:

```powershell
cargo build
```

Сборка release:

```powershell
cargo build --release
```

GUI:

```powershell
cargo run
```

Headless runner:

```powershell
cargo run --bin batch_run -- ppl --mests 1 --year 2024
```

Выгрузка `из ГДМ` из консоли (полный список ключей и пояснения — в `--help`):

```powershell
cargo run --release -- --gdm --gdm-mest bngkm,hgkm --gdm-step quarter --gdm-start 01012020 --gdm-end max
```

На Windows нужны MSVC Build Tools.

## Полезные команды для разработки

```powershell
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Где что менять

- Нужно поменять интерфейс или поведение формы: [`src/ui/screens`](src/ui/screens)
- Нужно подвинуть элемент интерфейса: [`src/ui/screens/layout.rs`](src/ui/screens/layout.rs)
- Нужно поменять чтение/преобразование данных конкретного режима: [`src/modes`](src/modes)
- Нужно поменять расположение входных/выходных файлов: [`src/paths`](src/paths)
- Нужно добавить новое месторождение или подпись: [`src/domain.rs`](src/domain.rs)

Файл делим, когда он перевалил за ~400 строк, а не заранее — подробнее в [`PROJECT_MAP.md`](PROJECT_MAP.md).




##Сборка под astra linux v1
podman run --rm -it -v "$PWD:/src" -w /src docker.io/library/debian:buster bash

cat > /etc/apt/sources.list <<'EOF'

deb http://archive.debian.org/debian buster main contrib non-free

deb http://archive.debian.org/debian-security buster/updates main contrib non-free

EOF



apt-get -o Acquire::Check-Valid-Until=false update

apt-get install -y ca-certificates curl git build-essential pkg-config libssl-dev



curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

. "$HOME/.cargo/env"



cargo build --release --locked

strip target/release/onebigscript



##Сборка под astra linux v2
podman run --rm -it \
    --network=host \
    -e http_proxy=http://127.0.0.1:10808 \
    -e https_proxy=http://127.0.0.1:10808 \
    -e HTTP_PROXY=http://127.0.0.1:10808 \
    -e HTTPS_PROXY=http://127.0.0.1:10808 \
    -v "$PWD:/src" \
    -w /src \
    docker.io/library/debian:buster \
    bash -lc '
set -e

cat > /etc/apt/sources.list <<'"'"'EOF'"'"'
deb [check-valid-until=no] http://archive.debian.org/debian buster main contrib non-free
deb [check-valid-until=no] http://archive.debian.org/debian-security buster/updates main contrib non-free
EOF

apt-get \
    -o Acquire::Check-Valid-Until=false \
    -o APT::Update::Error-Mode=any \
    update

apt-get install -y \
    ca-certificates \
    curl \
    git \
    build-essential \
    pkg-config \
    libssl-dev

curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y

. "$HOME/.cargo/env"

cargo build --release --locked
strip target/release/onebigscript

echo
echo "Сборка завершена:"
file target/release/onebigscript
'
