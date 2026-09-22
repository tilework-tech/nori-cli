//! Optional linear transition controller; rendering also accepts explicit time and progress.

use std::time::Duration;

use super::MotionBackground;
use super::MotionScene;

/// Optional caller-held animation clock and interruptible scene transition.
///
/// Adjacent scenes take three seconds to morph. No wall clock, background task,
/// random source, or input handling is hidden in this state. Pause by not calling
/// [`Self::advance`]. Reduced motion freezes scenery and switches scenes instantly.
#[derive(Clone, Debug, Default)]
pub struct MotionState {
    scene: MotionScene,
    position: f64,
    elapsed: Duration,
    reduced_motion: bool,
}

impl MotionState {
    pub fn new(scene: MotionScene) -> Self {
        Self {
            scene,
            position: scene.position(),
            ..Self::default()
        }
    }

    /// The requested destination, including while a transition is in progress.
    pub fn scene(&self) -> MotionScene {
        self.scene
    }

    pub fn set_scene(&mut self, scene: MotionScene) {
        self.scene = scene;
        if self.reduced_motion {
            self.position = scene.position();
        }
    }

    pub fn set_reduced_motion(&mut self, reduced: bool) {
        self.reduced_motion = reduced;
        if reduced {
            self.position = self.scene.position();
        }
    }

    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }

    /// Snapshot the current frame. Subsequent state changes do not affect it.
    /// Reduced motion preserves the controller's frozen ambient time.
    pub fn background(&self) -> MotionBackground {
        MotionBackground {
            position: self.position,
            ..MotionBackground::new(self.scene, self.elapsed)
        }
    }

    pub fn advance(&mut self, delta: Duration) {
        if self.reduced_motion {
            return;
        }
        self.elapsed = self.elapsed.saturating_add(delta);
        let distance = self.scene.position() - self.position;
        self.position += distance.signum() * distance.abs().min(delta.as_secs_f64() / 3.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    fn render(state: &MotionState, area: Rect) -> Buffer {
        let mut buffer = Buffer::empty(area);
        state.background().render(area, &mut buffer);
        buffer
    }
    #[test]
    fn transitions_reverse_from_the_current_frame_and_finish_without_overshoot() {
        let mut state = MotionState::default();
        state.set_scene(MotionScene::Blueprint);
        state.advance(Duration::from_secs(2));
        assert_eq!(state.position, 2.0 / 3.0);
        let before = state.position;
        state.set_scene(MotionScene::Dots);
        assert_eq!(state.position, before);
        state.advance(Duration::from_secs(1));
        assert_eq!(state.position, 1.0 / 3.0);
        state.advance(Duration::from_secs(100));
        assert_eq!((state.position, state.scene()), (0.0, MotionScene::Dots));
        state.set_scene(MotionScene::Blueprint);
        state.advance(Duration::MAX);
        assert_eq!(
            (state.position, state.scene()),
            (4.0, MotionScene::Blueprint)
        );
        // Very long-running clocks remain safe for field hash/index conversions.
        render(&state, Rect::new(0, 0, 20, 10));
    }

    #[test]
    fn reduced_motion_freezes_the_entire_field_and_makes_scene_changes_immediate() {
        let area = Rect::new(0, 0, 60, 20);
        let mut state = MotionState::new(MotionScene::Creatures);
        state.advance(Duration::from_secs(12));
        state.set_reduced_motion(true);
        let before = render(&state, area);
        state.advance(Duration::from_secs(50));
        assert_eq!(render(&state, area), before);
        state.set_scene(MotionScene::Blueprint);
        assert_eq!(state.position, 4.0);
        state.set_reduced_motion(false);
        state.advance(Duration::from_secs(1));
        assert_eq!(state.elapsed, Duration::from_secs(13));
    }

    #[test]
    fn background_is_an_owned_snapshot_of_the_controller() {
        let mut state = MotionState::default();
        let widget = state.background();
        let before = render(&state, Rect::new(0, 0, 24, 12));
        state.set_scene(MotionScene::Blueprint);
        state.advance(Duration::from_secs(20));
        let mut actual = Buffer::empty(before.area);
        widget.render(actual.area, &mut actual);
        assert_eq!(actual, before);
        assert_ne!(render(&state, before.area), before);
    }
}
