//! Coalesces frame requests into draws.

use std::time::Duration;
use std::time::Instant;

/// The shortest time between two draws, about 120 frames a second. A burst
/// of updates, such as a long history replayed on attach, then draws a few
/// frames instead of one per update.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// Spawn background scheduler to coalesce frame requests and emit draws at deadlines.
pub(crate) fn spawn_frame_scheduler(
    frame_schedule_rx: tokio::sync::mpsc::UnboundedReceiver<Instant>,
    draw_tx: tokio::sync::broadcast::Sender<()>,
) {
    tokio::spawn(async move {
        use tokio::select;
        use tokio::time::Instant as TokioInstant;
        use tokio::time::sleep_until;

        let mut rx = frame_schedule_rx;
        let mut next_deadline: Option<Instant> = None;
        let mut last_draw: Option<Instant> = None;

        loop {
            let target = next_deadline
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(60 * 60 * 24 * 365));
            let sleep_fut = sleep_until(TokioInstant::from_std(target));
            tokio::pin!(sleep_fut);

            select! {
                recv = rx.recv() => {
                    match recv {
                        Some(at) => {
                            let at = last_draw
                                .map(|drawn| drawn + MIN_FRAME_INTERVAL)
                                .filter(|earliest| *earliest > at)
                                .unwrap_or(at);
                            if next_deadline.is_none_or(|cur| at < cur) {
                                next_deadline = Some(at);
                            }
                            // Do not send a draw immediately here. By continuing the loop,
                            // we recompute the sleep target so the draw fires once via the
                            // sleep branch, coalescing multiple requests into a single draw.
                            continue;
                        }
                        None => break,
                    }
                }
                _ = &mut sleep_fut => {
                    if next_deadline.is_some() {
                        next_deadline = None;
                        last_draw = Some(Instant::now());
                        let _ = draw_tx.send(());
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::spawn_frame_scheduler;

    #[tokio::test]
    async fn requests_faster_than_the_frame_interval_draw_at_a_bounded_rate() {
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
        let (draw_tx, mut draw_rx) = tokio::sync::broadcast::channel(1024);
        spawn_frame_scheduler(request_rx, draw_tx);

        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(200) {
            request_tx.send(Instant::now()).unwrap();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut draws = 0;
        while draw_rx.try_recv().is_ok() {
            draws += 1;
        }
        assert!((1..=40).contains(&draws), "{draws} draws in 250 ms");
    }

    #[tokio::test]
    async fn a_lone_request_draws_at_once() {
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();
        let (draw_tx, mut draw_rx) = tokio::sync::broadcast::channel(16);
        spawn_frame_scheduler(request_rx, draw_tx);

        let asked = Instant::now();
        request_tx.send(asked).unwrap();
        draw_rx.recv().await.unwrap();

        assert!(
            asked.elapsed() < Duration::from_millis(20),
            "{:?}",
            asked.elapsed()
        );
    }
}
