//! Runs blocking work (network, launches) off the UI thread and delivers the
//! result back as a message the app drains each frame.

use crate::core::Core;
use crate::services::Services;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// Messages background tasks post back to the UI.
pub enum Msg {
    /// A finished operation with a toast to show (error flag second).
    Toast(String, bool),
    /// The account list should be reloaded from the vault (revision bumped).
    AccountsChanged,
    /// A game name resolved for a place id (place_id, name).
    GameName(u64, String),
    /// Private servers loaded for the Private Servers page.
    PrivateServers(crate::error::AppResult<Vec<crate::store::model::SavedPrivateServer>>),
    /// A quick sign-in produced a code to display, or finished.
    QuickLogin(QuickLoginMsg),
    /// An arbitrary UI callback to run on the main thread.
    Run(Box<dyn FnOnce(&mut super::NovaApp) + Send>),
}

pub enum QuickLoginMsg {
    Code(String),
    Added(String),
    Failed(String),
}

pub struct Tasks {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
}

impl Tasks {
    pub fn new(ctx: egui::Context) -> Tasks {
        let (tx, rx) = std::sync::mpsc::channel();
        Tasks { tx, rx, ctx }
    }

    pub fn sender(&self) -> TaskSender {
        TaskSender { tx: self.tx.clone(), ctx: self.ctx.clone() }
    }

    pub fn drain(&self) -> Vec<Msg> {
        self.rx.try_iter().collect()
    }
}

/// A cloneable handle for posting messages and spawning tasks.
#[derive(Clone)]
pub struct TaskSender {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl TaskSender {
    pub fn post(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        self.ctx.request_repaint();
    }

    pub fn toast(&self, message: impl Into<String>) {
        self.post(Msg::Toast(message.into(), false));
    }

    pub fn error(&self, message: impl Into<String>) {
        self.post(Msg::Toast(message.into(), true));
    }

    /// Runs `work` on a worker thread; its returned message is delivered to the UI.
    pub fn spawn(&self, core: Arc<Core>, services: Arc<Services>, work: impl FnOnce(&Core, &Services) -> Msg + Send + 'static) {
        let sender = self.clone();
        std::thread::Builder::new()
            .name("ui-task".into())
            .spawn(move || {
                let msg = work(&core, &services);
                sender.post(msg);
            })
            .ok();
    }
}
