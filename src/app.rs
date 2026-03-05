use std::{path::PathBuf, sync::Arc, time::Instant};

use egui::Context;

use crate::{
    indexer::MmapFile,
    tab::TabState,
    theme::{apply_theme, AppTheme},
    ui,
};

pub struct AppState {
    pub tabs: Vec<TabState>,
    pub active_tab: usize,
    pub theme: AppTheme,
    last_theme_check: Instant,

    /// Result channel from native file dialog
    file_open_rx: Option<crossbeam_channel::Receiver<Option<PathBuf>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab: 0,
            theme: AppTheme::detect(),
            last_theme_check: Instant::now(),
            file_open_rx: None,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_file_dialog(&mut self) {
        if self.file_open_rx.is_some() {
            return; // dialog already open
        }
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.file_open_rx = Some(rx);

        std::thread::spawn(move || {
            let result = rfd::FileDialog::new()
                .add_filter("Log/Text files", &["log", "txt", "out", "csv", "json", "xml"])
                .add_filter("All files", &["*"])
                .pick_file();
            let _ = tx.send(result);
        });
    }

    fn poll_file_dialog(&mut self) {
        let Some(ref rx) = self.file_open_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(path)) => {
                self.file_open_rx = None;
                self.open_file(path);
            }
            Ok(None) => {
                // User cancelled
                self.file_open_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(_) => {
                self.file_open_rx = None;
            }
        }
    }

    pub fn open_file(&mut self, path: PathBuf) {
        // Don't open duplicates — switch to existing tab
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = i;
            return;
        }

        match MmapFile::open(&path) {
            Ok(mmap) => {
                let mmap = Arc::new(mmap);
                let tab = TabState::new(path, mmap);
                self.tabs.push(tab);
                self.active_tab = self.tabs.len() - 1;
            }
            Err(e) => {
                log::error!("Failed to open {}: {e}", path.display());
                // TODO: surface as toast/dialog
            }
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }

        // Signal background indexer to stop
        if let Some(ref handle) = self.tabs[index].indexer {
            handle.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        self.tabs.remove(index);

        // Adjust active tab
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    fn handle_keyboard(&mut self, ctx: &Context) {
        ctx.input_mut(|i| {
            // Ctrl+O / Cmd+O — open file
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::O) {
                // Can't call self.open_file_dialog() from inside input_mut closure
                // so we use a flag — handled after
            }
            // Ctrl+W — close tab
            if i.consume_key(egui::Modifiers::COMMAND, egui::Key::W) {
                // Handled below via return value
            }
        });

        // Check keyboard shortcuts outside the closure
        let open = ctx.input(|i| i.key_pressed(egui::Key::O) && i.modifiers.command);
        let close = ctx.input(|i| i.key_pressed(egui::Key::W) && i.modifiers.command);
        let next_tab = ctx.input(|i| i.key_pressed(egui::Key::Tab) && i.modifiers.ctrl);
        let prev_tab =
            ctx.input(|i| i.key_pressed(egui::Key::Tab) && i.modifiers.ctrl && i.modifiers.shift);

        if open {
            self.open_file_dialog();
        }
        if close && !self.tabs.is_empty() {
            self.close_tab(self.active_tab);
        }
        if !self.tabs.is_empty() {
            if next_tab {
                self.active_tab = (self.active_tab + 1) % self.tabs.len();
            }
            if prev_tab {
                self.active_tab = if self.active_tab == 0 {
                    self.tabs.len() - 1
                } else {
                    self.active_tab - 1
                };
            }
        }
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Theme detection (throttled to once per second)
        if self.last_theme_check.elapsed().as_secs() >= 1 {
            let new_theme = AppTheme::detect();
            if new_theme != self.theme {
                self.theme = new_theme;
                apply_theme(ctx, self.theme);
            }
            self.last_theme_check = Instant::now();
        }

        // Poll background work
        self.poll_file_dialog();

        let mut any_indexing = false;
        for tab in &mut self.tabs {
            tab.poll_indexer();
            tab.poll_search();
            tab.poll_histogram();
            tab.poll_overview_cache();

            // Trigger debounced search
            if tab.search.should_search() && tab.search.search_handle.is_none() {
                tab.trigger_search();
            }

            if tab.is_indexing() {
                any_indexing = true;
            }
        }

        // Handle keyboard shortcuts
        self.handle_keyboard(ctx);

        // ── Menu bar ─────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open…  ⌘O").clicked() {
                        self.open_file_dialog();
                        ui.close_menu();
                    }
                    if ui.add_enabled(!self.tabs.is_empty(), egui::Button::new("Close Tab  ⌘W")).clicked() {
                        self.close_tab(self.active_tab);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });

        // ── Tab bar ───────────────────────────────────────────────────────────
        let close_request = egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui::tab_bar::render_tab_bar(ui, &mut self.tabs, &mut self.active_tab)
        });
        if let Some(idx) = close_request.inner {
            self.close_tab(idx);
        }

        // ── Search bar (only when a file is open) ─────────────────────────────
        if !self.tabs.is_empty() {
            let trigger = egui::TopBottomPanel::top("search_bar").show(ctx, |ui| {
                let tab = &mut self.tabs[self.active_tab];
                ui::search_bar::render_search_bar(ui, tab)
            });
            if trigger.inner {
                self.tabs[self.active_tab].trigger_search();
            }
        }

        // ── Histogram panel (bottom, toggled) ────────────────────────────────
        let show_histogram = self.tabs.get(self.active_tab).map(|t| t.show_histogram).unwrap_or(false);
        if show_histogram {
            egui::TopBottomPanel::bottom("histogram")
                .resizable(true)
                .default_height(130.0)
                .min_height(60.0)
                .show(ctx, |ui| {
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        ui::histogram::render_histogram(ui, tab);
                    }
                });
        }

        // ── Status bar ────────────────────────────────────────────────────────
        if !self.tabs.is_empty() {
            egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
                ui::status_bar::render_status_bar(ui, &self.tabs[self.active_tab]);
            });
        }

        // ── Overview panel (right, always visible when file open) ─────────────
        if !self.tabs.is_empty() {
            let overview_row = egui::SidePanel::right("overview")
                .default_width(22.0)
                .min_width(22.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let tab = &self.tabs[self.active_tab];
                    ui::overview::render_overview(ui, tab)
                });
            if let Some(row) = overview_row.inner {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.scroll_to_row = Some(row);
                }
            }
        }

        // ── Detail panel ─────────────────────────────────────────────────────
        let detail_open = self.tabs.get(self.active_tab).map(|t| t.detail_open).unwrap_or(false);
        if detail_open {
            egui::SidePanel::right("detail_panel")
                .resizable(true)
                .default_width(400.0)
                .show(ctx, |ui| {
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        ui::detail_panel::render_detail_panel(ui, tab);
                    }
                });
        }

        // ── Central panel: log view ───────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                ui::log_view::render_log_view(ui, tab, ctx);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Open a file with File > Open… or ⌘O");
                });
            }
        });

        // Capture text copied from selectable log labels → offer as a search term
        let copied = ctx.output(|o| o.copied_text.clone());
        if !copied.is_empty() {
            let trimmed = copied.trim().to_string();
            if !trimmed.is_empty() {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    tab.pending_search_addition = Some(trimmed);
                }
            }
        }

        // Request repaint while indexing so progress updates
        if any_indexing {
            ctx.request_repaint();
        }
    }
}
