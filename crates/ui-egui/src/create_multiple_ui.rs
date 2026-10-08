//! Create a PDF ▸ Multiple files: PDFs, images and text files, in order, become one combined
//! PDF or one PDF each. Every file is converted when it is added, so a bad one is refused then.

use std::sync::Arc;

use egui::{Align, Layout};
use pdfcraft_engine::SourceKind;

use crate::combine_ui::CombineFile;
use crate::theme::{self, Tokens};
use crate::{Dialog, PdfCraftApp, widgets};

/// The files (already converted to PDF) and what to make of them.
#[derive(Clone, Debug, Default)]
pub struct CreateMultiple {
    pub files: Vec<CombineFile>,
    /// One PDF per file, rather than one PDF of them all.
    pub separate: bool,
}

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) -> (bool, bool) {
    ui.label(egui::RichText::new(tl!("Create PDF from multiple files")).font(theme::semibold(18.0)));
    ui.add_space(6.0);
    let draft = &mut app.create_multiple;
    ui.radio_value(&mut draft.separate, false, tl!("Combine into one PDF"));
    ui.radio_value(&mut draft.separate, true, tl!("Create a separate PDF for each file"));
    ui.add_space(4.0);
    let hint = if draft.separate {
        tl!("Each image or text file becomes its own PDF. Files that are already PDFs are skipped.")
    } else {
        tl!("Files are combined in this order. Leave Pages empty to take every page.")
    };
    ui.label(egui::RichText::new(hint).small().color(t.text_faint));
    ui.add_space(8.0);
    let ranges = !draft.separate;
    crate::combine_ui::file_list(ui, &mut draft.files, t, "create-multiple-files", ranges);
    ui.add_space(10.0);
    let (mut go, mut cancel) = (false, false);
    ui.horizontal(|ui| {
        if widgets::pill_button(ui, tl!("Add files…"), false).clicked() {
            app.create_multiple_dialog();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let ok = !app.create_multiple.files.is_empty();
            if ui
                .add_enabled_ui(ok, |ui| widgets::pill_button(ui, tl!("Create"), true))
                .inner
                .on_disabled_hover_text(tl!("Add at least one file"))
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

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

/// `<stem>.pdf` in `dir`, or `<stem> (2).pdf` and so on when that name is taken: nothing that is
/// already there is overwritten.
#[cfg(not(target_arch = "wasm32"))]
fn unused(dir: &std::path::Path, stem: &str) -> std::path::PathBuf {
    let safe: String = stem.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '_' } else { c }).collect();
    let safe = if safe.trim_matches('.').is_empty() { "document".to_string() } else { safe };
    let first = dir.join(format!("{safe}.pdf"));
    if !first.exists() {
        return first;
    }
    (2..10_000u32).map(|n| dir.join(format!("{safe} ({n}).pdf"))).find(|p| !p.exists()).unwrap_or(first)
}

impl PdfCraftApp {
    /// Add picked files to the Create ▸ Multiple files list (and show it), converting each one.
    pub(crate) fn stage_create_multiple(&mut self, files: Vec<(String, Vec<u8>)>) {
        for (name, bytes) in files {
            if self.create_multiple.files.len() >= pdfcraft_engine::MAX_CREATE_FILES {
                self.notify_fmt("You can add up to {n} files at once", &[("n", &pdfcraft_engine::MAX_CREATE_FILES.to_string())]);
                break;
            }
            let converted = self.session.convert_to_pdf(&name, &Arc::new(bytes)).and_then(|(kind, pdf)| {
                let pages = self.session.page_count_of(&name, &pdf)?;
                Ok(CombineFile { name: name.clone(), bytes: pdf, pages, range: String::new(), kind })
            });
            match converted {
                Ok(file) => self.create_multiple.files.push(file),
                // A source error already names the file.
                Err(pdfcraft_engine::EditError::Source(why)) => self.notify_fmt("Couldn't add {e}", &[("e", &why)]),
                Err(e) => self.notify_fmt("Couldn't add {name}: {e}", &[("name", &name), ("e", &e.to_string())]),
            }
        }
        self.dialog = Some(Dialog::CreateMultiple);
    }

    /// Make the PDF, or the PDFs, from the listed files.
    pub fn finish_create_multiple(&mut self) {
        let CreateMultiple { files, separate } = std::mem::take(&mut self.create_multiple);
        if files.is_empty() {
            return;
        }
        if separate {
            return self.create_separate(files);
        }
        let count = files.len();
        let sources: Vec<(String, Arc<Vec<u8>>, Option<String>)> =
            files.into_iter().map(|f| (stem(&f.name).to_string(), f.bytes, Some(f.range).filter(|r| !r.trim().is_empty()))).collect();
        match self.session.combine_ranges(&sources) {
            Ok(bytes) => {
                let message = crate::i18n::fmt(tl!("Created a PDF from {n} files"), &[("n", &count.to_string())]);
                self.open_created("Combined.pdf", bytes, &message)
            }
            Err(e) => self.notify_fmt("Couldn't create a PDF: {e}", &[("e", &e.to_string())]),
        }
    }

    /// One PDF per converted file: into a folder the user chooses, or (in a browser, which has
    /// no folders) as new unsaved documents.
    fn create_separate(&mut self, files: Vec<CombineFile>) {
        let made: Vec<CombineFile> = files.into_iter().filter(|f| f.kind != SourceKind::Pdf).collect();
        if made.is_empty() {
            self.notify_tr("Those files are already PDFs");
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let write = move |app: &mut PdfCraftApp, dir: std::path::PathBuf| {
                let (mut written, mut failed) = (0usize, Vec::new());
                for f in &made {
                    let path = unused(&dir, stem(&f.name));
                    match crate::editing::write_atomically(&path.to_string_lossy(), &f.bytes) {
                        Ok(()) => written += 1,
                        Err(e) => failed.push(format!("{} ({e})", f.name)),
                    }
                }
                let dir = dir.display().to_string();
                let failed = failed.join(", ");
                let n = written.to_string();
                match (written, failed.is_empty()) {
                    (1, true) => app.notify_fmt("Created 1 PDF in {dir}", &[("dir", &dir)]),
                    (_, true) => app.notify_fmt("Created {n} PDFs in {dir}", &[("n", &n), ("dir", &dir)]),
                    (_, false) => app.notify_fmt("Created {n} PDFs in {dir}; failed: {failed}", &[("n", &n), ("dir", &dir), ("failed", &failed)]),
                }
            };
            match self.export_dir_override.clone() {
                Some(d) => write(self, d.into()),
                None => {
                    let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a folder for the new PDFs").to_string());
                    self.ask_one(crate::pickers::Ask::Folder(dialog), None, write);
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let n = made.len().to_string();
            for f in made {
                let message = crate::i18n::fmt(tl!("Created {n} PDFs"), &[("n", &n)]);
                self.open_created(&format!("{}.pdf", stem(&f.name)), f.bytes, &message);
            }
        }
    }
}
