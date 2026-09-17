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
