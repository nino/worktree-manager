//! Periodic background `git fetch` of every configured repo.

use std::time::Duration;

use log::warn;

use crate::git::{fetch_repo, has_remote};
use crate::types::RepoConfig;

/// How often to fetch: 8 minutes and 43 seconds.
pub const FETCH_INTERVAL: Duration = Duration::from_secs(8 * 60 + 43);

/// Fetch every repo that has a remote, concurrently. Never fails; returns how
/// many repos fetched successfully.
pub async fn fetch_all(repos: Vec<RepoConfig>) -> usize {
    let mut set = tokio::task::JoinSet::new();
    for repo in repos {
        set.spawn(async move {
            if !has_remote(&repo.path, "origin").await {
                return false;
            }
            match fetch_repo(&repo.path).await {
                Ok(()) => true,
                Err(e) => {
                    warn!("auto-fetch: {} failed: {e}", repo.name);
                    false
                }
            }
        });
    }
    let mut fetched = 0;
    while let Some(r) = set.join_next().await {
        if r.unwrap_or(false) {
            fetched += 1;
        }
    }
    fetched
}
