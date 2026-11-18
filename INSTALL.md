# Установка `zя`

## Быстрая установка

Одна строка в PowerShell:

```powershell
irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex
```

или из cmd:

```cmd
powershell -NoProfile -Command "irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex"
```

Если репозиторий уже скачан:

```powershell
.\install.ps1
```

Установщик работает так:

1. Ищет уже собранный `zя.exe` или `zya.exe` в `abrakadabra/target/release` или `abrakadabra/target/debug`.
2. Если его нет, скачивает один self-contained файл с GitHub Releases (сейчас v0.1.1, asset `z.exe`; внутри embedded-бинарь с крупными иконками).
3. Проверяет, запущен ли Punto Switcher.
4. Кладет приложение в `%APPDATA%\zя\zя.exe`.
5. Создает `%APPDATA%\zя\config.json` и `diario.log`, если их нет.
6. Останавливает старую копию программы, если она уже запущена.
7. Запускает `zя.exe` без параметров, то есть в рабочем режиме.

## Punto Switcher

Если Punto Switcher запущен, установщик предупредит об этом. Чтобы закрыть его автоматически:

```powershell
.\install.ps1 -StopPunto
```

## Установка с новой сборкой

Если вы изменили исходный код:

```powershell
.\install.ps1 -Build
```

При сборке установщик может автоматически:

- установить Rust через `rustup-init.exe`;
- скачать Visual Studio Build Tools;
- установить workload **C++ build tools**.

Если скачивание Rust не удалось, Rust можно установить вручную:

```powershell
winget install Rust.Rust
```

Если нет `link.exe`:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

Во время установки выбрать workload: `C++ build tools`.

## Запуск без параметров

После установки можно запустить:

```powershell
zya.exe
```

или просто двойной клик по `zya.exe`. Без параметров приложение сразу работает: ставится клавиатурный хук и появляется значок в трее.

## Полезные ключи

```powershell
.\install.ps1 -DryRun
```
Проверяет, что будет установлено, ничего не меняет.

```powershell
.\install.ps1 -NoDownload
```
Не скачивает release, использует только локальную сборку.

```powershell
.\install.ps1 -StopPunto
```
Закрывает Punto Switcher перед установкой.

## Иконки

Трейные иконки:

- прозрачный фон;
- белый текст;
- крупная буква `Я`/`A` по максимуму высоты;
- сплошная буква: автопереключение включено;
- контурная буква: автопереключение выключено.

## Удаление

```powershell
.\Uninstall.ps1
```

или:

```text
uninstall.bat
```

Чтобы оставить настройки и журнал:

```powershell
.\Uninstall.ps1 -KeepSettings
```
