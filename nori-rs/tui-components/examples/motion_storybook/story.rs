use std::time::Duration;

use nori_tui_components::MotionBackground;
use nori_tui_components::MotionFormation;
use nori_tui_components::MotionPalette;
use nori_tui_components::MotionScene;

/// Example-owned timeline. Rendering accepts explicit time and transition progress.
pub struct Story {
    elapsed: Duration,
    pub formation: MotionFormation,
    pub zooming_in: bool,
}

impl Story {
    pub fn new(muster: bool) -> Self {
        Self {
            elapsed: Duration::ZERO,
            formation: if muster {
                MotionFormation::Muster
            } else {
                MotionFormation::Platoon
            },
            zooming_in: true,
        }
    }

    pub fn advance(&mut self, delta: Duration) {
        self.elapsed = self.elapsed.saturating_add(delta);
        self.zooming_in = self.elapsed.as_secs() % 6 < 3;
    }

    pub fn phase(&self) -> f64 {
        let leg_seconds =
            (self.elapsed.as_secs() % 3) as f64 + f64::from(self.elapsed.subsec_nanos()) / 1e9;
        let progress = leg_seconds / 3.0;
        if self.zooming_in {
            progress
        } else {
            1.0 - progress
        }
    }

    pub fn widget(&self) -> MotionBackground {
        MotionBackground::new(
            MotionScene::Dots,
            self.elapsed.saturating_add(Duration::from_secs(12)),
        )
        .transition_to(MotionScene::Formations, self.phase())
        .palette(MotionPalette::nori())
        .formation(self.formation)
    }
}
