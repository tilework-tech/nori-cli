use nori_acp::find_nori_home;

#[test]
fn nori_acp_crate_is_available_to_cli() {
    let _ = find_nori_home as fn() -> anyhow::Result<std::path::PathBuf>;
}
