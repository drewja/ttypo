```
┏┌────┓                                                     ▄   ▄                                        
││Esc │                                                    ▀█▀ ▀█▀ █ █ █▀█ █▀█                           
│/────┤                                                     █▄  █▄ █▄█ █▄█ █▄█                           
┗─────┛                                                            ▄▄█ █ v0.1.19                         
┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┐┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏───────────┐┓
││ `~ │││ 1! │││ 2@ │││ 3# │││ 4$ │││ 5% │││ 6^ ││ 7& │││ 8* │││ 9( │││ 0) │││ -_ │││ =+ │││ Backspace ││
│/────┤│/────┤│/────┤│/────┤│/────┤│/────┤│/────││────\││────\││────\││────\││────\││────\││───────────\│
┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗────────────┛
┏┌────────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏───────┐┓
││  Tab   │││ Q  │││ W  │││ E  │││ R  │││ T  ││ Y  │││ U  │││ I  │││ O  │││ P  │││ [{ │││ ]} │││     \ ││
│/────────┤│/────┤│/────┤│/────┤│/────┤│/────┤├────\│├────\│├────\│├────\│├────\│├────\│├────\│├───────\│
┗─────────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗────────┛
┏┌──────────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────────────┐┓
││  Caps    │││ A  │││ S  │││ D  │││ F  │││ G  ││ H  │││ J  │││ K  │││ L  │││ ;: │││ '" │││ Enter      ││
│/──────────┤│/────┤│/────┤│/────┤│/────┤│/────┤├────\│├────\│├────\│├────\│├────\│├────\│├────────────\│
┗───────────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────────────┛
┏┌──────────────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏┌────┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏────┐┓┏───────────────┐┓
││  Shift       │││ Z  │││ X  │││ C  │││ V  │││ B  ││ N  │││ M  │││ ,< │││ .> │││ /? │││ Shift         ││
│/──────────────┤│/────┤│/────┤│/────┤│/────┤│/────┤├────\│├────\│├────\│├────\│├────\│├───────────────\│
┗───────────────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗─────┛┗────────────────┛
┏┌──────┓┏┌──────┓┏┌──────┓┏┌──────────────────────────────────────┐┓┏──────┐┓┏──────┐┓┏──────┐┓┏──────┐┓
││ Ctrl │││  Fn  │││ Alt  │││                 Space                │││ Alt  │││ Fn   │││ Menu │││ Ctrl ││
│/──────┤│/──────┤│/──────┤│/──────────────────────────────────────\│├──────\│├──────\│├──────\│├──────\│
┗───────┛┗───────┛┗───────┛┗────────────────────────────────────────┛┗───────┛┗───────┛┗───────┛┗───────┛
```

ttypo is a touch-type training program.
Practice, improve, and measure your touch typing skills while reading and learning about anything you like!

Read a book or other document while simultaneously improving your typing speed and accuracy.

This project is under active development with new features coming soon!
Please report bugs and/or feature requests to the github issue tracker.
Pull requests are welcomed and encouraged.

```bash
ttypo --ascii The.Great.Gatsby.txt
```
Books in the public domain can be found on project [gutenberg](https://www.gutenberg.org)
<!-- ![Recording](./resources/recording.gif) -->

## installation

```bash
cargo install ttypo
```

## web version

A browser build is hosted at [drewja.github.io/ttypo](https://drewja.github.io/ttypo/).
Language mode only (no file/stdin input, no saved progress).
Use your browser's zoom (Ctrl/Cmd +) to enlarge the terminal grid.

To build it locally:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve  # http://localhost:8080
```

## cli usage

```
ttypo [OPTIONS] [PATH] [COMMAND]

Commands:
  completions  Generate shell completions
  help         Print this message or the help of the given subcommand(s)

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
      --restart               Ignore saved progress for the input file and start from the beginning
      --no-save               Do not persist per-document progress for this invocation
  -h, --help                  Print help
  -V, --version               Print version
```

### examples

| command                                  |                                          test contents |
| :--------------------------------------- | -----------------------------------------------------: |
| `ttypo`                                  |                                       interactive menu |
| `ttypo text.txt`                         |                     practice on contents of `text.txt` |
| `ttypo --ascii source.rs`                |      type a source file, skipping non-ASCII characters |
| `man ls \| ttypo --ascii -`              |                  practice typing a man page from stdin |

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

Additional languages can be added by creating a file in the config language directory with a word on each line. On Linux, the config directory is `$HOME/.config/ttypo/language`; on macOS it's `$HOME/Library/Application Support/ttypo/language`.

# Look and Feel
You can customize the color and/or style of just about any element in this application via the config.

Configuration is specified by `config.toml` in the config directory (e.g. `$HOME/.config/ttypo/config.toml`).

Default values:

```toml
# the language used when one is not manually specified
default_language = "english200"

[theme]
# default style (includes empty cells)
default = "none"
# title text and key-art labels
title = "e6e6e6;bold"

## test styles ##

# prompt box border
prompt_border = "505078"
# border type: plain, rounded, double, thick, quadrantinside, quadrantoutside
border_type = "rounded"

# correctly typed words
prompt_correct = "647864"
# incorrectly typed words
prompt_incorrect = "c85050"
# untyped words
prompt_untyped = "c8c8c8"

# correctly typed letters in current word
prompt_current_correct = "78b478;bold"
# incorrectly typed letters in current word
prompt_current_incorrect = "c86450;bold"
# untyped letters in current word
prompt_current_untyped = "c8c8dc;bold"

# cursor character
prompt_cursor = "none;reversed;bold"
# skipped non-typeable characters (--ascii flag)
prompt_skipped = "c8b43c"

## status bar styles (during test) ##

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

# overview box border
results_overview_border = "505078"
# worst-keys box border
results_worst_keys_border = "505078"
# missed-words box border
results_missed_words_border = "505078"

# WPM line on the chart
results_chart = "50b4dc"
# mistake markers on the chart
results_chart_mistakes = "e65050"
# y-axis title on the chart
results_chart_y = "6e6e6e;bold"

# control hints under the chart (text-fallback styling and key descriptions)
results_restart_prompt = "b4b4c8;bold"

## resume prompt styles ##

# "File changed" warning title and inline warning text
resume_prompt_warning = "e6b450;bold"
# emphasized labels (source path, [Y]/[N]/[D] buttons)
resume_prompt_emphasis = "none;bold"
```

### text style format
`foreground-color:background-color;modifiers`

Colors can be a terminal color name, 6-digit hex, or none.
Modifiers must be separated by semicolons.

`bold`, `crossed_out`, `dim`, `hidden`, `italic`, `rapid_blink`, `slow_blink`, `reversed`, `underlined`


Examples:

- `blue:white;italic` -- italic blue text on a white background
- `none;bold;underlined` -- bold underlined text with no set color
- `00ff00:000000` -- green text on a black background

### border types
`plain`, `rounded` (default), `double`, `thick`, `quadrantinside`, `quadrantoutside`
