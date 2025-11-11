use nori_cli::backends;

#[test]
fn test_backend_availability_check_installed() {
    // sh is always present on Unix systems
    assert!(
        backends::is_available("sh"),
        "sh should be detected as available"
    );
}

#[test]
fn test_backend_availability_check_missing() {
    // Extremely unlikely to exist
    assert!(
        !backends::is_available("nonexistent-command-xyz-12345"),
        "nonexistent command should not be detected as available"
    );
}
