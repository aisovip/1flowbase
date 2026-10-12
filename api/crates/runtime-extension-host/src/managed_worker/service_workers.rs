//! Actor-isolated carrier reuse. The Host owns retirement; business sessions stay in workers.
use super::{invalid, LoadedManagedBinding};
use crate::stdio_runtime::MultiplexProviderWorker;
use extension_package_runtime::FrameworkResult;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Activity {
    active: usize,
    last_used: Instant,
    retiring: bool,
}
#[derive(Debug)]
struct Entry {
    worker: Arc<MultiplexProviderWorker>,
    activity: Mutex<Activity>,
    _capacity: tokio::sync::OwnedSemaphorePermit,
}
#[derive(Debug, Default)]
struct State {
    epoch: u64,
    resetting: bool,
    entries: BTreeMap<String, Arc<Entry>>,
}
#[derive(Debug)]
pub(crate) struct ServiceWorkers {
    state: Mutex<State>,
    retirement: tokio::sync::Mutex<()>,
    idle: Duration,
    budget: Arc<tokio::sync::Semaphore>,
}

/// Retained by the carrier until Response/Cancelled or actual worker termination.
struct ActivityLease(Arc<Entry>);
impl Drop for ActivityLease {
    fn drop(&mut self) {
        if let Ok(mut activity) = self.0.activity.lock() {
            activity.active -= 1;
            activity.last_used = Instant::now();
        }
    }
}
impl ServiceWorkers {
    pub(super) fn new(idle_ms: u64, budget: Arc<tokio::sync::Semaphore>) -> Arc<Self> {
        let pool = Arc::new(Self {
            state: Mutex::new(State::default()),
            retirement: tokio::sync::Mutex::new(()),
            idle: Duration::from_millis(idle_ms),
            budget,
        });
        let weak = Arc::downgrade(&pool);
        tokio::spawn(async move {
            Self::idle_loop(weak).await;
        });
        pool
    }
    pub(super) fn epoch(&self) -> FrameworkResult<u64> {
        let state = self
            .state
            .lock()
            .map_err(|_| invalid("service worker registry unavailable"))?;
        if state.resetting {
            return Err(invalid("service worker reset in progress"));
        }
        Ok(state.epoch)
    }
    pub(super) fn acquire(
        &self,
        epoch: u64,
        actor: &str,
        binding: &LoadedManagedBinding,
    ) -> FrameworkResult<(Arc<MultiplexProviderWorker>, Box<dyn Send + Sync>)> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid("service worker registry unavailable"))?;
        if state.resetting || state.epoch != epoch {
            return Err(invalid("service worker generation retired"));
        }
        let entry = if let Some(entry) = state.entries.get(actor) {
            if entry.worker.is_failed()
                || entry
                    .activity
                    .lock()
                    .map_err(|_| invalid("service worker activity unavailable"))?
                    .retiring
            {
                return Err(invalid("service worker is retiring"));
            }
            entry.clone()
        } else {
            let capacity = self
                .budget
                .clone()
                .try_acquire_owned()
                .map_err(|_| invalid("managed worker capacity exhausted"))?;
            let worker = Arc::new(MultiplexProviderWorker::activate(
                binding.runtime_executable.clone(),
                binding.limits.clone(),
                epoch,
            )?);
            let entry = Arc::new(Entry {
                worker,
                _capacity: capacity,
                activity: Mutex::new(Activity {
                    active: 0,
                    last_used: Instant::now(),
                    retiring: false,
                }),
            });
            state.entries.insert(actor.to_owned(), entry.clone());
            entry
        };
        entry
            .activity
            .lock()
            .map_err(|_| invalid("service worker activity unavailable"))?
            .active += 1;
        Ok((entry.worker.clone(), Box::new(ActivityLease(entry))))
    }
    /// Fence admitted but unpolled calls before terminating and observing every child exit.
    pub(crate) async fn retire_all(&self) -> FrameworkResult<()> {
        let _retirement = self.retirement.lock().await;
        let entries = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| invalid("service worker registry unavailable"))?;
            state.epoch = state
                .epoch
                .checked_add(1)
                .ok_or_else(|| invalid("service worker generation exhausted"))?;
            state.resetting = true;
            for entry in state.entries.values() {
                entry
                    .activity
                    .lock()
                    .map_err(|_| invalid("service worker activity unavailable"))?
                    .retiring = true;
            }
            state.entries.values().cloned().collect::<Vec<_>>()
        };
        let mut failure = None;
        for entry in &entries {
            if let Err(error) = Self::reap(entry).await {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            // Fail closed until a subsequent retirement confirms all old processes exited.
            return Err(error);
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| invalid("service worker registry unavailable"))?;
        state
            .entries
            .retain(|_, entry| !entries.iter().any(|old| Arc::ptr_eq(old, entry)));
        state.resetting = false;
        Ok(())
    }
    async fn reap(entry: &Entry) -> FrameworkResult<()> {
        let control = entry.worker.process_control();
        control.terminate().await;
        if !control.confirmed_exit().await {
            return Err(invalid("service worker reap unfinished"));
        }
        Ok(())
    }
    async fn idle_loop(weak: Weak<Self>) {
        loop {
            let Some(pool) = weak.upgrade() else {
                return;
            };
            let tick = (pool.idle / 4)
                .max(Duration::from_millis(1))
                .min(Duration::from_secs(60));
            drop(pool);
            tokio::time::sleep(tick).await;
            let Some(pool) = weak.upgrade() else {
                return;
            };
            if let Err(error) = pool.reap_idle().await {
                tracing::warn!(%error, "service worker idle reap unfinished");
            }
        }
    }
    async fn reap_idle(&self) -> FrameworkResult<()> {
        let entries = {
            let state = self
                .state
                .lock()
                .map_err(|_| invalid("service worker registry unavailable"))?;
            let mut expired = Vec::new();
            for (actor, entry) in &state.entries {
                let mut activity = entry
                    .activity
                    .lock()
                    .map_err(|_| invalid("service worker activity unavailable"))?;
                if activity.active == 0
                    && (entry.worker.is_failed()
                        || activity.last_used.elapsed() >= self.idle
                        || activity.retiring)
                {
                    activity.retiring = true;
                    expired.push((actor.clone(), entry.clone()));
                }
            }
            expired
        };
        for (actor, entry) in entries {
            Self::reap(&entry).await?;
            let mut state = self
                .state
                .lock()
                .map_err(|_| invalid("service worker registry unavailable"))?;
            if state
                .entries
                .get(&actor)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                state.entries.remove(&actor);
            }
        }
        Ok(())
    }
}
