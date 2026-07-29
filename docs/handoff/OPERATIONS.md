# Сборка, установка и выпуск

## Требования для сборки

- Rust stable и Cargo;
- `zip` для release packaging;
- Node.js и Python 3 для проверок workflow.

C toolchain, `pkg-config` и заголовки libudev **не нужны**: `hidapi` собран с
чистым Rust-бэкендом `linux-native-basic-udev`. Это сделано ради
immutable-дистрибутивов — на Bluefin `cargo build --release` работает прямо на
хосте, без toolbox и без наслаивания пакетов.

## Локальная сборка

```bash
cargo build --release
./target/release/g6-output --version
./target/release/g6-output doctor
```

`Cargo.lock` зафиксирован в репозитории — это binary application, поэтому
сборки должны быть воспроизводимыми. CI использует `--locked`.

## Установка из исходников

```bash
cargo build --release
./install.sh
```

Установщик:

1. копирует CLI в `~/.local/bin/g6-output`;
2. копирует extension в
   `~/.local/share/gnome-shell/extensions/soundblaster-g6-output@netherg.io/`;
3. через root ставит udev-rule;
4. reload rules и пытается включить extension.

После установки необходимо переподключить G6. Иногда GNOME требует logout/login.

## Установка из release tarball

```bash
tar -xzf soundblaster-g6-output-<version>-x86_64.tar.gz
cd soundblaster-g6-output-<version>-x86_64
./install.sh
```

Release tarball должен уже содержать `bin/g6-output`, поэтому Rust на машине
пользователя не нужен.

## Удаление

Запускать `uninstall.sh` из репозитория или сохранённой распакованной папки
релиза:

```bash
./uninstall.sh
```

Текущее ограничение: uninstall script не устанавливается в постоянное место.
Пользователю нужно сохранить release directory либо скачать ту же версию снова.
В будущем можно добавить `g6-output uninstall` или скопировать script в data dir,
но это не обязательно до первого тестового релиза.

## Диагностика

```bash
g6-output doctor
g6-output status
gnome-extensions info soundblaster-g6-output@netherg.io
lsusb | grep -i '041e:3256'
```

Проверить установленное правило:

```bash
cat /etc/udev/rules.d/70-soundblaster-g6-output.rules
```

Проверить локальные файлы:

```bash
command -v g6-output
ls -la ~/.local/share/gnome-shell/extensions/soundblaster-g6-output@netherg.io
cat "${XDG_STATE_HOME:-$HOME/.local/state}/g6-output/output"
```

## Частые проблемы

### `Device not found`

- проверить USB-подключение и `lsusb`;
- убрать проблемный hub;
- проверить VID/PID;
- проверить, существует ли ожидаемый interface;
- переподключить устройство.

### `failed to open ... Check the udev rule`

- убедиться, что rule установлен;
- выполнить reload rules;
- переподключить G6;
- убедиться, что пользователь находится в активной local session;
- проверить права соответствующего `/dev/hidraw*`.

Не решать проблему постоянным запуском CLI через `sudo` и не заменять rule на
`MODE="0666"` без отдельного обоснования безопасности.

### Extension не появился

- проверить `gnome-extensions info`;
- выполнить enable;
- сделать logout/login;
- проверить соответствие текущей версии GNOME `metadata.json`;
- проверить наличие бинарника в жёстко заданном пути `~/.local/bin/g6-output`.

### UI показывает неправильный output

`status` читает устройство, поэтому расхождение означает реальную проблему.
Сверить напрямую:

```bash
g6-output doctor
```

Строки `device:` и `cached:` показывают фактический выход и кеш рядом. Индикатор
обновляется при открытии меню; события hot-plug пока не отслеживаются, поэтому
после переподключения G6 меню нужно открыть заново.

### `the device rejected the command`

Устройство подтвердило не тот режим, который был запрошен. Проверить, не
управляет ли G6 другая программа одновременно, и повторить команду.

### `the device did not acknowledge`

Ответ не пришёл за 1 с. Переподключить устройство и проверить `doctor`.

## Release workflow

Workflow запускается на push в `main`, pull request, вручную и по тегам `v*`.

Рекомендованный выпуск:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Ожидаемые артефакты:

```text
soundblaster-g6-output-0.1.0-x86_64.tar.gz
soundblaster-g6-output@netherg.io.zip
SHA256SUMS
```

`scripts/package.sh` сверяет `VERSION` с версией из `Cargo.toml` и падает при
расхождении, поэтому тег `v0.2.0` без бампа `Cargo.toml` не соберётся. Версию в
`Cargo.toml` нужно поднимать до создания тега.

## Откат

Если команда ведёт себя неправильно:

1. отключить extension;
2. выполнить `uninstall.sh`;
3. переподключить G6;
4. при необходимости вручную удалить udev-rule и reload rules;
5. переключить устройство известной рабочей утилитой или в Windows.

Проект не меняет firmware и не должен записывать постоянные файлы на устройство,
но reverse-engineered protocol требует осторожного тестирования.
