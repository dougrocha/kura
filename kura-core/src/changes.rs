//! The change feed. Every edit writes a `changes` row in the same transaction
//! as the edit, so the feed can't miss or invent one. `Library::watch` reads that table,
//! which also makes it work across processes: an image the CLI adds shows up
//! in the server's feed.

use std::{collections::VecDeque, time::Duration};

use futures::Stream;
use jiff::Timestamp;
use tokio::sync::watch;

use crate::{error::Result, model::ImageHash, store::Store};

/// How often a waiting feed checks for commits made by other processes.
/// Commits in this process wake it immediately.
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const BATCH: i64 = 100;

/// Position in the change log. Increases with every edit; use it to resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChangeSeq(pub u64);

/// What an edit was. Not exhaustive: match with a `_` arm, since new kinds
/// arrive (an older reader skips a kind it doesn't know instead of failing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeKind {
    Added,
    Renamed,
    Tagged,
    Untagged,
    Deleted,
    /// A caption row was written or cleared.
    Captioned,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Renamed => "renamed",
            Self::Tagged => "tagged",
            Self::Untagged => "untagged",
            Self::Deleted => "deleted",
            Self::Captioned => "captioned",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "added" => Self::Added,
            "renamed" => Self::Renamed,
            "tagged" => Self::Tagged,
            "untagged" => Self::Untagged,
            "deleted" => Self::Deleted,
            "captioned" => Self::Captioned,
            _ => return None,
        })
    }
}

/// One edit. Carries identity, not a snapshot: fetch the image if you need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub seq: ChangeSeq,
    pub kind: ChangeKind,
    pub image: ImageHash,
    /// The new name (added, renamed), the tag (tagged, untagged), the old
    /// name (deleted), or `ocr`, `manual`, `cleared:ocr`, `cleared:manual`
    /// (captioned, never the text itself).
    pub detail: Option<String>,
    pub at: Timestamp,
}

/// What a feed yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    Change(Change),
    /// The feed was asked to resume from a point the log no longer holds
    /// (it's trimmed by `Library::sweep`). Reload everything with `list`,
    /// then keep going: newer changes follow.
    Resync,
}

/// Wakes waiting feeds when this process commits an edit.
#[derive(Clone)]
pub(crate) struct Notifier {
    tx: watch::Sender<u64>,
}

impl Notifier {
    pub(crate) fn new() -> Self {
        Self { tx: watch::Sender::new(0) }
    }

    pub(crate) fn notify(&self) {
        self.tx.send_modify(|n| *n = n.wrapping_add(1));
    }
}

struct FeedState {
    store: Store,
    wake: watch::Receiver<u64>,
    after: u64,
    pending: VecDeque<WatchEvent>,
}

/// Changes after `since` (or after the last one committed now), then live
/// ones, forever. The start point is fixed here, not at the first poll, so an
/// edit made right after subscribing is never missed.
pub(crate) async fn watch(
    store: Store,
    since: Option<ChangeSeq>,
) -> Result<impl Stream<Item = Result<WatchEvent>> + Send + 'static> {
    let mut pending = VecDeque::new();
    let (oldest, newest) = store.change_bounds().await?;
    let after = match since {
        None => newest,
        // Trimmed away, or from some other library (a seq this one never reached).
        Some(ChangeSeq(since)) if since > newest || oldest.is_some_and(|oldest| since + 1 < oldest) => {
            pending.push_back(WatchEvent::Resync);
            newest
        }
        Some(ChangeSeq(since)) => since,
    };
    let state = FeedState { wake: store.notifier().tx.subscribe(), store, after, pending };

    Ok(futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(event) = st.pending.pop_front() {
                return Some((Ok(event), st));
            }
            // Mark before reading, so a commit that lands during the read still wakes us.
            st.wake.mark_unchanged();
            match st.store.changes_after(st.after, BATCH).await {
                Ok(batch) if batch.through > st.after => {
                    st.after = batch.through;
                    st.pending.extend(batch.changes.into_iter().map(WatchEvent::Change));
                }
                Ok(_) => {
                    let _ = tokio::time::timeout(POLL_INTERVAL, st.wake.changed()).await;
                }
                Err(e) => return Some((Err(e), st)),
            }
        }
    }))
}
