//! Combine files: the files to combine, in order, each with the pages to take (all, or a range
//! such as "1-3, 6"); reorder, remove or add files, then Combine.

use std::sync::Arc;

use egui::{Align, Layout};
use pdfcraft_engine::SourceKind;

use crate::theme::{self, Tokens};
use crate::{Dialog, PdfCraftApp, icons, widgets};

#[derive(Clone, Debug)]
pub struct CombineFile {
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub pages: usize,
    /// The pages to take ("" = all).
    pub range: String,
    /// What the file was before Create converted it (`bytes` is always a PDF).
    pub kind: SourceKind,
}

enum RowAction {
    Up(usize),
    Down(usize),
    Remove(usize),
}

/// The files, in order, each with move up, move down and remove; `ranges` adds the Pages column.
pub(crate) fn file_list(ui: &mut egui::Ui, files: &mut Vec<CombineFile>, t: &Tokens, id: &str, ranges: bool) {
    let mut action = None;
    let n = files.len();
    egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new(id).num_columns(4).min_col_width(40.0).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
            ui.label(egui::RichText::new(tl!("File")).color(t.text_muted));
            ui.label(egui::RichText::new(if ranges { tl!("Pages") } else { "" }).color(t.text_muted));
            ui.label("");
            ui.label("");
            ui.end_row();
            for (i, f) in files.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(icons::image(if f.kind == SourceKind::Image { "image" } else { "file-text" }, 16.0, t.icon));
                    ui.add(egui::Label::new(&f.name).truncate());
                    let count =
                        if f.pages == 1 { tl!("1 page").to_string() } else { crate::i18n::fmt(tl!("{n} pages"), &[("n", &f.pages.to_string())]) };
                    ui.label(egui::RichText::new(count).small().color(t.text_faint));
                });
                if ranges {
                    ui.add_sized([120.0, 22.0], egui::TextEdit::singleline(&mut f.range).hint_text(tl!("All pages")))
                        .on_hover_text(crate::i18n::fmt(tl!("Pages of {name} to combine, e.g. 1-3, 6"), &[("name", &f.name)]));
                } else {
                    ui.label("");
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled_ui(i > 0, |ui| icons::button(ui, "chevron-up", 24.0, false, tl!("Move up"))).inner.clicked() {
                        action = Some(RowAction::Up(i));
                    }
                    if ui.add_enabled_ui(i + 1 < n, |ui| icons::button(ui, "chevron-down", 24.0, false, tl!("Move down"))).inner.clicked() {
                        action = Some(RowAction::Down(i));
                    }
                });
                if icons::button(ui, "trash-2", 24.0, false, &crate::i18n::fmt(tl!("Remove {name}"), &[("name", &f.name)])).clicked() {
                    action = Some(RowAction::Remove(i));
                }
                ui.end_row();
            }
        });
    });
    // The buttons are only enabled where the neighbour exists.
    match action {
        Some(RowAction::Up(i)) if i > 0 && i < n => files.swap(i, i - 1),
        Some(RowAction::Down(i)) if i + 1 < n => files.swap(i, i + 1),
        Some(RowAction::Remove(i)) if i < n => {
            files.remove(i);
        }
        _ => {}
    }
}

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> (bool, bool) {
    ui.label(egui::RichText::new(tl!("Combine files")).font(theme::semibold(18.0)));
    ui.add_space(4.0);
    ui.label(egui::RichText::new(tl!("Files are combined in this order. Leave Pages empty to take every page.")).small().color(t.text_faint));
    ui.add_space(8.0);
    file_list(ui, &mut app.combine_draft, t, "combine-files", true);
    ui.add_space(10.0);
    let (mut go, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        if widgets::pill_button(ui, tl!("Add files…"), false).clicked() {
            app.combine_dialog();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ok = app.combine_draft.len() >= 2;
            if ui
                .add_enabled_ui(ok, |ui| widgets::pill_button(ui, tl!("Combine"), true))
                .inner
                .on_disabled_hover_text(tl!("Add at least two files"))
                .clicked()
            {
                go = true;
            }
            if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                cancel = true;
            }
        });
    });
    (go, cancel)
}

impl PdfCraftApp {
    /// Add picked files to the Combine files list (and show it).
    pub(crate) fn stage_combine(&mut self, files: Vec<(String, Vec<u8>)>) {
        for (name, bytes) in files {
            let bytes = Arc::new(bytes);
            match pdfcraft_render::inspect(bytes.clone(), None) {
                Ok(info) => {
                    self.combine_draft.push(CombineFile { name, bytes, pages: info.pages.len(), range: String::new(), kind: SourceKind::Pdf })
                }
                Err(e) => self.notify_fmt("Couldn't add {name}: {e}", &[("name", &name), ("e", &e.to_string())]),
            }
        }
        self.dialog = Some(Dialog::Combine);
    }

    /// Combine the listed files into a new, unsaved document.
    pub fn combine_staged(&mut self) {
        let files = std::mem::take(&mut self.combine_draft);
        if files.is_empty() {
            return;
        }
        let count = files.len();
        let sources: Vec<(String, Arc<Vec<u8>>, Option<String>)> = files
            .into_iter()
            .map(|f| (crate::files::strip_pdf(&f.name).to_string(), f.bytes, Some(f.range).filter(|r| !r.trim().is_empty())))
            .collect();
        match self.session.combine_ranges(&sources) {
            Ok(bytes) => {
                let message = crate::i18n::fmt(tl!("Combined {n} files"), &[("n", &count.to_string())]);
                self.open_created("Combined.pdf", bytes, &message)
            }
            Err(e) => self.notify_fmt("Couldn't combine files: {e}", &[("e", &e.to_string())]),
        }
    }
}
