//! Background workers and the live runtime state the UI reads each frame.

pub mod activity;
pub mod anti_afk;
pub mod auto_rejoin;
pub mod avatars;
pub mod cookie_check;
pub mod history;
pub mod instances;
pub mod renamer;
pub mod updater;

use crate::core::Core;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// Live per-account activity (running clients and memory use).
#[derive(Debug, Clone, Default)]
pub struct Activity {
    pub running: bool,
    pub client_count: usize,
    pub ram_mb: f64,
}

/// A running Roblox client, for the Instances page.
#[derive(Debug, Clone)]
pub struct Instance {
    pub pid: u32,
    pub user_id: u64,
    pub username: String,
    pub hidden: bool,
}

/// Everything background workers publish for the UI to read. Cheap to clone-read.
#[derive(Default)]
pub struct Live {
    activity: RwLock<HashMap<u64, Activity>>,
    rejoin_status: RwLock<HashMap<String, String>>,
    instances: RwLock<Vec<Instance>>,
    update_available: RwLock<Option<String>>,
    /// Transient status text shown in the UI (and a flag the UI clears after reading).
    toast: Mutex<Option<(String, bool)>>,
}

impl Live {
    pub fn activity_for(&self, user_id: u64) -> Activity {
        self.activity.read().unwrap_or_else(|p| p.into_inner()).get(&user_id).cloned().unwrap_or_default()
    }

    pub fn set_activity(&self, map: HashMap<u64, Activity>) {
        *self.activity.write().unwrap_or_else(|p| p.into_inner()) = map;
    }

    pub fn rejoin_status(&self, account: &str) -> Option<String> {
        self.rejoin_status.read().unwrap_or_else(|p| p.into_inner()).get(account).cloned()
    }

    pub fn all_rejoin_status(&self) -> HashMap<String, String> {
        self.rejoin_status.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_rejoin_status(&self, account: &str, status: &str) {
        self.rejoin_status.write().unwrap_or_else(|p| p.into_inner()).insert(account.to_owned(), status.to_owned());
    }

    pub fn clear_rejoin_status(&self, account: &str) {
        self.rejoin_status.write().unwrap_or_else(|p| p.into_inner()).remove(account);
    }

    pub fn instances(&self) -> Vec<Instance> {
        self.instances.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_instances(&self, list: Vec<Instance>) {
        *self.instances.write().unwrap_or_else(|p| p.into_inner()) = list;
    }

    pub fn update_available(&self) -> Option<String> {
        self.update_available.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_update_available(&self, version: Option<String>) {
        *self.update_available.write().unwrap_or_else(|p| p.into_inner()) = version;
    }

    pub fn toast(&self, message: &str, error: bool) {
        *self.toast.lock().unwrap_or_else(|p| p.into_inner()) = Some((message.to_owned(), error));
    }

    /// Takes the pending toast, if any.
    pub fn take_toast(&self) -> Option<(String, bool)> {
        self.toast.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

/// A background thread that can be asked to stop and joined.
pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(name: &str, stop: Arc<AtomicBool>, body: impl FnOnce() + Send + 'static) -> Worker {
        let handle = std::thread::Builder::new().name(name.to_owned()).spawn(body).ok();
        Worker { stop, handle }
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Sleeps in small steps so a stop flag is noticed quickly.
pub fn interruptible_sleep(stop: &AtomicBool, total: std::time::Duration) -> bool {
    let step = std::time::Duration::from_millis(200);
    let mut slept = std::time::Duration::ZERO;
    while slept < total {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        std::thread::sleep(step.min(total - slept));
        slept += step;
    }
    stop.load(Ordering::SeqCst)
}

/// Owns the shared `Live` state, the avatar cache and the running workers.
pub struct Services {
    pub core: Arc<Core>,
    pub live: Arc<Live>,
    pub avatars: Arc<avatars::AvatarCache>,
    workers: Mutex<Vec<Worker>>,
    repaint: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl Services {
    pub fn new(core: Arc<Core>) -> Arc<Services> {
        let cache_days = core.settings().avatar_cache_days;
        Arc::new(Services {
            core,
            live: Arc::new(Live::default()),
            avatars: Arc::new(avatars::AvatarCache::new(cache_days)),
            workers: Mutex::new(Vec::new()),
            repaint: Mutex::new(None),
        })
    }

    /// Installs a callback the workers use to wake the GUI for a repaint.
    pub fn set_repaint(&self, repaint: impl Fn() + Send + Sync + 'static) {
        *self.repaint.lock().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(repaint));
    }

    pub fn request_repaint(&self) {
        if let Some(repaint) = self.repaint.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            repaint();
        }
    }

    pub(crate) fn add_worker(&self, worker: Worker) {
        self.workers.lock().unwrap_or_else(|p| p.into_inner()).push(worker);
    }

    /// Starts the always-on background workers.
    pub fn start_background(self: &Arc<Self>) {
        activity::start(self);
        if self.core.settings().check_updates {
            updater::start_check(self);
        }
    }

    /// Stops every worker (called on shutdown).
    pub fn stop_all(&self) {
        let workers = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|p| p.into_inner()));
        for worker in workers {
            worker.stop();
        }
    }
}
