use std::time::Duration;

use nori_tui_components::MotionBackground;
use nori_tui_components::MotionFormation;
use nori_tui_components::MotionPalette;
use nori_tui_components::MotionScene;
use nori_tui_components::MotionState;

/// Only the scene 1 <-> 2 zoom; no foreground widgets or reserved regions.
pub struct Story {
    motion: MotionState,
    pub formation: MotionFormation,
    leg_elapsed: Duration,
    pub zooming_in: bool,
}

impl Story {
    pub fn new(muster: bool) -> Self {
        let mut motion = MotionState::default();
        motion.advance(Duration::from_secs(12));
        motion.set_scene(MotionScene::Formations);
        Self {
            motion,
            formation: if muster {
                MotionFormation::Muster
            } else {
                MotionFormation::Platoon
            },
            leg_elapsed: Duration::ZERO,
            zooming_in: true,
        }
    }

    pub fn advance(&mut self, mut delta: Duration) {
        let leg = Duration::from_secs(3);
        while !delta.is_zero() {
            let step = delta.min(leg - self.leg_elapsed);
            self.motion.advance(step);
            self.leg_elapsed += step;
            delta -= step;
            if self.leg_elapsed == leg {
                self.leg_elapsed = Duration::ZERO;
                self.zooming_in = !self.zooming_in;
                self.motion.set_scene(if self.zooming_in {
                    MotionScene::Formations
                } else {
                    MotionScene::Dots
                });
            }
        }
    }

    pub fn phase(&self) -> f64 {
        let progress = self.leg_elapsed.as_secs_f64() / 3.0;
        if self.zooming_in {
            progress
        } else {
            1.0 - progress
        }
    }

    pub fn widget(&self) -> MotionBackground<'_> {
        MotionBackground::new(&self.motion)
            .palette(MotionPalette::nori())
            .formation(self.formation)
    }
}
