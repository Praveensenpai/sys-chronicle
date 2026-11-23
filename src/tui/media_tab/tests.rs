use super::*;

fn rec(title: &str, episode: u32, updated: &str) -> AnimeSyncRecord {
    AnimeSyncRecord {
        raw_title: format!("{title} - {episode:02}.mkv"),
        canonical_title: title.to_string(),
        episode,
        path: None,
        duration_secs: 1440,
        watched_secs: 1440,
        position_secs: Some(1440),
        watch_pct: 100.0,
        mal_anime_id: None,
        mal_current_ep: None,
        mal_total_episodes: None,
        mal_title: None,
        mal_title_en: None,
        mal_title_ja: None,
        mal_image_url: None,
        status: SyncStatus::Watching,
        last_updated: updated.to_string(),
        intervals: vec![],
    }
}

fn record_indices(rows: &[MediaRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            MediaRow::Record(index) => Some(*index),
            MediaRow::Header(_) => None,
        })
        .collect()
}

fn header_labels(rows: &[MediaRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            MediaRow::Header(label) => Some(label.clone()),
            MediaRow::Record(_) => None,
        })
        .collect()
}

#[test]
fn flat_mode_emits_no_headers() {
    let records = vec![
        rec("Alpha", 1, "2026-10-15 10:00:00"),
        rec("Beta", 1, "2026-09-01 10:00:00"),
    ];
    let rows = build_rows(
        &records,
        GroupMode::Flat,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    assert!(header_labels(&rows).is_empty());
    assert_eq!(record_indices(&rows), vec![0, 1]);
}

#[test]
fn day_mode_groups_today_yesterday_and_older() {
    let records = vec![
        rec("Alpha", 1, "2026-10-15 10:00:00"),
        rec("Beta", 1, "2026-10-14 10:00:00"),
        rec("Gamma", 1, "2026-10-01 10:00:00"),
    ];
    let rows = build_rows(
        &records,
        GroupMode::Day,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    let labels = header_labels(&rows);
    assert_eq!(labels[0], "Today");
    assert_eq!(labels[1], "Yesterday");
    assert!(
        labels[2].contains("2026"),
        "unexpected label: {}",
        labels[2]
    );
    assert_eq!(record_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn week_mode_groups_by_calendar_week() {
    let records = vec![
        rec("Alpha", 1, "2026-10-15 10:00:00"),
        rec("Beta", 1, "2026-10-07 10:00:00"),
    ];
    let rows = build_rows(
        &records,
        GroupMode::Week,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    let labels = header_labels(&rows);
    assert!(labels[0].starts_with("This Week"));
    assert!(labels[1].starts_with("Last Week"));
}

#[test]
fn month_mode_groups_this_and_last_month() {
    let records = vec![
        rec("Alpha", 1, "2026-10-15 10:00:00"),
        rec("Beta", 1, "2026-09-20 10:00:00"),
        rec("Gamma", 1, "2026-05-02 10:00:00"),
    ];
    let rows = build_rows(
        &records,
        GroupMode::Month,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    let labels = header_labels(&rows);
    assert_eq!(labels[0], "This Month");
    assert_eq!(labels[1], "Last Month");
    assert!(labels[2].contains("May 2026"));
}

#[test]
fn episodes_of_same_anime_share_one_bucket() {
    let records = vec![
        rec("Frieren", 1, "2026-10-15 10:00:00"),
        rec("Frieren", 2, "2026-10-15 11:00:00"),
        rec("Bleach", 1, "2026-10-14 10:00:00"),
    ];
    let rows = build_rows(
        &records,
        GroupMode::Day,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    let labels = header_labels(&rows);
    assert_eq!(labels, vec!["Today", "Yesterday"]);
    assert_eq!(record_indices(&rows), vec![0, 1, 2]);
}

#[test]
fn group_mode_cycles_through_all_variants() {
    let mut mode = GroupMode::Day;
    let mut seen = vec![mode];
    for _ in 0..3 {
        mode = mode.next();
        seen.push(mode);
    }
    assert_eq!(
        seen,
        vec![
            GroupMode::Day,
            GroupMode::Week,
            GroupMode::Month,
            GroupMode::Flat
        ]
    );
    assert_eq!(GroupMode::Flat.next(), GroupMode::Day);
}

#[test]
fn multibyte_titles_do_not_panic() {
    let long = rec(
        "日本語のタイトルがとても長いアニメーション作品",
        1,
        "2026-10-15 10:00:00",
    );
    let display = crate::tui::truncate_name(&long.canonical_title, 22);
    assert!(display.chars().count() <= 22, "got {display:?}");

    let short = rec("ワンピース", 1, "2026-10-15 10:00:00");
    let display = crate::tui::truncate_name(&short.canonical_title, 22);
    assert_eq!(display, "ワンピース");
}

#[test]
fn unknown_timestamp_falls_back_to_unknown_date() {
    let records = vec![rec("Alpha", 1, "not-a-timestamp")];
    let rows = build_rows(
        &records,
        GroupMode::Day,
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
    );
    assert_eq!(header_labels(&rows), vec!["Unknown Date"]);
}

fn state_with(records: Vec<AnimeSyncRecord>, mode: GroupMode) -> MediaTabState {
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    MediaTabState {
        records,
        selected_index: 0,
        status_message: None,
        confirming_force_sync: false,
        list_state,
        group_mode: mode,
        cover_cache: super::super::media_cover::CoverCache::new(),
        sync_rx: None,
    }
}

/// End-to-end render of the exact record that triggered the production panic
/// (`end byte index 19 is not a cha boundary`). Must not panic for any group mode.
#[test]
fn render_media_tab_does_not_panic_on_multibyte_titles() {
    use ratatui::{backend::TestBackend, Terminal};

    let records = vec![
        rec("夢で世界を変えるなら", 0, "2026-10-15 10:00:00"),
        rec(
            "日本語のタイトルがとても長いアニメ",
            1,
            "2026-10-15 11:00:00",
        ),
        rec(
            "Laid-Back Camp - The Movie (2022) [Japanese] [1080p].mkv",
            0,
            "2026-10-14 10:00:00",
        ),
    ];

    for mode in [
        GroupMode::Day,
        GroupMode::Week,
        GroupMode::Month,
        GroupMode::Flat,
    ] {
        let mut state = state_with(records.clone(), mode);
        let backend = TestBackend::new(160, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");

        let result = terminal.draw(|f| {
            let area = f.size();
            render_media_tab(f, area, &mut state);
        });

        assert!(
            result.is_ok(),
            "render failed for mode {:?}: {:?}",
            mode,
            result.err()
        );
    }
}

#[test]
fn render_media_inspector_with_cover_and_titles() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut record = rec("Non Non Biyori", 1, "2026-11-21 23:25:35");
    record.mal_title = Some("Non Non Biyori Nonstop".into());
    record.mal_title_ja = Some("のんのんびより のんすとっぷ".into());
    record.mal_title_en = Some("Non Non Biyori Nonstop Season 3".into());
    record.mal_anime_id = Some(39808);

    let mut state = state_with(vec![record], GroupMode::Day);
    let backend = TestBackend::new(160, 40);
    let mut terminal = Terminal::new(backend).expect("terminal");

    let result = terminal.draw(|f| {
        let area = f.size();
        render_media_tab(f, area, &mut state);
    });

    assert!(result.is_ok());
    let buffer = terminal.backend().buffer();
    let content = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.get(x, y).symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(content.contains("Cover"));
    assert!(content.contains("Non Non Biyori Nonstop"));
    assert!(content.contains("Fetching Cover"));
}

#[test]
fn test_format_display_folder_and_filename() {
    use super::super::media_inspector::{format_display_filename, format_display_folder};

    let path = "/home/paisen/Videos/Anime/Non Non Biyori/Season 02/Non Non Biyori - S02E01.mkv";

    // Folder formatting across widths
    let folder_full = format_display_folder(path, 60);
    assert_eq!(
        folder_full,
        Some("/home/paisen/Videos/Anime/Non Non Biyori/Season 02".to_string())
    );

    let folder_short = format_display_folder(path, 30);
    assert_eq!(
        folder_short,
        Some(".../Non Non Biyori/Season 02".to_string())
    );

    let folder_tight = format_display_folder(path, 15);
    assert_eq!(folder_tight, Some(".../Season 02".to_string()));

    // Filename formatting preserves full name without middle dots
    let filename = format_display_filename(path, 35);
    assert_eq!(filename, "Non Non Biyori - S02E01.mkv");
    assert!(!filename.contains("..."));
    assert!(!filename.contains(".."));

    // Long filename truncation uses ellipsis at the end, not middle
    let truncated = format_display_filename(path, 20);
    assert_eq!(truncated.chars().count(), 20);
    assert!(truncated.ends_with('…'));
    assert!(!truncated.contains("..."));
}
