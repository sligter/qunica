#[path = "../src/runtime/group_scheduler/cancellation.rs"]
mod cancellation;

use std::sync::Arc;

use cancellation::{ActiveTurnRegistry, TurnCancellation};
use tokio::{
    sync::Barrier,
    time::{timeout, Duration},
};

#[tokio::test]
async fn cancelled_returns_when_cancellation_precedes_waiting() {
    let cancellation = TurnCancellation::new();
    cancellation.cancel();

    timeout(Duration::from_secs(1), cancellation.cancelled())
        .await
        .expect("pre-cancelled signal should return immediately");
    assert!(cancellation.is_cancelled());
}

#[tokio::test]
async fn cancellation_wakes_all_waiters() {
    const WAITER_COUNT: usize = 3;

    let cancellation = TurnCancellation::new();
    let barrier = Arc::new(Barrier::new(WAITER_COUNT + 1));
    let mut waiters = Vec::with_capacity(WAITER_COUNT);

    for _ in 0..WAITER_COUNT {
        let cancellation = cancellation.clone();
        let barrier = barrier.clone();
        waiters.push(tokio::spawn(async move {
            barrier.wait().await;
            cancellation.cancelled().await;
        }));
    }

    barrier.wait().await;
    tokio::task::yield_now().await;
    cancellation.cancel();

    for waiter in waiters {
        timeout(Duration::from_secs(1), waiter)
            .await
            .expect("waiter should be woken")
            .expect("waiter task should not panic");
    }
}

#[tokio::test]
async fn registry_does_not_cancel_or_remove_a_replacement_turn() {
    let registry = ActiveTurnRegistry::new();
    let first = registry.register("thread-1", "turn-1").await;
    let replacement = registry.register("thread-1", "turn-2").await;

    assert!(!registry.cancel("thread-1", "turn-1").await);
    assert!(!replacement.cancellation.is_cancelled());
    assert!(!registry.remove(&first).await);

    assert!(registry.cancel("thread-1", "turn-2").await);
    assert!(replacement.cancellation.is_cancelled());
    assert!(registry.remove(&replacement).await);
}
