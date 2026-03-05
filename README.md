# Rehighlighter

A fast, cross-platform log viewer built with [egui](https://github.com/emilk/egui). Open large log files instantly, search with multiple simultaneous highlighted terms, filter lines, and explore activity over time with a timestamp histogram.

![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)
![License](https://img.shields.io/badge/license-MIT-blue)

---

## Features

### Multi-term Search & Highlighting
- Type a search term and press **Enter** to commit it as a colored chip
- Add as many terms as you like — each gets its own highlight color (yellow, cyan, lime, orange, purple)
- Terms are matched with **OR** logic: a line matches if any term hits
- Toggle **regex** mode and **case-sensitive** matching per session
- **Filter mode**: show only matching lines (with optional context lines above/below)

### Right-click Context Menu
Right-click any word in the log view to open an editable mini-search:
- **🔍 Search** — adds the term as a highlighted search chip
- **⊘ Exclude** — hides all lines containing that term (shown as a dark-red chip)
- Edit the pre-filled text field to extend the selection to a multi-word phrase before confirming

### Text Selection → Search
- **Double-click** a token to instantly add it as a search term
- Select text and press **⌘C** — a `+` button appears in the search bar to add the copied text as a term

### Exclusion Filters
- Right-click → Exclude hides matching lines across the entire file
- Excluded terms appear as **⊘ chips** in the search bar — click × to remove
- Works in combination with filter mode and search terms

### Overview Panel
- A minimap on the right side shows the full file at a glance
- **Narrow mode** (< 60 px): tick marks for each matching line, colored by term
- **Wide mode** (≥ 60 px): line-length bars with match position highlighted in the term's color

### Timestamp Histogram
- Automatically detects ISO 8601, syslog, and Apache timestamp formats
- Draws a bar chart of log activity over time; match counts overlaid in yellow
- **Bin granularity selector**: Auto, 1 min, 5 min, 15 min, 1 hr, 6 hr, 1 day, 1 wk, 1 mo, 1 yr
- Click a bar to jump to that time range in the log view

### Multiple Tabs
- Open multiple log files simultaneously, each in its own tab
- Drag-and-drop files onto the window to open them

### Performance
- Memory-mapped file I/O — opens multi-GB files instantly
- Background indexing, search, and histogram computation (never blocks the UI)
- Virtual scrolling — only renders visible rows regardless of file size

---

## Installation

### Prerequisites
- [Rust](https://rustup.rs/) 1.75 or later

### Build from source
```bash
git clone https://github.com/YOUR_USERNAME/Rehighlighter.git
cd Rehighlighter
cargo build --release
./target/release/rehighlighter
```

### macOS / Linux
No additional system dependencies required. On Linux, egui uses the system's OpenGL driver.

---

## Usage

```bash
# Open with no arguments (use File → Open or drag-and-drop)
./rehighlighter

# Open a file directly
./rehighlighter /var/log/syslog
```

### Keyboard shortcuts

| Action | Shortcut |
|--------|----------|
| Open file | ⌘O |
| Next match | ↓ / F3 |
| Previous match | ↑ / Shift+F3 |
| Add selection as search term | ⌘C (then click + in search bar) |
| Toggle filter mode | button in search bar |
| Toggle histogram | button in toolbar |

---

## Architecture

```
src/
├── main.rs              # Entry point, eframe setup
├── app.rs               # Top-level App state, event loop
├── tab.rs               # Per-file TabState (index, search, histogram)
├── indexer/
│   ├── mod.rs           # Background indexer (spawns thread)
│   ├── line_index.rs    # Byte-offset line index
│   └── mmap.rs          # Memory-mapped file wrapper
├── search/
│   ├── mod.rs           # SearchState, background search worker
│   └── filter.rs        # Filter-mode visible-line computation
├── timestamp.rs         # Timestamp detection, histogram bucketing
├── overview_cache.rs    # Background line-length cache for wide overview
└── ui/
    ├── mod.rs           # TERM_COLORS palette, shared constants
    ├── log_view.rs      # Virtual-scroll table, highlighting, context menu
    ├── overview.rs      # Narrow/wide overview minimap
    ├── histogram.rs     # Timestamp histogram panel
    ├── search_bar.rs    # Search input, term chips, exclusion chips
    ├── tab_bar.rs       # Tab strip
    ├── status_bar.rs    # Match count, file info
    └── detail_panel.rs  # Full-line detail side panel
```

---

## License

MIT — see [LICENSE](LICENSE) for details.
