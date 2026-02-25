
Add this type of component called a "throbber" to the tui-components library, similar to the shimmer component.
```rs
            // Use legacy spinner animation
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner_text = format!(
                "{} {selected_agent} processing...",
                frames[model.loading_frame % frames.len()],
            );
            let spinner = Paragraph::new(spinner_text);
            frame.render_widget(spinner, shimmer_chunk);
```

Make sure to add snapshot tests and an example for the throbber, similar to @
