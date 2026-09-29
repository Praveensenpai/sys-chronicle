use super::*;

fn sample(ep: u32, watched: u64, dur: u64) -> AnimeSyncRecord {
    let watch_pct = if dur > 0 {
        (watched as f32 / dur as f32 * 100.0).min(100.0)
    } else {
        0.0
    };
    AnimeSyncRecord {
        raw_title: "raw".into(),
        canonical_title: "title".into(),
        episode: ep,
        path: None,
        duration_secs: dur,
        watched_secs: watched,
        position_secs: Some(watched),
        watch_pct,
        mal_anime_id: None,
        mal_current_ep: None,
        mal_total_episodes: None,
        status: SyncStatus::Watching,
        last_updated: "2026-09-17 12:00:00".into(),
        intervals: vec![],
    }
}

#[test]
fn test_sync_status_badge() {
    assert_eq!(SyncStatus::Synced.badge().0, "✔ SYNCED");
    assert_eq!(
        SyncStatus::AheadOnMal { mal_episode: 6 }.badge().0,
        "⏩ MAL AHEAD"
    );
}

#[test]
fn test_anime_record_serialization() {
    let mut r = sample(4, 1200, 1440);
    r.status = SyncStatus::AheadOnMal { mal_episode: 6 };
    let json = serde_json::to_string(&r).expect("serialize");
    let parsed: AnimeSyncRecord = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.episode, 4);
    assert_eq!(parsed.status, SyncStatus::AheadOnMal { mal_episode: 6 });
}

#[test]
fn test_deduplicate_records() {
    let mut r1 = sample(0, 100, 300);
    r1.raw_title = "SP01.mkv".into();
    let mut r2 = r1.clone();
    r2.watched_secs = 200;
    let deduped = AnimeSyncHistory::deduplicate(vec![r1, r2]);
    assert_eq!(deduped.len(), 1);
    assert_eq!(deduped[0].watched_secs, 200);
}

#[test]
fn test_update_progress_monotonic_watched_secs() {
    let (raw, title, ep) = ("Test - 01.mkv", "Test", 1);
    AnimeSyncHistory::update_progress(raw, title, ep, None, 1000, 15).expect("updated");
    assert_eq!(
        AnimeSyncHistory::find_record(raw, title, ep)
            .expect("found")
            .watched_secs,
        15
    );

    AnimeSyncHistory::update_progress_full(ProgressUpdateParams {
        raw_title: raw,
        canonical_title: title,
        episode: ep,
        path: None,
        duration_secs: 1000,
        watched_secs: 1,
        position_secs: Some(1),
        intervals: &[],
    })
    .expect("updated");
    assert_eq!(
        AnimeSyncHistory::find_record(raw, title, ep)
            .expect("found")
            .watched_secs,
        15
    );
}

#[test]
fn test_upsert_does_not_regress_watched_progress() {
    use crate::monitor::timeline_tracker::PlaybackInterval;
    let raw = "Non Non Biyori - S01E08 [1080p].mkv";
    let title = "Non Non Biyori";
    let ep = 8;

    // Simulate a fully-watched record (written by the threshold handler)
    let mut full = sample(ep, 1437, 1437);
    full.raw_title = raw.into();
    full.canonical_title = title.into();
    full.watch_pct = 100.0;
    full.status = SyncStatus::ThresholdMet;
    full.intervals = vec![PlaybackInterval { start: 0, end: 1437 }];
    AnimeSyncHistory::upsert(full).expect("upsert full");

    // Simulate briefly re-opening the episode and upsert being called again
    let mut brief = sample(ep, 16, 1437);
    brief.raw_title = raw.into();
    brief.canonical_title = title.into();
    brief.watch_pct = 1.1;
    brief.status = SyncStatus::Watching;
    brief.intervals = vec![PlaybackInterval { start: 118, end: 134 }];
    AnimeSyncHistory::upsert(brief).expect("upsert brief");

    let record = AnimeSyncHistory::find_record(raw, title, ep).expect("found");
    assert!(
        record.watched_secs >= 1437,
        "watched_secs must not regress: got {}",
        record.watched_secs
    );
    assert!(
        record.watch_pct >= 100.0,
        "watch_pct must not regress: got {}",
        record.watch_pct
    );
    assert_eq!(
        record.status,
        SyncStatus::ThresholdMet,
        "status must not regress to Watching"
    );
}

#[test]
fn test_reopen_synced_episode_does_not_regress_status_or_pct() {
    use crate::monitor::timeline_tracker::PlaybackInterval;
    let raw = "Yuru Camp - S01E05 [1080p].mkv";
    let title = "Yuru Camp";
    let ep = 5;

    // Simulate already synced record
    let mut synced = sample(ep, 1422, 1422);
    synced.raw_title = raw.into();
    synced.canonical_title = title.into();
    synced.watch_pct = 100.0;
    synced.status = SyncStatus::Synced;
    synced.intervals = vec![PlaybackInterval { start: 0, end: 1422 }];
    AnimeSyncHistory::upsert(synced).expect("upsert synced");

    // Simulate reopening and playing only 6 seconds
    AnimeSyncHistory::update_progress_full(ProgressUpdateParams {
        raw_title: raw,
        canonical_title: title,
        episode: ep,
        path: None,
        duration_secs: 1422,
        watched_secs: 6,
        position_secs: Some(1422),
        intervals: &[PlaybackInterval {
            start: 1416,
            end: 1422,
        }],
    })
    .expect("update progress full");

    let record = AnimeSyncHistory::find_record(raw, title, ep).expect("found");
    assert_eq!(record.watched_secs, 1422);
    assert_eq!(record.watch_pct, 100.0);
    assert_eq!(record.status, SyncStatus::Synced);
}

#[test]
fn test_reconcile_and_sort_episodes_from_start_to_finish() {
    let mut ep8 = sample(8, 6, 1422);
    ep8.raw_title = "Yuru Camp - S02E08.mkv".into();
    ep8.canonical_title = "Yuru Camp".into();
    ep8.last_updated = "2026-09-29 16:53:35".into();

    let mut ep7 = sample(7, 1342, 1422);
    ep7.raw_title = "Yuru Camp - S02E07.mkv".into();
    ep7.canonical_title = "Yuru Camp".into();
    ep7.status = SyncStatus::Synced;
    ep7.mal_anime_id = Some(38474);
    ep7.mal_current_ep = Some(7);
    ep7.mal_total_episodes = Some(13);
    ep7.last_updated = "2026-09-29 16:52:52".into();

    let mut ep5 = sample(5, 6, 1422);
    ep5.raw_title = "Yuru Camp - S02E05.mkv".into();
    ep5.canonical_title = "Yuru Camp".into();
    ep5.status = SyncStatus::Watching;
    ep5.last_updated = "2026-09-29 15:48:51".into();

    let mut ep6 = sample(6, 1347, 1422);
    ep6.raw_title = "Yuru Camp - S02E06.mkv".into();
    ep6.canonical_title = "Yuru Camp".into();
    ep6.status = SyncStatus::Synced;
    ep6.mal_anime_id = Some(38474);
    ep6.mal_current_ep = Some(6);
    ep6.mal_total_episodes = Some(13);
    ep6.last_updated = "2026-09-29 16:23:05".into();

    // Input in arbitrary order: 8, 7, 5, 6
    let records = vec![ep8, ep7, ep5, ep6];
    let reconciled = AnimeSyncHistory::deduplicate(records);

    // Verify sorted from start to finish: 5, 6, 7, 8
    assert_eq!(reconciled.len(), 4);
    assert_eq!(reconciled[0].episode, 5);
    assert_eq!(reconciled[1].episode, 6);
    assert_eq!(reconciled[2].episode, 7);
    assert_eq!(reconciled[3].episode, 8);

    // Ep 5 should be reconciled as Synced because Ep 7 is Synced on MAL
    assert_eq!(reconciled[0].status, SyncStatus::Synced);
    assert_eq!(reconciled[0].watch_pct, 100.0);
    assert_eq!(reconciled[0].mal_anime_id, Some(38474));
    assert_eq!(reconciled[0].mal_total_episodes, Some(13));

    // Ep 8 should inherit mal_anime_id and mal_total_episodes, but stay Watching
    assert_eq!(reconciled[3].status, SyncStatus::Watching);
    assert_eq!(reconciled[3].mal_anime_id, Some(38474));
    assert_eq!(reconciled[3].mal_total_episodes, Some(13));
}


