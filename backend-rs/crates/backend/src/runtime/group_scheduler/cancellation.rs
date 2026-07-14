use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tokio::sync::{Mutex, Notify};

/// A clonable cancellation signal for one scheduler turn.
#[derive(Clone, Debug)]
pub struct TurnCancellation {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl TurnCancellation {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Request cancellation and wake every task waiting on this turn.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Wait until cancellation is requested.
    pub async fn cancelled(&self) {
        loop {
            // Construct the waiter before checking the flag so a racing
            // `notify_waiters` call cannot be missed.
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
            if self.is_cancelled() {
                return;
            }
        }
    }

    fn same_signal(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancelled, &other.cancelled) && Arc::ptr_eq(&self.notify, &other.notify)
    }
}

impl Default for TurnCancellation {
    fn default() -> Self {
        Self::new()
    }
}

/// The in-memory handle for a scheduler turn currently running on a thread.
#[derive(Clone, Debug)]
pub struct ActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
    pub cancellation: TurnCancellation,
}

impl ActiveTurn {
    pub fn new(thread_id: impl Into<String>, turn_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            cancellation: TurnCancellation::new(),
        }
    }

    fn is_same_registration(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id
            && self.turn_id == other.turn_id
            && self.cancellation.same_signal(&other.cancellation)
    }
}

/// Tracks the currently active scheduler turn for each thread.
#[derive(Clone, Debug, Default)]
pub struct ActiveTurnRegistry {
    active_turns: Arc<Mutex<HashMap<String, ActiveTurn>>>,
}

impl ActiveTurnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a turn as active and return the handle used by that turn.
    pub async fn register(
        &self,
        thread_id: impl Into<String>,
        turn_id: impl Into<String>,
    ) -> ActiveTurn {
        let active_turn = ActiveTurn::new(thread_id, turn_id);
        self.active_turns
            .lock()
            .await
            .insert(active_turn.thread_id.clone(), active_turn.clone());
        active_turn
    }

    /// Signal the current registration only when it has the requested turn ID.
    ///
    /// Returning `false` means the thread is inactive or has been replaced by
    /// a different turn.
    pub async fn cancel(&self, thread_id: &str, turn_id: &str) -> bool {
        let cancellation = self
            .active_turns
            .lock()
            .await
            .get(thread_id)
            .filter(|active_turn| active_turn.turn_id == turn_id)
            .map(|active_turn| active_turn.cancellation.clone());

        let Some(cancellation) = cancellation else {
            return false;
        };
        cancellation.cancel();
        true
    }

    /// Remove a completed turn only if it is still the current registration.
    ///
    /// The registration identity check prevents delayed cleanup from removing
    /// a replacement turn for the same thread.
    pub async fn remove(&self, active_turn: &ActiveTurn) -> bool {
        let mut active_turns = self.active_turns.lock().await;
        let is_current = active_turns
            .get(&active_turn.thread_id)
            .is_some_and(|current| current.is_same_registration(active_turn));
        if !is_current {
            return false;
        }
        active_turns.remove(&active_turn.thread_id);
        true
    }
}
