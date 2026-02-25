use crate::backends::is_available;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaScriptRuntime {
    Bun,
    Npm,
}

impl JavaScriptRuntime {
    pub fn command(&self) -> &'static str {
        match self {
            JavaScriptRuntime::Bun => "bunx",
            JavaScriptRuntime::Npm => "npx",
        }
    }
}

pub fn detect_javascript_runtime() -> Option<JavaScriptRuntime> {
    if is_available("bun") || is_available("bunx") {
        Some(JavaScriptRuntime::Bun)
    } else if is_available("npm") || is_available("npx") {
        Some(JavaScriptRuntime::Npm)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_some_when_runtime_available() {
        // This test checks that detection returns Some when at least one runtime exists
        // We can't guarantee which runtime is installed in the test environment,
        // but we can verify the behavior is correct
        let result = detect_javascript_runtime();

        // If bun/bunx is available, should return Bun
        if is_available("bun") || is_available("bunx") {
            assert_eq!(result, Some(JavaScriptRuntime::Bun));
        }
        // Else if npm/npx is available, should return Npm
        else if is_available("npm") || is_available("npx") {
            assert_eq!(result, Some(JavaScriptRuntime::Npm));
        }
        // Otherwise should return None
        else {
            assert_eq!(result, None);
        }
    }

    #[test]
    fn test_runtime_command_returns_correct_executable() {
        assert_eq!(JavaScriptRuntime::Bun.command(), "bunx");
        assert_eq!(JavaScriptRuntime::Npm.command(), "npx");
    }
}
