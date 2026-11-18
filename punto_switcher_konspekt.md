# Punto Switcher — разбор принципов для написания собственного клона

Источники: puntos.ru, Yandex Support (punto-win), Habr (Ghbdtn, Зябра), GitHub: Laex/Ghbdtn, linuxbuh/xneur, olegius88/keyswitch, alexantoshuk/kbdf, mizuhomizuho/punto-switcher-for-linux, Krot66/LangBarXX, rundax/SwitchFix, kertser/KeyboardSwitcher.

---

## 1. Точный механизм (что происходит при «не той» раскладке)

Конвейер одинаковый у всех реализаций:

1. **Глобальный перехват ввода.** Windows: `WH_KEYBOARD_LL`; Linux X11: XRecord; Wayland: grab клавиатуры. Хук видит *scan code* (физическую клавишу) + VK + shift-состояние.
2. **Буфер текущего слова.** Все «буквенные» символы накапливаются в `TInputBuffer` (Ghbdtn) / `_buffer` (xneur). Не-буквы (пробел, пунктуация, цифры) не входят в «ядро» слова, но определяют его границу.
3. **Триггер анализа.** Классический Punto Switcher: «если программа видит недопустимое сочетание … после нажатия **пробела, Enter или Tab** происходит автоматическое переключение раскладки». Современные клоны добавляют:
   - **idle-триггер** (KeySwitch: пауза 0.7 с — проверка «висящего» слова);
   - **раннее переключение по префиксу** (`ghbd` → `прив`, модель префикса, длина 4–12 символов);
   - **мягкие пунктуаторы**: точка, запятая, `:`, `;` тоже считаются границей, но «двусмысленные» клавиши (`,`/`.` в русской раскладке = `б`/`ю`) держатся в буфере, пока не ясно, буква это или знак.
4. **Анализ пары строк.** `typed = word`, `mirrored = swapLayout(word)`. Считается правдоподобность обеих:
   `scoreEN(typed)`, `scoreRU(mirrored)` (и наоборот) — переключение только если `mirroredScore - typedScore >= margin`.
5. **Физическая подмена текста.** Подменить уже введённый текст в Windows можно только так: `Backspace` × len(word), затем ввод исправленных символов, затем разделитель (пробел/Enter), если он не был «съеден». Всё одним пакетом `SendInput` (иначе мерцание и дерганье каретки).
6. **Смена раскладки.** `LoadKeyboardLayoutW("00000409"/"00000419", KLF_ACTIVATE)` + `ActivateKeyboardLayout` + `PostMessage(hwndActive, WM_INPUTLANGCHANGEREQUEST, INPUTLANGCHANGE_SYSCHARSET, hkl)`. `GetKeyboardLayout()` у активного окна часто врёт — надежнее опираться на скан-коды.
7. **Отмена и обучение.** Hotkey = инверсия последней конвертации (toggle). xneur: «wrong input добавляется в словарь, если дважды вручную конвертирован в одну раскладку»; Ghbdtn: на каждый процесс/слово два счётчика (запрет/предпочтение, 0..12), порог подтверждается 3 раза.

Пример (классика): `ghbdtn` → n-граммы EN = 0, гласных нет, штраф за форму; RU-зеркало `привет`: 5 биграмм + 3 триграммы + словарь + 2 гласные + бонус «известного ключа» = перевес ~36 баллов → переключение.

---

## 2. Эвристики «невозможных» сочетаний

### 2.1 Русский (из `share/languages/ru/proto` xneur — 2-граммы, которых в русском нет)

Категории (реальный список xneur, 158 пар):

| Тип | Примеры |
|---|---|
| Гласная + ь/ъ/ы на конце «мягкого» основания | `аь`, `оь`, `уы`, `ыы`, `эы`, `яь`, `ёь` |
| Согласная + `й` после твёрдой согласной | `бй`, `вй`, `гй`, `дй`, `жй`, `зй`, `кй`, `лй`, `мй`, `нй`, `рй`, `сй`, `тй`, `фй`, `хй`, `цй`, `чй`, `шй` |
| `ъ` почти везде | `ъа`, `ъб`, `ъв`, …, `ъщ`, `ъы`, `ъь` (кроме редких), `аъ`, `иъ`, `оъ`, `уъ`, `къ`, `лъ`, `мъ`, `эъ`, `юъ`, `яъ` |
| `щ` + согласная (щ всегда перед гласной) | `щб`, `щг`, `щд`, `щж`, `щз`, `щк`, `щл`, `щп`, `щс`, `щт`, `щф`, `щх`, `щц`, `щч`, `щш`, `щщ` |
| Удвоения/кластеры | `шш`, `шщ`, `щщ`, `цч`, `чщ`, `гх`, `жх`, `фх`, `цх`, `фц`, `цф`, `цш`, `чч`? |
| `ч/ш/ц` + ряд согласных | `чг`, `чз`, `чп`, `чф`, `шг`, `шд`, `шж`, `шз` |
| `ь` в начале/после гласной | `ьа`, `ьй`, `ьр`, `ьу`, `ьы`, `ьь`, `ьъ` |
| `ё` почти не стоит в начале/перед согласной | `ёа`, `ёе`, `ёё`, `ёф`, `ёъ`, `ёы`, `ёь` |

Отдельные «старт-правила» (Punto-style): слово **не может начинаться** с `ь`, `ъ`, `ы` (кроме имён/аббревиатур) — типичный признак `ьщь` = `mom`.

### 2.2 Английский (`share/languages/us/proto`, 70 пар)

`bq cj cx dx fq fv fx fz gx hx jb jd jf jh jl jm jp jq jt jv jw jx jz kx kz mx mz pq px qc qd qe qf qg qh qj qk ql qm qn qo qp qs qv qx qy qz sx tq tx vc vf vj vm vp vq vw vx vz wq wv wx xd xj xk yq zf zj zx`

Выводы, которые полезно зашить в собственный клон:
- **q** почти всегда + `u` (исключения: `qat`, `qi`, `inqsha`) → `qc`, `qe`, `qi?`, `qo`, `qx` — красный флаг;
- **x** почти не соседствует с согласными (`xd`, `xj`, `xk`, `sx`, `tx`, `zx`);
- **j** + согласная (`jd`, `jf`, `jh`, `jl`, `jm`, `jp`, `jq`, `jt`, `jv`, `jw`);
- **f/v/m/w/z + q/x/v** редки;
- **c** почти не идёт после `v`, `w`;
- слова **без гласных** (кроме `a`, `I`, `my`, `by`, `gym`, `lynx`, `hmm`, `shh`) — сильный признак чужой раскладки: `ghbdtn` (0 гласных) → русская.
- `y` в английском — «полугласная»: Ghbdtn даёт ей **+1** балл и только если уже найдена типичная английская n-грамма, иначе русское `н` маскируется под `y`.

### 2.3 Комбинация признаков (таблица Ghbdtn)

| Слагаемое | Балл |
|---|---|
| Знакомая биграмма | +1 |
| Знакомая триграмма | +2 |
| Полное попадание в словарь частых слов | +10 |
| Наличие гласных | +2 (для `y` — +1, только при подтверждённой EN n-грамме) |
| Невозможная буква в начале (`ь`, `ъ`, `ы`) | −2 / −4 |
| Запрещённая пара (`ьь`, `щк`, `ыь`) | −4 |
| 4 согласных подряд | −3 |
| Контекст фразы (2–3 предыдущих слова на том же языке) | до +3 |
| «Известный ключ» (`ghbdtn`→`привет`, `xnj`→`что`) | +8 |

**Пороги (margin):** обычный 4, осторожный 7, агрессивный 2, раскладка неизвестна 6 (в более поздней версии Ghbdtn: 28 / +12 / −8 / 36; ручной hotkey −16). Ключевая идея: контекст сам по себе не должен переключать слово, он только разрешает спорные короткие случаи.

---

## 3. Статистика: биграммы/триграммы и вероятности

Три рабочих подхода (от простого к сложному):

1. **Запретные n-граммы (xneur).** `proto` = список пар/троек, которых в языке не бывает. Алгоритм (`lib/ai/detection.c`):
   - если в слове на текущей раскладке **0** попаданий в `proto` → оставить как есть;
   - если есть попадания, а у зеркала на другой раскладке **0** → переключить;
   - если попадания есть в обеих → не трогать.
   Порядок проверок: словарь → aspell/enchant → похожие слова (Levenshtein < 3) → 2-gram proto → 3-gram proto.
2. **Подсчёт частотных n-грамм (Ghbdtn).** `ScoreNGrams`: скользящее окно, бинарный поиск в отсортированных массивах, `+1` за биграмму, `+2` за триграмму. `ghbdtn` в EN = 0; `привет` в RU = 5 биграмм + 3 триграммы = 11.
3. **Markov cost-модель 2-го порядка (Ghbdtn.LetterModel).** Таблица «стоимость буквы после двух предыдущих» в четвертях бита (4 = 1 бит), строится из частотных списков слов обоих языков. Невозможные переходы имеют большую стоимость; сравнивается разница `LetterModelCost(word, EN)` vs `LetterModelCost(mirror, RU)`. Отдельно есть `PrefixCost` для ещё не законченного слова. Плюс стемминг: список окончаний (3→1 буква) + зеркальная проекция в QWERTY (`ами`→`fvb`, `ый`→`sq`), корень не короче 4 символов, таблица супплетивных семейств (`идти/шёл`, `ребёнок/детки`) — чтобы отказ пользователя на одной форме распространялся на все.

Практический совет для клона: начните с (1) запретных биграмм + (2) «нет гласных» + (3) словарь 3–5k самых частых слов EN/RU; Markov-таблицу добавляйте второй итерацией.

---

## 4. Цифры, `ё`, апостроф, дефис, капс, смешанные слова, имена, аббревиатуры

- **Цифры.** Сами цифры не зависят от раскладки, но `Shift+цифра` даёт разные символы: EN `! @ # $ % ^ & * ( ) _ +`, RU `! " № ? % & / ( ) - +`. Поэтому `@`, `#`, `$`, `_` «в русской» просто не существуют, а `№`, `?`, `/` в английской — редкость. Ghbdtn штрафует такие клавиши (`InnerKeyCost = 60`): «клавиша, которая на английской раскладке даёт русскую букву (`,`, `.`, `;`, `[`, `]` и их shifted-варианты) почти никогда не часть английского слова».
- **`ё`** — полноценная русская буква; физическая клавиша `` ` `` ↔ `ё`. Не нормализуйте `ё`→`е` при сравнении (иначе `ё`/`е` дают несовпадение при перенаборе).
- **Апостроф / кавычки.** EN `'` ↔ RU `э`, EN `"` ↔ RU `№`(на части раскладок). В клонах (`kbdf`) маппинг строится как парная таблица символов; апостроф в английских словах (`don't`) — сильный «не трогать» маркер; дефис/подчёркивание — защита слова от конвертации.
- **Защита смешанных токенов.** Ghbdtn перед анализом проверяет `IsProtectedToken`: URL, пути, e-mail, camelCase, слова с цифрами, технические термины (`dev`, `ctrl`, `git`, `api`, `ssh`, `id`, `ip`…), файловые расширения (62 шт.). Отдельно — «двусмысленные» термины: `exe` в русской = `учу`, `vs` в русской = `мы`, поэтому их не переконвертируют.
- **Капс.** Сравнение регистронезависимое, но перенабор сохраняет исходный регистр букв; при неопределённости не переключать. Отдельная эвристика «две заглавные подряд» и «CorrectIncidentalCaps» (есть даже звуковой сигнал в xneur).
- **Имена/аббревиатуры.** Правила «Совпадать с данным сочетанием» + «Учитывать регистр» (чтобы `Ын-джон` не превратился в `Hn-djon`, а `внж` не стал `dy;`). В xneur есть `ReplaceAbbreviationIgnoreLayout Yes` — аббревиатуры не конвертируются независимо от раскладки.
- **Пароль.** Автопереключение в парольных полях запрещается: `EM_GETPASSWORDCHAR` для Win32, `UIA_IsPasswordPropertyId` через UI Automation (асинхронно, с кэшем); буфер сбрасывается. KeySwitch исключает `KeePassXC`, `1Password`, `Bitwarden`.

---

## 5. Горячие клавиши оригинала и ключевые настройки

**Punto Switcher (по умолчанию, Windows):**

| Действие | Комбинация |
|---|---|
| Отменить/повторить конвертацию последнего слова | `Break` (Pause/Break) |
| Сменить раскладку выделенного текста | `Shift + Break` |
| Сменить регистр выделенного текста | `Alt + Break` |
| Транслитерировать выделенное | `Alt + Scroll Lock` |
| Вставить текст без форматирования | `Ctrl + Alt + V` |
| Найти в Яндексе | `Win + S` |
| Открыть настройки | `Ctrl + Win + Alt + P` |
| Вкл/выкл автопереключение | `Ctrl + Win + Alt + K` |
| Звуковые эффекты | `Ctrl + Win + Alt + S` |
| Автозамена: добавить/настроить/показать | `Ctrl+Win+Alt+Q / E / L / M` |
| Буфер обмена: транслит/раскладка/история | `Ctrl+Win+Alt+R / X / H` |
| Дневник | `Ctrl+Win+Alt+D / C / V` |

В ранних версиях и в macOS-варианте: `Alt` = отмена/обратная конвертация последнего слова, `Ctrl+Alt` = вкл/выкл автопереключения.

**Ключевые настройки:** «Автопереключение» вкл/выкл; «Программы-исключения» (по файлу, по заголовку окна, по папке); «Не взаимодействовать с программами-исключениями» (снижает нагрузку CPU в играх); «Правила переключения» (содержит/начинается/совпадает + «учитывать регистр» + действие «переводить/не переводить»); «Предлагать добавить правило после N отмен подряд»; индикатор раскладки в трее/у каретки; звуки; дневник; автозамена; транслит.

**Клоны:** KeySwitch — `Pause` = конвертировать/переключить, `Pause` дважды = обучить правило, `Ctrl+Alt+Z` = отмена (10 с), «переключать одной клавишей» (левый/правый Shift или Ctrl, off by default, не работает со Sticky Keys). Caramba — двойной Shift. xneur — `Break`, `Shift+Break`, `Alt+Scroll_Lock`, `Shift+Tab` (аббревиатуры), `Tab` (подтверждение автодополнения).

---

## 6. Маппинг физических клавиш QWERTY ↔ ЙЦУКЕН

| Физ. клавиша | EN | RU (без Shift) | EN (Shift) | RU (Shift) |
|---|---|---|---|---|
| `` ` `` | `` ` `` | `ё` | `~` | `Ё` |
| 1 | 1 | 1 | `!` | `!` |
| 2 | 2 | 2 | `@` | `"` |
| 3 | 3 | 3 | `#` | `№` |
| 4 | 4 | 4 | `$` | `?` |
| 5 | 5 | 5 | `%` | `%` |
| 6 | 6 | 6 | `&` | `&` |
| 7 | 7 | 7 | `'` | `/` |
| 8 | 8 | 8 | `(` | `(` |
| 9 | 9 | 9 | `)` | `)` |
| 0 | 0 | 0 | `_` | `-` |
| `-` | `-` | `-` | `+` | `+` |
| `=` | `=` | `=` | `+` | `=` |
| Q W E R T Y U I O P | q w e r t y u i o p | й ц у к е н г ш щ з | Q…P | Й Ц У К Е Н Г Ш Щ З |
| `[` `]` | `[` `]` | `х` `ъ` | `{` `}` | `Х` `Ъ` |
| A S D F G H J K L | a s d f g h j k l | ф ы в а с р о л д | A…L | Ф Ы В А С Р О Л Д |
| `;` `'` | `;` `'` | `ж` `э` | `:` `"` | `Ж` `Э` |
| Z X C V B N M | z x c v b n m | я ч с м и т ь | Z…M | Я Ч С М И Т Ь |
| `,` `.` `/` | `,` `.` `/` | `б` `ю` `.` | `<` `>` `?` | `Б` `Ю` `,` |

Код (kbdf, Python) — парная таблица, симметричная:

```python
EN = "`qwertyuiop[]asdfghjkl;'zxcvbnm,.~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>#&"
RU = "ёйцукенгшщзхъфывапролджэячсмитьбюЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ№?"
EN_RU = str.maketrans(EN, RU)
text.translate(EN_RU)
```

Важно: маппинг лучше строить от **скан-кодов** (Scan Set 1), а не от字符, иначе `VK`/`ToUnicodeEx` дадут сюрпризы на `;`,`[`,`]`,`,`,`.`.

---

## 7. Низкий уровень: hook и «перенабор»

```c
// 1. Установка хука
HHOOK h = SetWindowsHookExW(WH_KEYBOARD_LL, LowLevelKeyProc, NULL, 0);

LRESULT CALLBACK LowLevelKeyProc(int code, WPARAM vk, KBDLLHOOKSTRUCT *k) {
    if (code != HC_ACTION) return CallNextHookEx(h, code, vk, k);
    if (k->dwExtraInfo == MY_SIGNATURE) return CallNextHookEx(h, code, vk, k); // не поглощать свои SendInput
    // scan = k->scanCode  -> физическая клавиша
    if (isLetterOrBoundary(k)) buffer.push(mapPhysical(k->scanCode, shiftState));
    if (isWordBoundary(k) && shouldSwitch(buffer)) queue.push(buffer); // не делать SendInput прямо из хука!
    return CallNextHookEx(h, code, vk, k);   // никогда не возвращаем 1 (не гасим ввод)
}
```

```c
// 2. Пакет подмены: N Backspace + символы через KEYEVENTF_UNICODE + разделитель
INPUT in[2*N + M + 1];
for (i=0;i<N;i++){ in[2i]=KEYBACKSPACE_DOWN(SIG); in[2i+1]=KEYBACKSPACE_UP(SIG); }
for (j=0;j<M;j++){ in[2N+2j]=UNICODE_CHAR(chars[j]); in[2N+2j+1]=UNICODE_CHAR_UP(chars[j]); }
in[2N+2M] = delimiter (space/enter) if not already consumed;
SendInput(count, in, sizeof(INPUT));
PostMessageW(GetActiveWindow(), WM_INPUTLANGCHANGEREQUEST, 0x1, hklTarget);
```

Ключевые практики (Ghbdtn, KeySwitch):
- **Сигнатура** `dwExtraInfo` на всех синтетических событиях + временное `UnhookWindowsHookEx`/`CallNextHookEx`-пропуск, иначе хук «съедает» собственные нажатия (бесконечная рекурсия, залипшие модификаторы).
- **Два контура**: хук (микросекунды, только подсчёт и очередь) + рабочий поток, который делает `SendInput`, UI Automation, clipboard. Хук, который «думает» десятки миллисекунд, Windows тихо выкидывает из цепочки.
- **Replay-буфер**: всё, что пользователь нажал, пока поток готовил пакет, сохраняется и «досылается» после подмены — иначе Backspace стирает уже начало следующего слова.
- **Отказ по Backspace**: если пользователь сразу после автозамены стёр всё слово (или ≥ половины длинного слова) — это явный отказ, обучаем «не переводить»; 1–2 Backspace в хвосте = обычная опечатка, обучение не трогаем.

---

## 8. Подводные камни

| Проблема | Что делать |
|---|---|
| Пароль/CAPTCHA/2FA | Не анализировать: `EM_GETPASSWORDCHAR`, UIA `IsPassword`, исключения для менеджеров паролей; сбрасывать буфер |
| Терминал, IDE, код | `user_name`, флаги `--no-p`, camelCase, пути, расширения → «защищённые токены»; исключения по процессу/окну; режим «осторожный» (margin выше) |
| Lag | Хук ≤ доли мс; вся математика — плоские отсортированные массивы в RAM; подмена — отдельным потоком; один `SendInput` вместо цикла `keybd_event` |
| Recursion / «авто-переключение обратно» | Сигнатура `dwExtraInfo`, unhook вокруг SendInput, не анализировать собственные события |
| UIPI | Нельзя injecting input в окно с более высоким integrity level (UAC/админ-консоль) — нужен соответствующий уровень или исключение |
| Конфликт с автокоррекцией ОС/IME | macOS TextEdit исправляет слово на пробеле раньше нас; при IME/фонетической раскладке не работать; не перекрывать чужую правку |
| Синтетический ввод (TeamViewer, macros, экранный ввод) | Отдельный фильтр «ввод от других программ» |
| Sticky Keys | Одно-клавишные hotkey-режимы не работают |
| Игры | Отключать полностью: `WH_KEYBOARD_LL` нагружает CPU; Punto даже при выключенном автопереключении тратит процессор, пока не включено «Не взаимодействовать с исключениями» |
| Двусмысленная пунктуация | `,`/`.` — это `б`/`ю` в русской раскладке: решение откладывать, пока не ясно, буква или знак |
| Короткие слова (1–3 буквы) | Только чистый n-граммный скоринг даёт массу ложных срабатываний на `id`, `as`, `in`, `to`; нужны слова-функции и белый список |

---

## 9. Ссылки

- https://puntos.ru/ ; https://puntos.ru/faq.html ; https://puntos.ru/news/6-nastrojki-programmy-punto-switcher.html ; https://puntos.ru/news/4-punto-switcher-gorjachie-klavishi-po-umolchaniju.html
- https://yandex.ru/support2/punto-win/ru/features/autoswitch (правила, исключения, пароль)
- https://yandex.ru/support2/punto-win/ru/settings/hotkeys (таблица hotkeys)
- https://yandex.ru/support2/punto-win/ru/settings/exception (программы-исключения, нагрузка CPU в играх)
- https://habr.com/ru/articles/1093518/ — «Свой переключатель раскладки на Delphi, от низкоуровневого хука до скоринга n-грамм» (Ghbdtn)
- https://github.com/Laex/Ghbdtn — открытый Delphi-клон: `Ghbdtn.Hook`, `Ghbdtn.Analyzer`, `Ghbdtn.Heuristics`, `Ghbdtn.LetterModel`, `Ghbdtn.Fixer`, `tests/Ghbdtn.Tests.Core.pas`
- https://habr.com/ru/articles/1097500/ — «Зябра» (Rust, офлайн, словари, scan set 1, `LoadKeyboardLayoutW` + `WM_INPUTLANGCHANGEREQUEST`)
- https://github.com/linuxbuh/xneur (C, GPL-2) — `lib/ai/detection.c`, `share/languages/ru/proto`, `us/proto`, `xneurrc.5`
- https://github.com/olegius88/keyswitch (Rust) — README: idle-триггер 0.7 с, ранний prefix-переключатель, boundary-модель, `WH_KEYBOARD_LL`/XRecord/Wayland, UIPI
- https://github.com/alexantoshuk/kbdf (Python, pynput) — чистая таблица маппинга + `LoadKeyboardLayoutW`/`PostMessageW`
- https://github.com/mizuhomizuho/punto-switcher-for-linux (bash, xdotool/xte/xsel, xbindkeys `Release + Pause`, `Release + Ctrl+Alt+z`)
- https://github.com/Krot66/LangBarXX (AutoHotkey) — `Lib/StringToAnotherLayout.ahk`, `Lib/SetInputLayout.ahk`, `Lib/Spell.ahk`
- https://github.com/rundax/SwitchFix (Swift, macOS) — «detect on space, delete wrong chars, switch layout, retype», `Cmd+Z` = undo
- https://github.com/kertser/KeyboardSwitcher (C++, LSTM/ONNX) — ML-подход (для сравнения)
- https://grokipedia.com/page/Punto_Switcher — история и hotkeys
- http://kbdlayout.miloush.net/kbdru/3.10.497.1/scancodes — скан-коды русской раскладки
- https://wiredin.ru/articles/instructions/perevod-russko-anglijskaya-raskladka-klaviatury-tablicza/ — таблица соответствия клавиш

---

## 10. Псевдокод_minima

```pseudo
state: word_buf, prev_layout, last_conversion

on_key(scan, shift):
    if synthetic(scan): return pass_through
    ch = physicalMap(scan, shift, currentLayout)
    if isLetter(ch): word_buf.append(ch); maybeEarlyPrefixCheck(word_buf)
    else if isBoundary(ch):
        if word_buf: decide(word_buf); word_buf.clear()

decide(word):
    if word in NEVER_SWITCH or word in ALWAYS_SWITCH: apply rule
    if isProtectedToken(word): return            # url, path, camelCase, digits, underscores
    core = stripPunctuation(word)
    if len(core) < 3: return                     # только слова-функции
    typed_score   = score(core, layoutOf(core))
    mirror_score  = score(swap(core), otherLayout)
    known_bonus   = 8 if core in KNOWN_KEYS else 0
    context_bias  = min(3, sameLanguageInLastWords)
    if mirror_score + known_bonus - typed_score >= MARGIN:   # MARGIN: 4 normal, 7 cautious, 2 aggressive
        enqueueReplacement(word_len, swap(core), delimiter)

enqueueReplacement(n, newtext, delim):
    unhook()
    SendInput([DOWN/UP VK_BACK × n] + [UNICODE chars] + [delim] with signature)
    LoadKeyboardLayout(target); PostMessage(hwnd, WM_INPUTLANGCHANGEREQUEST, hkl)
    replay(held_keys)
    hook()
```
