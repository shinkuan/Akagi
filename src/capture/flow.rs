//! Per-WebSocket-flow bridge ownership map, generic over the flow key.
//!
//! Both capture backends face the same problem: each WebSocket connection
//! needs a fresh [`crate::bridge::Bridge`] instance because the parser's
//! request/response correlation is per-connection. The hudsucker handler
//! historically keyed by `SocketAddr`; the Chromium backend keys by
//! `(sessionId, requestId)`.
//!
//! `FlowBridges<K>` lifts that pattern out so both backends share one
//! lazy-create + ref-count-clean-up implementation.

use crate::bridge::{self, Bridge, BridgeHooks};
use crate::config::Platform;
use crate::logger::Session;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex as StdMutex,
};
use tracing::warn;

/// Shared per-flow bridge. Both directions of a single WS flow lock the
/// same `Mutex<Bridge>` so `parse(direction, ..)` calls serialise (Majsoul's
/// pending-request map lives inside the parser).
pub type SharedBridge = Arc<StdMutex<Box<dyn Bridge>>>;

pub struct FlowBridges<K> {
    session: Arc<Session>,
    platform: Platform,
    /// Autoplay's shared slots, handed to every bridge this map creates.
    /// Empty when the backend has no autoplay context (MITM proxy).
    hooks: BridgeHooks,
    map: StdMutex<HashMap<K, SharedBridge>>,
    next_flow_id: AtomicU64,
}

impl<K> FlowBridges<K>
where
    K: Eq + Hash + Clone,
{
    pub fn new(session: Arc<Session>, platform: Platform, hooks: BridgeHooks) -> Self {
        Self {
            session,
            platform,
            hooks,
            map: StdMutex::new(HashMap::new()),
            next_flow_id: AtomicU64::new(1),
        }
    }

    /// Get the bridge for `key`, creating it on first call. `slug` is a
    /// filename-safe label used in the per-flow log file name (caller is
    /// responsible for sanitising). `label` is the human-readable
    /// description written to the log header.
    pub fn platform(&self) -> Platform {
        self.platform
    }

    pub fn acquire(&self, key: K, slug: &str, label: &str) -> SharedBridge {
        let mut map = self.map.lock().expect("flow bridges mutex poisoned");
        map.entry(key)
            .or_insert_with(|| {
                let flow_id = self.next_flow_id.fetch_add(1, Ordering::Relaxed);
                let file_name = format!("{flow_id:06}-{slug}.log");
                let flow_log = match self.session.flow_logger(
                    self.platform.subdir(),
                    &file_name,
                    label.to_string(),
                ) {
                    Ok(log) => Some(log),
                    Err(e) => {
                        warn!("failed to open flow log for {label}: {e:#}");
                        None
                    }
                };
                Arc::new(StdMutex::new(bridge::for_platform(
                    self.platform,
                    flow_log,
                    Some(self.session.clone()),
                    self.hooks.clone(),
                )))
            })
            .clone()
    }

    /// Drop the caller's reference. If only the map's own `Arc` remained,
    /// remove the entry so per-connection state doesn't leak.
    pub fn release(&self, key: &K, b: SharedBridge) {
        drop(b);
        let mut map = self.map.lock().expect("flow bridges mutex poisoned");
        if let Some(existing) = map.get(key) {
            if Arc::strong_count(existing) == 1 {
                map.remove(key);
            }
        }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.map.lock().unwrap().is_empty()
    }
}

/// Sanitize a free-form label into a filename-safe slug. Anything outside
/// `[A-Za-z0-9_-]` becomes `_`. Empty input returns `"flow"`.
pub fn slugify(input: &str) -> String {
    let trimmed = input.trim_matches('/');
    if trimmed.is_empty() {
        return "flow".into();
    }
    trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("/game-gateway"), "game-gateway");
        assert_eq!(slugify(""), "flow");
        assert_eq!(slugify("/"), "flow");
        assert_eq!(slugify("a/b c"), "a_b_c");
    }
}
