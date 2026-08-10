pub fn requery_default_colors() {
    imp::requery_default_colors();
}

#[derive(Clone, Copy)]
pub struct DefaultColors {
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
}

pub fn default_colors() -> Option<DefaultColors> {
    imp::default_colors()
}

pub fn default_fg() -> Option<(u8, u8, u8)> {
    default_colors().map(|colors| colors.fg)
}

pub fn default_bg() -> Option<(u8, u8, u8)> {
    default_colors().map(|colors| colors.bg)
}

#[cfg(all(unix, not(test)))]
mod imp {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    use crossterm::style::Color as CrosstermColor;
    use crossterm::style::query_background_color;
    use crossterm::style::query_foreground_color;

    use super::DefaultColors;

    struct Cache<T> {
        attempted: bool,
        value: Option<T>,
    }

    impl<T> Default for Cache<T> {
        fn default() -> Self {
            Self {
                attempted: false,
                value: None,
            }
        }
    }

    impl<T: Copy> Cache<T> {
        fn get_or_init_with(&mut self, mut init: impl FnMut() -> Option<T>) -> Option<T> {
            if !self.attempted {
                self.value = init();
                self.attempted = true;
            }
            self.value
        }

        fn refresh_with(&mut self, mut init: impl FnMut() -> Option<T>) -> Option<T> {
            self.value = init();
            self.attempted = true;
            self.value
        }
    }

    fn default_colors_cache() -> &'static Mutex<Cache<DefaultColors>> {
        static CACHE: OnceLock<Mutex<Cache<DefaultColors>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(Cache::default()))
    }

    pub(super) fn default_colors() -> Option<DefaultColors> {
        let cache = default_colors_cache();
        let mut cache = cache.lock().ok()?;
        cache.get_or_init_with(|| query_default_colors().unwrap_or_default())
    }

    pub(super) fn requery_default_colors() {
        if let Ok(mut cache) = default_colors_cache().lock() {
            // Don't try to refresh if the cache is already attempted and failed.
            if cache.attempted && cache.value.is_none() {
                return;
            }
            cache.refresh_with(|| query_default_colors().unwrap_or_default());
        }
    }

    fn query_default_colors() -> std::io::Result<Option<DefaultColors>> {
        let fg = query_foreground_color()?.and_then(color_to_tuple);
        let bg = query_background_color()?.and_then(color_to_tuple);
        Ok(fg.zip(bg).map(|(fg, bg)| DefaultColors { fg, bg }))
    }

    fn color_to_tuple(color: CrosstermColor) -> Option<(u8, u8, u8)> {
        match color {
            CrosstermColor::Rgb { r, g, b } => Some((r, g, b)),
            _ => None,
        }
    }
}

#[cfg(not(all(unix, not(test))))]
mod imp {
    use super::DefaultColors;

    pub(super) fn default_colors() -> Option<DefaultColors> {
        None
    }

    pub(super) fn requery_default_colors() {}
}
