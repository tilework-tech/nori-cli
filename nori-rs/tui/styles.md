# Headers, primary, and secondary text

- **Headers:** Use `bold`. For markdown with various header levels, leave in the `#` signs.
- **Primary text:** Default.
- **Secondary text:** Use `dim`.

# Foreground colors

- **Default:** Most of the time, just use the default foreground color. `reset` can help get it back.
- **Pointers and focus:** Use ANSI `green` only on compact interaction signals such as row pointers, shortcut keys, focused borders, and full-screen overlay rails. Keep selected primary text in the default foreground.
- **Information:** Use ANSI `cyan` for targeted informational or secondary accents such as a marker, key, value, status, or compact cell. Do not wash supporting prose in cyan.
- **Success and additions:** Use ANSI `green`.
- **Errors, failures and deletions:** Use ANSI `red`.
- **Agent identity:** Use yellow/orange for Claude, white for Codex, blue for Gemini and Antigravity, and green for Nori. Apply identity colors only to explicitly typed category tabs or cells. The status block uses Handroll's Claude orange (`#ff9e64`) on the agent name only; supporting model, effort, and priority text stays in the default foreground.

# Avoid

- Avoid custom colors because there's no guarantee that they'll contrast well or look good in various terminal color themes. (`shimmer.rs` is an exception that works well because we take the default colors and just adjust their levels.)
- Avoid ANSI `black` and general-purpose `white` foregrounds because the default terminal theme color will do a better job. (Use `reset` if you need to in order to get those.) Explicit Codex identity and contrast over a manually colored background are exceptions.
- Avoid general-purpose ANSI `blue` and `yellow`. Explicit agent identity, warning, and other semantic tokens are exceptions.

(There are some rules to try to catch this in `clippy.toml`.)
