use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use async_trait::async_trait;
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::EphemeralKvStore;

#[derive(Clone)]
pub struct MemoryKvStore {
    namespace: String,
    inner: Arc<RwLock<MemoryState>>,
}

#[derive(Clone)]
struct MemoryEntry {
    value: serde_json::Value,
    expires_at: Option<OffsetDateTime>,
}

#[derive(Default)]
struct MemoryState {
    entries: HashMap<String, MemoryEntry>,
    expirations: BTreeSet<(OffsetDateTime, String)>,
}

impl std::ops::Deref for MemoryState {
    type Target = HashMap<String, MemoryEntry>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl MemoryState {
    fn insert(&mut self, key: String, entry: MemoryEntry) {
        self.remove(&key);
        if let Some(deadline) = entry.expires_at {
            self.expirations.insert((deadline, key.clone()));
        }
        self.entries.insert(key, entry);
    }
    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            if let Some(deadline) = entry.expires_at {
                self.expirations.remove(&(deadline, key.to_owned()));
            }
        }
    }
    // Expired unique tokens are reclaimed on the next store operation, even if never revisited.
    fn purge_expired(&mut self) {
        let now = OffsetDateTime::now_utc();
        while self
            .expirations
            .first()
            .is_some_and(|(deadline, _)| *deadline <= now)
        {
            let (_, key) = self
                .expirations
                .pop_first()
                .expect("expiry index contains a deadline");
            self.entries.remove(&key);
        }
    }
}

impl MemoryKvStore {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            inner: Arc::new(RwLock::new(MemoryState::default())),
        }
    }

    pub fn raw_key_for_test(&self, key: &str) -> String {
        self.namespaced_key(key)
    }

    fn namespaced_key(&self, key: &str) -> String {
        format!("{}:{}", self.namespace, key)
    }

    fn expires_at(ttl: Option<time::Duration>) -> Option<OffsetDateTime> {
        ttl.map(|ttl| OffsetDateTime::now_utc() + ttl)
    }

    fn ensure_counter_ttl(ttl: Option<time::Duration>) -> anyhow::Result<()> {
        anyhow::ensure!(
            ttl.is_none_or(|ttl| ttl > time::Duration::ZERO),
            "counter_ttl_must_be_positive"
        );
        Ok(())
    }

    #[cfg(test)]
    pub async fn entry_count_for_test(&self) -> usize {
        self.inner.read().await.entries.len()
    }

    pub async fn clear_namespace(&self) {
        let prefix = format!("{}:", self.namespace);
        let mut map = self.inner.write().await;
        map.purge_expired();
        let keys: Vec<_> = map
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        for key in keys {
            map.remove(&key);
        }
    }
}

#[async_trait]
impl EphemeralKvStore for MemoryKvStore {
    async fn set_json(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl: Option<time::Duration>,
    ) -> anyhow::Result<()> {
        let mut map = self.inner.write().await;
        map.purge_expired();
        map.insert(
            self.namespaced_key(key),
            MemoryEntry {
                value,
                expires_at: Self::expires_at(ttl),
            },
        );
        Ok(())
    }

    async fn get_json(&self, key: &str) -> anyhow::Result<Option<serde_json::Value>> {
        let namespaced_key = self.namespaced_key(key);
        let mut map = self.inner.write().await;
        map.purge_expired();
        let Some(entry) = map.get(&namespaced_key).cloned() else {
            return Ok(None);
        };

        if entry
            .expires_at
            .is_some_and(|deadline| deadline <= OffsetDateTime::now_utc())
        {
            map.remove(&namespaced_key);
            return Ok(None);
        }

        Ok(Some(entry.value))
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.inner.write().await.remove(&self.namespaced_key(key));
        Ok(())
    }

    async fn touch(&self, key: &str, ttl: time::Duration) -> anyhow::Result<bool> {
        let namespaced_key = self.namespaced_key(key);
        let mut map = self.inner.write().await;
        map.purge_expired();
        let Some(mut entry) = map.get(&namespaced_key).cloned() else {
            return Ok(false);
        };

        if entry
            .expires_at
            .is_some_and(|deadline| deadline <= OffsetDateTime::now_utc())
        {
            map.remove(&namespaced_key);
            return Ok(false);
        }

        entry.expires_at = Some(OffsetDateTime::now_utc() + ttl);
        map.insert(namespaced_key, entry);
        Ok(true)
    }

    async fn set_if_absent_json(
        &self,
        key: &str,
        value: serde_json::Value,
        ttl: Option<time::Duration>,
    ) -> anyhow::Result<bool> {
        let namespaced_key = self.namespaced_key(key);
        let mut map = self.inner.write().await;
        map.purge_expired();

        if let Some(entry) = map.get(&namespaced_key) {
            if entry
                .expires_at
                .is_some_and(|deadline| deadline <= OffsetDateTime::now_utc())
            {
                map.remove(&namespaced_key);
            } else {
                return Ok(false);
            }
        }

        map.insert(
            namespaced_key,
            MemoryEntry {
                value,
                expires_at: Self::expires_at(ttl),
            },
        );
        Ok(true)
    }

    async fn increment_counter(
        &self,
        key: &str,
        amount: i64,
        ttl: Option<time::Duration>,
    ) -> anyhow::Result<i64> {
        Self::ensure_counter_ttl(ttl)?;

        let namespaced_key = self.namespaced_key(key);
        let mut map = self.inner.write().await;
        map.purge_expired();
        let now = OffsetDateTime::now_utc();
        let existing = map.get(&namespaced_key).cloned();
        let active_entry =
            existing.filter(|entry| entry.expires_at.is_none_or(|deadline| deadline > now));
        if active_entry.is_none() {
            map.remove(&namespaced_key);
        }

        let current = active_entry
            .as_ref()
            .map(|entry| {
                entry
                    .value
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("counter_value_must_be_integer"))
            })
            .transpose()?
            .unwrap_or(0);
        let next = current
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("counter_value_overflow"))?;
        let expires_at = ttl
            .map(|ttl| now + ttl)
            .or_else(|| active_entry.as_ref().and_then(|entry| entry.expires_at));
        map.insert(
            namespaced_key,
            MemoryEntry {
                value: serde_json::Value::Number(next.into()),
                expires_at,
            },
        );
        Ok(next)
    }
}
