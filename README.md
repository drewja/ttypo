# ttyper

A terminal-based typing test built with Rust and Ratatui. Forked from [max-niederman/ttyper](https://github.com/max-niederman/ttyper).

<!-- ![Recording](./resources/recording.gif) -->

## added features

- **File mode** - passing a file preserves original line breaks, indentation, and empty lines
- **ASCII mode** (`--ascii`) - skips non-ascii characters during typing, highlighted in yellow
- **Live status bar** - shows WPM, elapsed time, and word progress while typing
- **Full-width progress bar** - visual progress indicator below the prompt
- **Missed words panel** - results screen shows which words had errors
- **Practice missed words** - press `p` on results to practice missed words
- **Repeat test** - press `r` on results to repeat the test
- **Mistake markers on chart** - red dots on the WPM chart show when errors occurred
- **theme** - richer default color scheme using RGB values
- **Stdin support** - pipe text in with `-` (e.g. `man ls | ttyper --ascii -`)

## installation

```bash
cargo install --path .
```

## usage

```
ttyper [OPTIONS] [PATH] [COMMAND]

Arguments:
  [PATH]  Read test contents from the specified file, or "-" for stdin

Options:
  -d, --debug
  -w, --words <N>             Specify word count [default: 50]
  -c, --config <PATH>         Use config file
      --language-file <PATH>  Specify test language in file
  -l, --language <LANG>       Specify test language
      --list-languages        List installed languages
      --no-backtrack          Disable backtracking to completed words
      --sudden-death          Enable sudden death mode to restart on first error
      --no-backspace          Disable backspace
      --ascii                 Display all but skip non-ASCII characters during typing
  -h, --help                  Print help
  -V, --version               Print version
```

### examples

| command                                  |                                          test contents |
| :--------------------------------------- | -----------------------------------------------------: |
| `ttyper`                                 |                50 of the 200 most common english words |
| `ttyper -w 100`                          |               100 of the 200 most common English words |
| `ttyper -w 100 -l english1000`           |              100 of the 1000 most common English words |
| `ttyper --language-file lang`            |                   50 random words from the file `lang` |
| `ttyper text.txt`                        |       contents of `text.txt` with original line layout |
| `ttyper --ascii source.rs`               |      type a source file, skipping non-ASCII characters |
| `man ls \| ttyper --ascii -`             |                  practice typing a man page from stdin |

## languages

The following languages are available by default:

| name                 |                         description |
| :------------------- | ----------------------------------: |
| `c`                  |          The C programming language |
| `cpp`                |        The C++ programming language |
| `csharp`             |         The C# programming language |
| `english100`         |       100 most common English words |
| `english200`         |       200 most common English words |
| `english1000`        |      1000 most common English words |
| `english-advanced`   |              Advanced English words |
| `english-ngrams`     |          300 common English n-grams |
| `english-pirate`     |       50 pirate speak English words |
| `french100`          |        100 most common French words |
| `french200`          |        200 most common French words |
| `french1000`         |       1000 most common French words |
| `galician`           |      185 most common Galician words |
| `german`             |        207 most common German words |
| `german1000`         |       1000 most common German words |
| `german10000`        |      10000 most common German words |
| `go`                 |         The Go programming language |
| `html`               |           HyperText Markup Language |
| `java`               |       The Java programming language |
| `javascript`         | The Javascript programming language |
| `korean100`          |        100 most common Korean words |
| `korean200`          |        200 most common Korean words |
| `norwegian`          |     200 most common Norwegian words |
| `php`                |        The PHP programming language |
| `portuguese`         |    100 most common Portuguese words |
| `portuguese200`      |    200 most common Portuguese words |
| `portuguese1000`     |   1000 most common Portuguese words |
| `portuguese-advanced`|           Advanced Portuguese words |
| `python`             |     The Python programming language |
| `qt`                 |                The QT GUI framework |
| `ruby`               |       The Ruby programming language |
| `rust`               |       The Rust programming language |
| `thai`               |         4000 most common Thai words |
| `spanish`            |       100 most common Spanish words |
| `sql`                |           Structured Query Language |
| `ukrainian`          |     100 most common Ukrainian words |
| `russian`            |       200 most common Russian words |
| `russian1000`        |      1000 most common Russian words |
| `russian10000`       |     10000 most common Russian words |

Additional languages can be added by creating a file in the config language directory with a word on each line. On Linux, the config directory is `$HOME/.config/ttyper/language`; on macOS it's `$HOME/Library/Application Support/ttyper/language`.

## config

Configuration is specified by `config.toml` in the config directory (e.g. `$HOME/.config/ttyper/config.toml`).

Default values:

```toml
# the language used when one is not manually specified
default_language = "english200"

[theme]
# default style (includes empty cells)
default = "none"
# title text
title = "e6e6e6;bold"

## test styles ##

# prompt box border
prompt_border = "505078"
# border type
border_type = "rounded"

# correctly typed words
prompt_correct = "64c864"
# incorrectly typed words
prompt_incorrect = "e65050"
# untyped words
prompt_untyped = "5a5a5a"

# correctly typed letters in current word
prompt_current_correct = "78e678;bold"
# incorrectly typed letters in current word
prompt_current_incorrect = "ff6450;bold"
# untyped letters in current word
prompt_current_untyped = "c8c8dc;bold"

# cursor character
prompt_cursor = "none;reversed;bold"
# skipped non-typeable characters (--ascii flag)
prompt_skipped = "c8b43c"

## status bar styles ##

# live WPM counter
status_wpm = "64c864;bold"
# elapsed time
status_timer = "b4b4c8"
# word progress counter
status_progress = "b4b4c8"
# progress bar filled portion
status_progress_filled = "64c864"
# progress bar empty portion
status_progress_empty = "323232"

## results styles ##

# overview text
results_overview = "64c864;bold"
# overview border
results_overview_border = "505078"

# worst keys text
results_worst_keys = "dcb43c;bold"
# worst keys border
results_worst_keys_border = "505078"

# missed words text
results_missed_words = "e65050;bold"
# missed words border
results_missed_words_border = "505078"

# results chart line
results_chart = "50b4dc"
# mistake markers on chart
results_chart_mistakes = "e65050"
# results chart x-axis label
results_chart_x = "6e6e6e"
# results chart y-axis label
results_chart_y = "6e6e6e;bold"

# restart/quit prompt
results_restart_prompt = "b4b4c8;bold"
```

### style format

Styles are encoded as a string. Start with the color specification: a single color (foreground), or two colors separated by a colon (foreground and background). Colors can be a terminal color name, a 6-digit hex color code, `none`, or `reset`.

After the colors, optionally specify modifiers separated by semicolons:

`bold`, `crossed_out`, `dim`, `hidden`, `italic`, `rapid_blink`, `slow_blink`, `reversed`, `underlined`

Examples:

- `blue:white;italic` -- italic blue text on a white background
- `none;bold;underlined` -- bold underlined text with no set color
- `00ff00:000000` -- green text on a black background

### border types

`plain`, `rounded` (default), `double`, `thick`, `quadrantinside`, `quadrantoutside`
