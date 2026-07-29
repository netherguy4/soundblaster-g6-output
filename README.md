# Sound Blaster G6 Output for GNOME

Минимальный GNOME-виджет для переключения физического выхода Creative
Sound BlasterX G6 между **наушниками** и **динамиками**.

- управление устройством написано на Rust;
- GNOME Shell UI — тонкое расширение на GJS;
- фонового демона нет;
- установка и удаление выполняются одним скриптом;
- релизы собираются GitHub Actions при создании тега `v*`.

> Проект экспериментальный. USB-протокол G6 не документирован Creative и был
> восстановлен сообществом. Используйте на свой риск.

Документация для передачи проекта разработчику или AI-агенту: [HANDOFF.md](HANDOFF.md).

## Что входит в MVP

- значок Sound Blaster G6 в верхней панели GNOME;
- меню `Headphones` / `Speakers`;
- нативный Rust CLI `g6-output`;
- чтение фактического выхода из устройства и подтверждение записи по ACK;
- команда диагностики доступа к HID;
- безопасное udev-правило с `TAG+="uaccess"`;
- установка в домашний каталог, совместимая с Bluefin;
- готовая сборка и публикация GitHub Release.

Поддерживаемое устройство:

```text
Vendor ID:  041e
Product ID: 3256
Interface:  4
```

Расширение заявляет GNOME Shell 48–50. Фактически проверено на GNOME Shell
50.3 (Bluefin 44); 48 и 49 заявлены по совместимости API и не тестировались.

## Установка готового релиза

Скачайте `soundblaster-g6-output-<version>-x86_64.tar.gz` из GitHub Releases:

```bash
tar -xzf soundblaster-g6-output-*.tar.gz
cd soundblaster-g6-output-*
./install.sh
```

Скрипт:

1. копирует бинарник в `~/.local/bin/g6-output`;
2. копирует расширение GNOME в `~/.local/share/gnome-shell/extensions/`;
3. через `sudo` устанавливает только udev-правило в `/etc/udev/rules.d/`;
4. пытается включить расширение.

После установки переподключите G6. Если значок не появился, выйдите из сеанса
и войдите снова, затем выполните:

```bash
gnome-extensions enable soundblaster-g6-output@netherg.io
```

## Установка из исходников

Нужны только Rust и Cargo. `hidapi` собирается с чистым Rust-бэкендом
(`linux-native-basic-udev`), поэтому ни компилятор C, ни `pkg-config`, ни
заголовки libudev не требуются:

```bash
cargo build --release
./install.sh
```

Это сделано специально ради immutable-дистрибутивов: на Bluefin сборка идёт
прямо на хосте, без toolbox и без наслаивания пакетов.

## Использование CLI

```bash
g6-output set headphones
g6-output set speakers
g6-output toggle
g6-output status
g6-output doctor
```

`status` опрашивает устройство и печатает `headphones`, `speakers`,
`disconnected` либо `unknown`, поэтому переключение сторонней программой
отражается корректно. Локальный файл состояния остаётся только резервом на
случай отсоединённого устройства.

`toggle` тоже читает фактический выход, а не угадывает его.

Каждая команда `set` подтверждается ответом устройства: если G6 применил не тот
режим, команда завершается ошибкой вместо ложного успеха.

## Удаление

Из распакованной папки релиза или репозитория:

```bash
./uninstall.sh
```

Скрипт отключает и удаляет расширение, CLI, локальное состояние и udev-правило.
После удаления иногда требуется повторный вход в GNOME.

## Создание релиза

Workflow находится в `.github/workflows/release.yml`.

Проверка обычного push/PR:

- `cargo fmt`;
- `cargo clippy`;
- `cargo test`;
- release build;
- проверка Bash, JSON и синтаксиса расширения;
- загрузка готового архива как Actions Artifact.

Публикация релиза:

```bash
git tag v0.1.0
git push origin v0.1.0
```

GitHub Actions создаст Release и приложит:

```text
soundblaster-g6-output-0.1.0-x86_64.tar.gz
soundblaster-g6-output@netherg.io.zip
SHA256SUMS
```

## Структура

```text
src/main.rs                         Rust CLI и HID-команда
extension/extension.js              GNOME Shell индикатор
extension/metadata.json             метаданные GNOME 48–50
packaging/*.rules                   udev-доступ к hidraw
install.sh / uninstall.sh           установка и удаление
scripts/package.sh                  упаковка релиза
.github/workflows/release.yml       CI и GitHub Release
```

## Ограничения MVP

- поддерживается только Sound BlasterX G6 с PID `3256`;
- только переключение наушники/динамики;
- нет SBX, EQ, Direct Mode и других настроек;
- нет hot-plug: состояние обновляется при открытии меню, а не по событию;
- проверено на одной конфигурации (Bluefin 44, GNOME 50.3, x86_64).
