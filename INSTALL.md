# Установка `z.exe`

## Быстрая установка

Требуется запуск от администратора.

Одна строка в PowerShell:

```powershell
irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex
```

или из cmd:

```cmd
powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/demoonk/Zya/main/install.ps1 | iex"
```

Если репозиторий уже скачан:

```powershell
.\install.ps1
```

Установщик работает так:

1. Скачивает один self-contained файл с GitHub Releases: release asset называется `z.exe`, локально он сохраняется как `z.exe`.
2. Проверяет, запущен ли Punto Switcher.
3. Кладет приложение в `%APPDATA%\zя\z.exe`.
4. Создает `%APPDATA%\zя\config.json` и `diario.log`, если их нет.
5. Останавливает старую копию программы, если она уже запущена.
6. Запускает `z.exe` без параметров, то есть в рабочем режиме.

## Punto Switcher

Если Punto Switcher запущен, установщик предупредит об этом. Чтобы закрыть его автоматически:

```powershell
.\install.ps1 -StopPunto
```

## Запуск без параметров

После установки можно запустить:

```powershell
z.exe
```

или просто двойной клик по `z.exe`. Без параметров приложение сразу работает как `run headless`: ставится клавиатурный хук и появляется RU/EN значок в трее.

## Полезные ключи

```powershell
.\install.ps1 -NoStart
```
Установить, но не запускать.

```powershell
.\install.ps1 -StopPunto
```
Закрывает Punto Switcher перед установкой.

## Иконки

Трейные иконки:

- прозрачный фон;
- белый текст;
- текст `RU`/`EN` на всю высоту 32×32 px.

В меню значка есть:

- `Исключить активное окно`;
- `Исключить папку активного окна`;
- `Исключения` — список и удаление;
- `Сбросить исключения`.

Из командной строки:

```powershell
z.exe exclude add chrome.exe
z.exe exclude add-dir C:\Games
z.exe exclude list
```

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
