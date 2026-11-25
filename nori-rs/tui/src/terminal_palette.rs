use std::sync::Mutex;

use crossterm::style::Color;

type Rgb = (u8, u8, u8);

#[derive(Default)]
struct DefaultColors {
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

impl DefaultColors {
    fn update(&mut self, slot: u8, color: Option<Color>) -> bool {
        let Some(Color::Rgb { r, g, b }) = color else {
            return false;
        };
        let entry = match slot {
            10 => &mut self.fg,
            11 => &mut self.bg,
            _ => return false,
        };
        entry.replace((r, g, b)) != Some((r, g, b))
    }
}

// Rendering and one-shot commands can inspect the palette without reading the terminal.
static COLORS: Mutex<DefaultColors> = Mutex::new(DefaultColors { fg: None, bg: None });

pub(crate) fn record(slot: u8, color: Option<Color>) -> bool {
    COLORS
        .lock()
        .is_ok_and(|mut colors| colors.update(slot, color))
}

pub(crate) fn default_fg() -> Option<Rgb> {
    COLORS.lock().ok()?.fg
}

pub(crate) fn default_bg() -> Option<Rgb> {
    COLORS.lock().ok()?.bg
}

#[cfg(test)]
mod tests {
    use super::DefaultColors;
    use crossterm::style::Color;
    use pretty_assertions::assert_eq;

    #[test]
    fn color_reports_update_each_slot_independently() {
        let mut colors = DefaultColors::default();
        let light = Color::Rgb {
            r: 240,
            g: 239,
            b: 235,
        };
        let dark = Color::Rgb {
            r: 21,
            g: 22,
            b: 23,
        };
        assert!(colors.update(11, Some(light)));
        assert_eq!(colors.bg, Some((240, 239, 235)));
        assert_eq!(colors.fg, None);
        assert!(colors.update(10, Some(dark)));
        assert!(!colors.update(11, None));
        assert!(!colors.update(11, Some(light)));
        assert!(!colors.update(12, Some(dark)));
        assert_eq!(colors.bg, Some((240, 239, 235)));
        assert_eq!(colors.fg, Some((21, 22, 23)));
    }
}
