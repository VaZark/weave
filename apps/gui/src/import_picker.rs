//! System import choosers and the report for a completed partial import.
use crate::{Element, Message};
use iad::prelude::{self as w, typography as ty};
use iced::Length;
use iced::widget::{column, row, scrollable, space};
use iced_adaptive_design as iad;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use weave_player_lib::core::ImportReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Files,
    Folder,
}

/// Opening this future launches the desktop chooser directly; cancellation imports nothing.
pub async fn choose(mode: Mode) -> Vec<PathBuf> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_directory(initial_directory())
        .set_title(match mode {
            Mode::Files => "Import files",
            Mode::Folder => "Import folder",
        });
    match mode {
        Mode::Files => dialog
            .pick_files()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|file| file.path().to_path_buf())
            .collect(),
        Mode::Folder => dialog
            .pick_folder()
            .await
            .map(|folder| vec![folder.path().to_path_buf()])
            .unwrap_or_default(),
    }
}

pub fn report_view(report: &ImportReport) -> Element<'_> {
    column![
        ty::title_3("Import results"),
        ty::body(summary(report)),
        scrollable(
            column(report.issues.iter().map(|issue| {
                w::action_row(issue.path.clone())
                    .subtitle(issue.message.clone())
                    .into()
            }))
            .spacing(8)
        )
        .height(Length::Fill),
        row![
            space::horizontal(),
            w::suggested_button(ty::label("Done")).on_press(Message::DismissImportReport)
        ],
    ]
    .spacing(12)
    .into()
}

fn initial_directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| {
            let music = home.join("Music");
            if music.is_dir() { music } else { home }
        })
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn summary(report: &ImportReport) -> String {
    let mut parts = Vec::new();
    if report.added > 0 {
        parts.push(format!(
            "Imported {} {}",
            report.added,
            if report.added == 1 { "track" } else { "tracks" }
        ));
    }
    if report.skipped > 0 {
        parts.push(format!("{} already indexed", report.skipped));
    }
    if report.ignored > 0 {
        parts.push(format!("{} non-audio files ignored", report.ignored));
    }
    if !report.issues.is_empty() {
        parts.push(format!(
            "{} {}",
            report.issues.len(),
            if report.issues.len() == 1 {
                "issue"
            } else {
                "issues"
            }
        ));
    }
    if parts.is_empty() {
        "No audio found".into()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    pub fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "weave-picker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    pub fn wav(path: &Path) {
        let mut bytes = Vec::from(&b"RIFF"[..]);
        bytes.extend(36u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(0u32.to_le_bytes());
        std::fs::write(path, bytes).unwrap();
    }
    #[test]
    fn one_batch_imports_files_and_folders_with_partial_results_and_no_duplicate_tracks() {
        let dir = temp();
        let music = dir.join("music");
        std::fs::create_dir(&music).unwrap();
        wav(&music.join("a.wav"));
        wav(&dir.join("b.wav"));
        std::fs::write(music.join("notes.txt"), "not audio").unwrap();
        let db = dir.join("library.db");
        let paths = vec![
            music.clone(),
            music.join("a.wav"),
            dir.join("b.wav"),
            dir.join("missing"),
        ];
        let report = crate::backend::import(&db, &paths).unwrap();
        assert_eq!(report.added, 2);
        assert_eq!(report.ignored, 1);
        assert_eq!(report.issues.len(), 1);
        assert!(summary(&report).contains("1 issue"));
        let report = crate::backend::import(&db, &paths[..3]).unwrap();
        assert_eq!((report.added, report.skipped), (0, 2));
        let library = weave_player_lib::Library::open(&db).unwrap();
        assert_eq!(
            library
                .search(&Default::default(), weave_player_lib::core::Page::ALL)
                .unwrap()
                .total,
            2
        );
        drop(library);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
