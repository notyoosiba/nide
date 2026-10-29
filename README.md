# nide — Terminal Workspace with CLI Agent Integration

A fast, terminal-native, pure-Rust 3-pane code workspace designed for working alongside CLI coding agents.

## Architecture

- **Rendered Document Preview & Mermaid Diagrams (`p`):** In `MiddleMode::Normal`, pressing `p` toggles a rendered document preview for Markdown (`.md`), CSV, TSV, and JSONL files. Renders Markdown headings, tables (with unicode box-drawing borders `┌─┬─┐`, `│ │ │`, `├─┼─┤`, `└─┴─┘`), tasklists (`☐`/`☑`), code fences, and Mermaid diagrams (rasterized to text/PNG via `merman`). CSV, TSV, and JSONL files are parsed into aligned tables with column-width formatting. Every rendered line is mapped back to its 0-indexed source line with gutter numbers and cursor indicator (`▶`); pressing `Enter` or `i` on any rendered line immediately enters `MiddleMode::Edit` at that exact source line.
- **Single Pure-Rust Crate:** Asynchronous-free synchronous `mpsc` event loop in `src/main.rs`.
- **Configuration & Workspace Ergonomics (`~/.config/nide/config.toml`):** Optional TOML configuration (`config.rs`) allowing custom `autosave_delay_ms` (default: 1000ms) and initial pane `widths` weights (e.g. `[20, 50, 30]`).
- **Debounced Autosave:** Background debounced autosave timer in `app.tick()` automatically saves dirty buffers atomically to disk after the debounce delay while remaining seamlessly in `MiddleMode::Edit`.
- **Mouse Drag-Border Resizing:** Mouse dragging on vertical pane borders dynamically adjusts pane widths (clamped to a minimum of 15 columns per visible pane) and sends PTY resize updates to embedded terminals; top-border dragging on Quake console resizes the dropdown overlay.
- **Project Tree File CRUD & Clipboard:** Interactive file/folder creation (`a`/`Ctrl+N`), inline rename (`r`) via `LineEdit` prompt, copy (`y`) / cut (`x`) and paste (`p`) into target directories, and immediate deletion (`Delete`/`Backspace`) refreshing the project tree and open buffers.
- **3-Pane Horizontal Split:**
  - **Left (20%):** Project tree view with live Git status marks (`git status --porcelain -uall` coloring modified `M` yellow, added/untracked `A`/`?` green, and deleted `D` red, rolled up to ancestor folders) and dimmed `.gitignore`d entries with lazy expansion
  - **Middle (50%):** Code editor & buffer viewer
  - **Right (30%):** Embedded agent terminal
  - **Bottom (1 line):** Status bar
- **Hybrid Syntax Highlighting & Binary Circuit Breakers:** Layered syntax highlighting combining checkpointed `syntect` (`two-face`) baseline coloring across 200+ file types with incremental AST-based **Tree-sitter** highlighting for core languages (Rust, JS/TS, Python, Go, JSON, TOML, Bash, C/C++, HTML, CSS) using crate-bundled `HIGHLIGHTS_QUERY` constants and time-sliced parsing (`500µs` active / `3ms` idle). Features automatic 8KB NUL-byte sniffing (opening non-text files read-only with a binary banner) and large-file circuit breakers (`MAX_HL_LINES = 30_000`, `MAX_HL_BYTES = 4MB`, and 20,000 bytes per line limit). Markdown (`.md`) files open with raw syntax highlighting.
- **Inline Git Diff, Phantom Ghost Lines & Myers Intraline Diff:** Background `git diff -U0` (with `diff_seq` deduping) tracks modifications against `HEAD`. The editor gutter displays green (added), blue (changed), and red (deleted below) indicator bars (`▎`), interleaves phantom deleted "ghost" lines (`-`) above their anchor lines with Myers word-level intraline token highlighting, and enables fast hunk stepping with `c` (next hunk) and `C` (previous hunk) in Normal mode.
- **Live Sync & Clean Buffer Auto-Reload:** Background `notify` filesystem watcher with debounce (200ms quiet / 1s max, ignoring `.nide-save-*` temp files) automatically refreshes the Left Tree (preserving expanded directories) and silently reloads clean Middle Buffers when modified on disk by external CLI agents.
- **Base-Aware 3-Way Diff & Interactive Conflict Resolution:** When an external agent modifies an open dirty buffer on disk, `nide` performs a 3-way diff against `base_lines`. Non-overlapping edits are merged automatically without leaving `Edit` mode. Conflicting edits enter `MiddleMode::Conflict` with inline Left (Yours) and Right (Agent) blocks, stepping (`c`/`C`), and keybindings to accept Yours (`l`), Agent (`r`), Both (`b`), or All (`L`/`R`). File switching and quitting are blocked until all conflicts are resolved and saved atomically.
- **In-Buffer Incremental Smart-Case Search:** Incremental live regex search (`/` in Normal mode or `Ctrl+F` in Edit/Normal modes) with automatic smart-case sensitivity, literal fallback on invalid regex syntax, live match highlighting in Catppuccin Latte Peach (active match) and Yellow (other matches), cycling with `n` (next) and `N` (previous) with wraparound, and pre-search cursor anchor restoration with `Esc`.
- **Fuzzy File Picker Modal (`nucleo`):** Fast modal file picker (`o` in Tree or Editor Normal mode, or `Ctrl+E` globally) backed by `nucleo`, featuring matched-character highlights, recent-file score boosting (ranking MRU visited files first on empty query), and a live syntax-highlighted preview pane. Enter auto-saves dirty buffers and opens the selection in Edit mode.
- **Fuzzy Symbol Search (`D` / `Ctrl+T`):** Project-wide code intelligence modal extracting function, struct, enum, trait, impl, class, def, interface, type, and constant definitions across Rust, Python, and other languages using Tree-sitter and regex extractors. Filters via `nucleo` fuzzy matching; Enter jumps to the exact file and line in Edit mode with target line centering in preview.
- **Go-to-Definition (`d`), Find Usages (`u`) & Jump History (`[` / `]`):** Static code navigation in Normal mode: pressing `d` jumps directly to the definition of the identifier under cursor (or opens the Picker if ambiguous), checking local buffer bindings first then ranking project symbols (imported/same-directory files first); `u` scans project files for word-boundary occurrences of the identifier (filtering out comment lines) and displays call sites in the Picker; and `[` / `]` traverses backward and forward through the jump history stack.
- **Live Background Project Grep (`s`):** Fast ripgrep-style project search streaming matches from a background `grep-searcher` worker with smart-case regex matching, match highlighting, and `seq: u64` deduplication to reliably drop stale in-flight results when typing. Enter jumps to the matched file and line.
- **Native Multi-Protocol Graphics & SVG Viewer:** Raster images (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF via `image` crate) and SVGs (`resvg`) open in the Middle Pane in `MiddleMode::View`, safely querying terminal pixel dimensions via `crossterm::terminal::window_size()`. Auto-detects and drives terminal graphics with pure-Rust fallbacks: Kitty Graphics Protocol -> iTerm2 OSC 1337 inline images -> Sixel bitmap graphics -> built-in Unicode truecolor half-blocks (`▀`/`▄`). Override via `NIDE_GRAPHICS=kitty|iterm2|sixel|halfblocks`. Background worker decoding with `media_seq` deduplication drops stale in-flight results. An automatic collision erase guard suppresses and erases graphics whenever Quake, Help, or Picker overlays intersect the Middle Pane.
- **Syntax-Aware Code Folding (`f`):** Pressing `f` in `MiddleMode::Normal` folds/unfolds the enclosing or selected function, struct, or block at the cursor line behind a `⋯` indicator. Supports Tree-sitter syntax nodes with brace-matching and indentation fallbacks. Folded lines are skipped automatically during `j`/`k` vertical navigation and omitted from display.
- **Floating Quake Console Overlay & Help Modal:** Floating dropdown PTY overlay (`Alt+\`` or `Ctrl+\``) rendered over the bottom 35% of the workspace using Ratatui `Clear` without shrinking the Right Agent terminal or triggering `SIGWINCH`. Lazily spawns `TermId::Quake` with full mouse selection, scrollback, and `Ctrl+Click` / `Cmd+Click` link resolution into the Middle Editor. A centered Help modal (`?` in Tree or Normal mode) displays an instant keybinding reference table.

## CLI Usage

```bash
# Launch in the current directory
nide

# Launch in a specific workspace directory
nide /path/to/project
```

## Keybindings & Controls

### Global Controls

| Action | Control |
| :--- | :--- |
| **Toggle Left Tree Pane** | `Shift+1` (in Tree/Normal/View modes) or `Alt+1` (macOS Option override: `¡`, fallback: `Ctrl+1`) |
| **Toggle Middle Editor Pane** | `Shift+2` (in Tree/Normal/View modes) or `Alt+2` (macOS Option override: `™` / `€`, fallback: `Ctrl+2`) |
| **Toggle Right Terminal Pane** | `Shift+3` (in Tree/Normal/View modes) or `Alt+3` (macOS Option override: `£`, fallback: `Ctrl+3`) |
| **Toggle Quake Console Overlay** | `~` (Shift+` in Tree/Normal/View modes) or `Alt+\`` (fallback: `Ctrl+\`` / `F12`) |
| **Toggle Help Overlay Modal** | `?` (in Tree or Editor Normal mode; closes on `?`, `Esc`, or `q`) |
| **Cycle Focus** | `Tab` (`Tree` -> `Editor` -> `Terminal`; inserts indent in `Edit` mode, forwarded to PTY in `Terminal`) |
| **Fuzzy File Picker** | `Ctrl+E` (globally opens `nucleo` fuzzy file picker modal) |
| **Fuzzy Symbol Search** | `Ctrl+T` (globally opens symbol search modal) |
| **Focus Escape / Cycle** | `Alt+Tab` or `Ctrl+\` (cycles across visible panes from any mode, including `Edit` mode and `Terminal`) |
| **Direct Focus** | Left mouse click inside any pane |
| **Border Drag Resize** | Left click & drag on vertical pane borders to resize panes (min 15 cols); drag Quake top border to resize dropdown |
| **Quit** | `q` (in `Tree` or `Editor` Normal mode, auto-saving dirty buffers), or `Ctrl+C` |

### Left Pane (Project Tree)

| Action | Control |
| :--- | :--- |
| **Move Selection Down** | `j` or `Down` |
| **Move Selection Up** | `k` or `Up` |
| **Create File / Directory** | `a` or `Ctrl+N` (inline `LineEdit` prompt pre-filled with directory prefix; trailing `/` creates directory, otherwise creates file and opens in `Edit` mode) |
| **Rename File / Directory** | `r` (inline `LineEdit` prompt pre-filled with current path to rename selected node) |
| **Copy / Cut / Paste Node** | `y` (copy) / `x` (cut) on selected node, navigate to target directory, and `p` to paste |
| **Fuzzy File Picker** | `o` (opens `nucleo` fuzzy file picker modal) |
| **Fuzzy Symbol Search** | `D` (opens fuzzy symbol search modal) |
| **Live Project Grep** | `s` (opens background `grep-searcher` project search modal) |
| **Delete File / Directory** | `Delete` or `Backspace` (immediately deletes selected file or folder, updates tree index, and clears open buffer if affected) |
| **Open File / Toggle Dir** | `Enter` (opens file in Editor `Edit` mode; expands/collapses directory, lazily loading ignored directories) |
| **Toggle Expand / Collapse Dir** | `Right` or `l` |
| **Collapse Directory / Parent Jump** | `Left` or `h` |
| **Select Row** | Left mouse click on tree row |
| **Git Status Marks & Ignored Styling** | Modified (`M`) yellow, added/untracked (`A`/`?`) green, deleted (`D`) red, rolled up to ancestor folders; `.gitignore`d nodes rendered dimmed (`OVERLAY0`) |

### Middle Pane (Modal Code Editor)

| Mode / Action | Control |
| :--- | :--- |
| **Undo / Redo** | `Ctrl+Z` (undo) / `Ctrl+Y` (redo), restoring exact buffer content and cursor positions |
| **Visual Selection** | `Shift+Arrows` (in Edit and Normal modes), `v` (in Normal mode), or mouse click & drag |
| **Copy to Clipboard** | `Ctrl+C` (copies active selection or current line via OSC 52 + `tmux`), or `y` in Normal visual mode |
| **Cut to Clipboard** | `Ctrl+X` (cuts active selection or current line via OSC 52 + `tmux`), or `d`/`x` in Normal visual mode |
| **Paste from Clipboard** | `Msg::Paste` / bracketed paste / `Ctrl+V` (inserts text at cursor, replacing selection if active) |
| **Soft Line Wrapping** | Clean word/character wrapping for lines exceeding middle pane width, with continuation markers (`·`) in gutter |
| **Edit Mode: Insert text** | Normal character keys (replaces active selection if present) |
| **Edit Mode: Newline** | `Enter` |
| **Edit Mode: Backspace / Delete** | `Backspace` / `Delete` (deletes selection if active) |
| **Edit Mode: Indent** | `Tab` (inserts 4 spaces) |
| **Edit / Normal: Find** | `Ctrl+F` (or `/` in Normal mode) enters `MiddleMode::Find` with live smart-case regex matching |
| **Find Mode: Confirm / Cancel** | `Enter` (confirms current match and drops to Normal mode) / `Esc` (restores pre-search cursor anchor and drops to Normal mode) |
| **Normal Mode: Step Matches** | `n` (step to next match with wraparound) / `N` (step to previous match with wraparound) |
| **Edit Mode: Cursor Navigation** | Arrows, `Home`, `End`, `PageUp`, `PageDown` (clears range selection on unshifted move) |
| **Normal Mode: Fuzzy File Picker** | `o` (opens `nucleo` fuzzy file picker modal) |
| **Normal Mode: Fuzzy Symbol Search** | `D` (opens symbol search modal, jumping to definition line on `Enter`) |
| **Normal Mode: Go-to-Definition** | `d` (jumps directly to definition of identifier under cursor, or opens Picker if ambiguous) |
| **Normal Mode: Find Usages** | `u` (scans project for word-boundary occurrences excluding comments and opens Picker modal) |
| **Normal Mode: Jump History Back / Forward** | `[` (jump back) / `]` (jump forward) through the jump history stack |
| **Normal Mode: Live Project Grep** | `s` (opens background project grep modal, jumping to match on `Enter`) |
| **Edit Mode: Atomic Save & Drop to Normal** | `Esc` (clears selection, saves atomically via `.nide-save-<pid>`, and enters `Normal` mode) |
| **Normal Mode: Enter Edit Mode** | `i` or `Enter` |
| **Normal Mode: Cursor Navigation** | `h`/`j`/`k`/`l`, Arrows, `0`/`Home`, `$` / `End`, `g` (BOF) / `G` (EOF), `PageUp`, `PageDown` |
| **Normal Mode: Clear Selection / Tree Focus** | `Esc` (clears selection if active, otherwise returns focus to Tree) |
| **Normal Mode: Next / Previous Git Hunk** | `c` (jump to next hunk) / `C` (jump to previous hunk, Shift+C) |
| **Normal Mode: Toggle Code Folding** | `f` (folds/unfolds the function, struct, or block at cursor line behind a `⋯` indicator; skips folded lines on `j`/`k`) |
| **Normal Mode: Toggle Document Preview** | `p` (toggles rendered document preview for `.md`, `.csv`, `.tsv`, `.jsonl` files) |
| **Preview Mode: Jump to Source in Edit Mode** | `Enter` or `i` (jumps to the mapped source line in `MiddleMode::Edit` with focused cursor) |
| **Preview Mode: Navigate Preview Rows** | `j`/`k`, `Up`/`Down`, `g`/`G`, `Home`/`End`, `PageUp`/`PageDown` (keeps source line synced) |
| **Preview Mode: Exit Preview** | `p` or `Esc` (returns to Normal raw text view) |
| **View Mode: Image & SVG Preview** | Raster images and SVGs open in `MiddleMode::View` displaying metadata header (dimensions, format, file size, cell pixel size, placement bounds, graphics driver) and Kitty/iTerm2/Sixel/Unicode half-block graphics; `?` opens Help overlay and `q` quits |
| **Inline Git Diff Gutter & Ghost Lines** | Green/blue/red gutter indicator bars (`▎`), phantom deleted ghost lines (`-`) with Myers intraline token highlighting |
| **Conflict Mode: Accept Yours (Left)** | `l` (accepts user's in-memory edit for current hunk) |
| **Conflict Mode: Accept Agent (Right)** | `r` (accepts external agent's on-disk edit for current hunk) |
| **Conflict Mode: Keep Both** | `b` (keeps user's edit followed by agent's edit) |
| **Conflict Mode: Accept All Yours / Agent** | `L` (all Yours) / `R` (all Agent) |
| **Conflict Mode: Step Hunks** | `c` (next conflict hunk) / `C` (previous conflict hunk) |
| **Conflict Mode: Blocked Actions** | Quitting (`q`) and switching files are blocked with status warnings until all conflicts are resolved |
| **Normal Mode: Quit** | `q` (auto-saves dirty buffer before quitting) |
| **Position Cursor & Drag Selection** | Left mouse click to position cursor; click & drag inside editor to select text |
| **Mouse Wheel Viewport Scroll** | Scroll down/up with mouse wheel freely navigates file viewport without moving cursor; navigation keys snap viewport back to cursor |
| **Auto-Save** | Switching files or quitting automatically saves dirty buffers atomically |

### Right Pane (Embedded Agent Terminal)

| Action | Control |
| :--- | :--- |
| **Terminal Input** | Keystrokes (`Tab`, `Esc`, arrows, etc.) forwarded directly to embedded PTY shell |
| **TUI Mouse Wheel & Scrollback** | When agent TUI enables mouse reporting (`MouseProtocolMode != None`), `ScrollUp`/`ScrollDown` forwards SGR/Default/UTF-8 mouse sequences (`\x1b[<64;...M` / `\x1b[<65;...M`); in alternate screen mode (`alternate_screen == true`), forwards cursor Up/Down escape sequences (`\x1bOA`/`\x1bOB` or `\x1b[A`/`\x1b[B`); otherwise (or when `Shift` is held to bypass TUI mouse capture), adjusts host scrollback buffer without underflowing at deep scroll depths |
| **Scrollback Navigation** | Mouse wheel (`ScrollUp`/`ScrollDown` or `Shift+Scroll`) navigates terminal history; renders `[+N lines]` badge in border; any key event resets scrollback to 0 |
| **Mouse Cell Selection** | Left mouse click & drag inside terminal selects cells, highlighted with Catppuccin Latte selection background |
| **Smart Copy / SIGINT** | `Ctrl+C` or `Cmd+C` copies active terminal selection to clipboard via OSC 52 + `tmux` and clears selection; sends `SIGINT` (`\x03`) to PTY when no selection is active |
| **Paste & Bracketed Paste** | `Ctrl+V`, `Cmd+V`, `Ctrl+P`, or `Msg::Paste` pastes clipboard text into terminal, automatically wrapping in `\x1b[200~...\x1b[201~` when bracketed paste is enabled |
| **Escape Focus** | `Alt+Tab` or `Ctrl+\` (cycles focus to the next visible pane) |
| **Open File Link** | `Ctrl+Click` / `Cmd+Click` on filename or `path:line:col` (resolves against project, reveals in Tree, opens in Editor in `Edit` mode at line:col) |
| **Respawn Exited Shell** | `Enter` (when `[Process exited — press Enter to respawn]` is displayed) |
| **Direct Focus** | Left mouse click inside terminal pane |

### Floating Quake Console Overlay & Help Modal

| Action | Control |
| :--- | :--- |
| **Toggle Quake Console** | `Alt+\`` or `Ctrl+\`` (lazily spawns shell on first open, preserves background process when hidden, and respawns fresh on PTY exit) |
| **Quake Open File Link** | `Ctrl+Click` / `Cmd+Click` on `path:line:col` inside Quake console (opens file in Middle editor at line:col and reveals in Tree) |
| **Quake Mouse Selection & Copy** | Left click & drag selects cells; `Ctrl+C` / `Cmd+C` copies selection (or sends `SIGINT` if none) |
| **Quake TUI Scroll & Scrollback** | Mouse wheel forwards mouse events / alternate-scroll to child TUI; `Shift+Scroll` bypasses to host scrollback with `[+N lines]` badge |
| **Toggle Help Overlay** | `?` (in Tree or Normal mode; centered keybinding reference table) |
| **Close Help Overlay** | `?`, `Esc`, or `q` |

## Automated Verification

```bash
cargo check --all-targets
cargo test
cargo clippy --all-targets -- -D warnings
```
