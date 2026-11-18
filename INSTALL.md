# Установка `zя`

## Быстрая установка существующего бинарника

Откройте папку проекта и запустите один из файлов:

```powershell
.\install.ps1
```

или двойной клик:

```text
install.bat
```

Установщик:

- копирует программу в `%APPDATA%\Abrakadra\zya.exe`;
- запускает встроенную команду `zya.exe install`, которая кладет рабочую копию в `%APPDATA%\zя\zя.exe`;
- создает настройки в `%APPDATA%\zя\config.json`, если их нет;
- отключает предыдущую копию программы, если она запущена;
- добавляет автозапуск;
- запускает `zя` в фоновом режиме.

## Установка с новой сборкой

Если вы изменили исходный код и хотите собрать его:

```powershell
.\build.ps1
.\install.ps1
```

Если сборка падает с ошибкой:

```text
error: linker `link.exe` not found
```

нужно установить Visual Studio Build Tools с компонентом C++:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
```

Во время установки выбрать workload: `C++ build tools`.

## Удаление

```powershell
.\Uninstall.ps1
```

или:

```text
uninstall.bat
```

Чтобы оставить настройки и журнал, используйте:

```powershell
.\Uninstall.ps1 -KeepSettings
```
