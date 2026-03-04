use super::*;
use std::fs;
use tempfile::TempDir;

fn render_lines(lines: &[Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn read_nori_profile_finds_ancestor_config() {
    // Create a temp directory structure:
    // /tmp/xxx/
    //   .nori-config.json  (with profile)
    //   subdir/
    //     nested/  <- cwd
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create nested directory structure
    let nested = root.join("subdir/nested");
    fs::create_dir_all(&nested).expect("create nested dirs");

    // Create .nori-config.json at root with profile
    let config_content = r#"{
        "profile": {
            "baseProfile": "test-profile"
        }
    }"#;
    fs::write(root.join(".nori-config.json"), config_content).expect("write config");

    // Call read_nori_profile with nested directory as cwd
    let profile = read_nori_profile(&nested);

    assert_eq!(
        profile,
        Some("test-profile".to_string()),
        "Should find profile in ancestor .nori-config.json"
    );
}

#[test]
fn read_nori_profile_returns_none_when_no_config() {
    let tmp = TempDir::new().expect("tempdir");
    let profile = read_nori_profile(tmp.path());
    assert_eq!(
        profile, None,
        "Should return None when no config file exists"
    );
}

#[test]
fn discover_finds_all_ancestors_with_new_function() {
    // Create a temp directory structure with instruction files:
    // /tmp/xxx/
    //   .git  (to mark git root)
    //   AGENTS.md
    //   .claude/
    //     CLAUDE.md  (only specific files are found, not arbitrary .md)
    //   subdir/
    //     CLAUDE.md
    //     nested/  <- cwd
    //       AGENTS.md
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create .git to mark root
    fs::write(root.join(".git"), "gitdir: /path/to/git").expect("write .git");

    // Create instruction files at various levels
    fs::write(root.join("AGENTS.md"), "root agents").expect("write root AGENTS.md");
    fs::create_dir_all(root.join(".claude")).expect("create .claude dir");
    fs::write(root.join(".claude/CLAUDE.md"), "claude hidden").expect("write .claude/CLAUDE.md");

    let subdir = root.join("subdir");
    fs::create_dir_all(&subdir).expect("create subdir");
    fs::write(subdir.join("CLAUDE.md"), "subdir claude").expect("write subdir CLAUDE.md");

    let nested = subdir.join("nested");
    fs::create_dir_all(&nested).expect("create nested");
    fs::write(nested.join("AGENTS.md"), "nested agents").expect("write nested AGENTS.md");

    // Call discover_all_instruction_files_with_home with None home to avoid real home configs
    let files = discover_all_instruction_files_with_home(&nested, None, None);

    // Should find files in order from root to cwd:
    // 1. root/AGENTS.md
    // 2. root/.claude/CLAUDE.md
    // 3. subdir/CLAUDE.md
    // 4. nested/AGENTS.md
    assert_eq!(files.len(), 4, "Should find 4 instruction files");

    // Verify paths contain expected files
    let file_names: Vec<String> = files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(file_names.contains(&"AGENTS.md".to_string()));
    assert!(file_names.contains(&"CLAUDE.md".to_string()));
}

#[test]
fn discover_returns_empty_when_none_exist() {
    let tmp = TempDir::new().expect("tempdir");
    // Use None home to avoid picking up real home directory configs
    let files = discover_all_instruction_files_with_home(tmp.path(), None, None);
    assert!(
        files.is_empty(),
        "Should return empty vec when no instruction files exist"
    );
}

#[test]
fn nori_header_renders_instruction_files() {
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "test-agent".to_string(),
        directory: PathBuf::from("/tmp/test"),
        nori_profile: Some("test-profile".to_string()),
        instruction_files: vec![
            InstructionFile {
                path: PathBuf::from("/home/user/project/AGENTS.md"),
                active: true,
                token_count: Some(TokenCount {
                    count: 500,
                    approximate: true,
                }),
            },
            InstructionFile {
                path: PathBuf::from("/home/user/project/.claude/rules.md"),
                active: false,
                token_count: None,
            },
        ],
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should show instruction files section
    assert!(
        rendered.contains("Instruction Files"),
        "Should show 'Instruction Files' section header"
    );
}

#[test]
fn nori_header_renders_correctly() {
    let cell = NoriSessionHeaderCell::new("test-agent".to_string(), PathBuf::from("/tmp/test"));

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should contain simple "Nori" title (not ASCII art)
    assert!(
        rendered.contains("Nori"),
        "Header should contain Nori title"
    );

    // Should contain version in the title line
    assert!(rendered.contains(" v"), "Should show version prefix");

    // Should contain directory
    assert!(
        rendered.contains("directory:"),
        "Should show directory label"
    );

    // Should contain agent
    assert!(rendered.contains("agent:"), "Should show agent label");
    assert!(rendered.contains("test-agent"), "Should show agent name");

    // Should contain skillset
    assert!(rendered.contains("skillset:"), "Should show skillset label");
}

#[test]
fn nori_profile_shows_none_when_not_set() {
    // Create cell without a real config file
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "test-agent".to_string(),
        directory: PathBuf::from("/tmp/test"),
        nori_profile: None,
        instruction_files: Vec::new(),
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    assert!(
        rendered.contains("(none)"),
        "Should show (none) when profile not set"
    );
}

#[test]
fn nori_profile_shows_value_when_set() {
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "test-agent".to_string(),
        directory: PathBuf::from("/tmp/test"),
        nori_profile: Some("senior-swe".to_string()),
        instruction_files: Vec::new(),
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    assert!(
        rendered.contains("senior-swe"),
        "Should show profile name when set"
    );
}

#[test]
fn nori_header_snapshot() {
    let cell = NoriSessionHeaderCell {
        version: "0.1.0",
        agent: "claude-sonnet".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("senior-swe".to_string()),
        instruction_files: vec![
            InstructionFile {
                path: PathBuf::from("/home/user/project/AGENTS.md"),
                active: false,
                token_count: None,
            },
            InstructionFile {
                path: PathBuf::from("/home/user/project/.claude/settings.md"),
                active: true,
                token_count: Some(TokenCount {
                    count: 2450,
                    approximate: true,
                }),
            },
        ],
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    insta::assert_snapshot!(rendered);
}

#[test]
fn nori_status_output_shows_status_command_and_nori_branding() {
    let status_cell = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        None,
        None,
        None,
        None,
    );

    let lines = status_cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should show /status command echo
    assert!(
        rendered.contains("/status"),
        "Status output should show /status command"
    );

    // Should contain Nori branding
    assert!(
        rendered.contains("Nori"),
        "Status output should contain Nori branding"
    );

    // Should NOT contain OpenAI Codex branding
    assert!(
        !rendered.contains("OpenAI"),
        "Status output should NOT contain OpenAI branding"
    );
    assert!(
        !rendered.contains("Codex"),
        "Status output should NOT contain Codex branding"
    );

    // Should show directory and agent info
    assert!(
        rendered.contains("directory:"),
        "Status output should show directory"
    );
    assert!(
        rendered.contains("agent:"),
        "Status output should show agent"
    );
    assert!(
        rendered.contains("claude-sonnet"),
        "Status output should show agent name"
    );
}

// =========================================================================
// NEW TESTS: Agent-specific instruction file discovery and activation
// =========================================================================

#[test]
fn detect_agent_kind_from_model_string() {
    // Test Claude variants
    assert_eq!(
        detect_agent_kind("claude-code"),
        Some(AgentKindSimple::Claude)
    );
    assert_eq!(
        detect_agent_kind("claude-sonnet"),
        Some(AgentKindSimple::Claude)
    );
    assert_eq!(
        detect_agent_kind("claude-opus-4"),
        Some(AgentKindSimple::Claude)
    );

    // Test Codex variants
    assert_eq!(detect_agent_kind("codex"), Some(AgentKindSimple::Codex));
    assert_eq!(
        detect_agent_kind("codex-mini"),
        Some(AgentKindSimple::Codex)
    );

    // Test Gemini variants
    assert_eq!(detect_agent_kind("gemini"), Some(AgentKindSimple::Gemini));
    assert_eq!(
        detect_agent_kind("gemini-cli"),
        Some(AgentKindSimple::Gemini)
    );
    assert_eq!(
        detect_agent_kind("gemini-2.0-flash"),
        Some(AgentKindSimple::Gemini)
    );

    // Test unknown
    assert_eq!(detect_agent_kind("gpt-4"), None);
    assert_eq!(detect_agent_kind("unknown-model"), None);
}

#[test]
fn discover_all_instruction_file_types() {
    // Create a temp directory structure with ALL instruction file types:
    // /tmp/xxx/
    //   .git
    //   CLAUDE.md
    //   CLAUDE.local.md
    //   .claude/CLAUDE.md
    //   AGENTS.md
    //   AGENTS.override.md
    //   GEMINI.md
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // Create .git to mark root
    fs::write(root.join(".git"), "gitdir").expect("write .git");

    // Create all instruction file types
    fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
    fs::write(root.join("CLAUDE.local.md"), "claude local").expect("write CLAUDE.local.md");
    fs::create_dir_all(root.join(".claude")).expect("create .claude");
    fs::write(root.join(".claude/CLAUDE.md"), "hidden claude").expect("write .claude/CLAUDE.md");
    fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
    fs::write(root.join("AGENTS.override.md"), "agents override")
        .expect("write AGENTS.override.md");
    fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");

    let files = discover_all_instruction_files_with_home(root, None, None);

    // Should find all 7 files
    let paths: Vec<String> = files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        paths.contains(&"CLAUDE.md".to_string()),
        "Should find CLAUDE.md"
    );
    assert!(
        paths.contains(&"CLAUDE.local.md".to_string()),
        "Should find CLAUDE.local.md"
    );
    assert!(
        paths.iter().any(|p| p == "CLAUDE.md"),
        "Should find .claude/CLAUDE.md"
    );
    assert!(
        paths.contains(&"AGENTS.md".to_string()),
        "Should find AGENTS.md"
    );
    assert!(
        paths.contains(&"AGENTS.override.md".to_string()),
        "Should find AGENTS.override.md"
    );
    assert!(
        paths.contains(&"GEMINI.md".to_string()),
        "Should find GEMINI.md"
    );

    // Check we found the hidden variant by checking full path
    let has_hidden_claude = files
        .iter()
        .any(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
    assert!(
        has_hidden_claude,
        "Should find .claude/CLAUDE.md hidden variant"
    );
}

#[test]
fn claude_activation_algorithm_activates_all_claude_files() {
    // Claude should activate: .claude/CLAUDE.md, CLAUDE.md, CLAUDE.local.md
    // (all three per directory, not exclusive)
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
    fs::write(root.join("CLAUDE.local.md"), "claude local").expect("write CLAUDE.local.md");
    fs::create_dir_all(root.join(".claude")).expect("create .claude");
    fs::write(root.join(".claude/CLAUDE.md"), "hidden claude").expect("write .claude/CLAUDE.md");
    fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
    fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");

    // Use None home to avoid picking up real home directory configs
    let files = discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Claude), None);

    // All Claude files should be active
    let claude_files: Vec<_> = files
        .iter()
        .filter(|f| {
            let name = f.path.file_name().unwrap().to_string_lossy();
            name.contains("CLAUDE")
        })
        .collect();

    assert_eq!(claude_files.len(), 3, "Should find 3 Claude files");
    for f in &claude_files {
        assert!(f.active, "Claude file {:?} should be active", f.path);
    }

    // AGENTS.md and GEMINI.md should NOT be active
    let agents_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
        .expect("Should find AGENTS.md");
    assert!(
        !agents_file.active,
        "AGENTS.md should NOT be active for Claude agent"
    );

    let gemini_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "GEMINI.md")
        .expect("Should find GEMINI.md");
    assert!(
        !gemini_file.active,
        "GEMINI.md should NOT be active for Claude agent"
    );
}

#[test]
fn codex_activation_prefers_override_over_regular() {
    // Codex should activate: AGENTS.override.md OR AGENTS.md (preferring override)
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
    fs::write(root.join("AGENTS.override.md"), "agents override")
        .expect("write AGENTS.override.md");
    fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");

    let files = discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Codex), None);

    // AGENTS.override.md should be active (preferred over AGENTS.md)
    let override_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.override.md")
        .expect("Should find AGENTS.override.md");
    assert!(override_file.active, "AGENTS.override.md should be active");

    // AGENTS.md should NOT be active when override exists
    let agents_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
        .expect("Should find AGENTS.md");
    assert!(
        !agents_file.active,
        "AGENTS.md should NOT be active when override exists"
    );

    // CLAUDE.md should NOT be active
    let claude_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "CLAUDE.md")
        .expect("Should find CLAUDE.md");
    assert!(
        !claude_file.active,
        "CLAUDE.md should NOT be active for Codex agent"
    );
}

#[test]
fn codex_activation_falls_back_to_regular_when_no_override() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");
    // No AGENTS.override.md

    let files = discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Codex), None);

    let agents_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
        .expect("Should find AGENTS.md");
    assert!(
        agents_file.active,
        "AGENTS.md should be active when no override exists"
    );
}

#[test]
fn gemini_activation_only_activates_gemini_files() {
    // Gemini should only activate GEMINI.md files (no hidden variants, no overrides)
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("GEMINI.md"), "gemini").expect("write GEMINI.md");
    fs::write(root.join("CLAUDE.md"), "claude").expect("write CLAUDE.md");
    fs::write(root.join("AGENTS.md"), "agents").expect("write AGENTS.md");

    let files = discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Gemini), None);

    let gemini_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "GEMINI.md")
        .expect("Should find GEMINI.md");
    assert!(gemini_file.active, "GEMINI.md should be active");

    // Other files should NOT be active
    for f in &files {
        let name = f.path.file_name().unwrap().to_string_lossy();
        if name != "GEMINI.md" {
            assert!(!f.active, "{name} should NOT be active for Gemini agent");
        }
    }
}

#[test]
fn discovery_traverses_directory_hierarchy() {
    // Test that discovery walks from git root to cwd
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("CLAUDE.md"), "root claude").expect("write root CLAUDE.md");

    let subdir = root.join("subdir");
    fs::create_dir_all(&subdir).expect("create subdir");
    fs::write(subdir.join("CLAUDE.md"), "subdir claude").expect("write subdir CLAUDE.md");

    let nested = subdir.join("nested");
    fs::create_dir_all(&nested).expect("create nested");
    fs::write(nested.join("CLAUDE.local.md"), "nested local")
        .expect("write nested CLAUDE.local.md");

    // Discover from nested directory (use None home to avoid real home configs)
    let files =
        discover_all_instruction_files_with_home(&nested, Some(AgentKindSimple::Claude), None);

    // Should find files from all levels
    assert_eq!(files.len(), 3, "Should find 3 files across hierarchy");

    // All should be active for Claude
    for f in &files {
        assert!(f.active, "File {:?} should be active for Claude", f.path);
    }
}

#[test]
fn header_renders_instruction_files_section() {
    let files = vec![
        InstructionFile {
            path: PathBuf::from("/home/user/.claude/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 1000,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 500,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/AGENTS.md"),
            active: false,
            token_count: None,
        },
    ];

    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "claude-code".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("test-profile".to_string()),
        instruction_files: files,
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should have "Instruction Files" section header
    assert!(
        rendered.contains("Instruction Files"),
        "Should show 'Instruction Files' section header"
    );

    // Should show file paths
    assert!(
        rendered.contains("CLAUDE.md"),
        "Should show CLAUDE.md in output"
    );

    // Should show token counts for active files
    assert!(
        rendered.contains("~1,000 tokens"),
        "Should show token count for first active file"
    );
    assert!(
        rendered.contains("~500 tokens"),
        "Should show token count for second active file"
    );

    // Should show total line
    assert!(rendered.contains("total"), "Should show total line");
    assert!(
        rendered.contains("~1,500 tokens"),
        "Should show combined total"
    );
}

#[test]
fn header_renders_exact_token_counts_without_tilde() {
    let files = vec![InstructionFile {
        path: PathBuf::from("/home/user/project/AGENTS.md"),
        active: true,
        token_count: Some(TokenCount {
            count: 750,
            approximate: false,
        }),
    }];

    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "codex".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: None,
        instruction_files: files,
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Exact counts should NOT have tilde prefix
    assert!(
        rendered.contains("750 tokens"),
        "Should show exact token count: {rendered}"
    );
    assert!(
        !rendered.contains("~750"),
        "Exact count should not have tilde prefix"
    );
}

#[test]
fn discovery_populates_token_counts_for_active_files() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    fs::write(root.join(".git"), "gitdir").expect("write .git");
    fs::write(root.join("CLAUDE.md"), "hello world test content").expect("write CLAUDE.md");
    fs::write(root.join("AGENTS.md"), "agent instructions here").expect("write AGENTS.md");

    // Claude agent: CLAUDE.md is active, AGENTS.md is not
    let files = discover_all_instruction_files_with_home(root, Some(AgentKindSimple::Claude), None);

    let claude_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "CLAUDE.md")
        .expect("Should find CLAUDE.md");
    assert!(claude_file.active);
    assert!(
        claude_file.token_count.is_some(),
        "Active file should have token count"
    );
    assert!(
        claude_file.token_count.as_ref().unwrap().count > 0,
        "Token count should be positive"
    );

    let agents_file = files
        .iter()
        .find(|f| f.path.file_name().unwrap().to_string_lossy() == "AGENTS.md")
        .expect("Should find AGENTS.md");
    assert!(!agents_file.active);
    assert!(
        agents_file.token_count.is_none(),
        "Inactive file should not have token count"
    );
}

// =========================================================================
// HOME CONFIG DISCOVERY TESTS
// =========================================================================

#[test]
fn discover_finds_claude_home_config() {
    // Test that discovery finds ~/.claude/CLAUDE.md for Claude agents
    // Structure:
    //   fake_home/
    //     .claude/
    //       CLAUDE.md  <- user-level config
    //   project/
    //     .git
    //     CLAUDE.md  <- project-level config
    let tmp = TempDir::new().expect("tempdir");
    let fake_home = tmp.path().join("fake_home");
    let project = tmp.path().join("project");

    // Create fake home with .claude/CLAUDE.md
    fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
    fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
        .expect("write user CLAUDE.md");

    // Create project with .git and CLAUDE.md
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join(".git"), "gitdir").expect("write .git");
    fs::write(project.join("CLAUDE.md"), "project claude config").expect("write project CLAUDE.md");

    // Discover files with custom home
    let files = discover_all_instruction_files_with_home(
        &project,
        Some(AgentKindSimple::Claude),
        Some(&fake_home),
    );

    // Should find both user-level and project-level config
    assert!(
        files.len() >= 2,
        "Should find at least 2 files (user and project): found {}",
        files.len()
    );

    // Should find the home config
    let has_home_config = files
        .iter()
        .any(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
    assert!(has_home_config, "Should find ~/.claude/CLAUDE.md");

    // Home config should be active for Claude
    let home_file = files
        .iter()
        .find(|f| {
            f.path.to_string_lossy().contains("fake_home")
                && f.path.to_string_lossy().contains(".claude/CLAUDE.md")
        })
        .expect("Should find home CLAUDE.md");
    assert!(
        home_file.active,
        "Home CLAUDE.md should be active for Claude"
    );
}

#[test]
fn discover_finds_codex_home_config() {
    // Test that discovery finds ~/.codex/AGENTS.md for Codex agents
    let tmp = TempDir::new().expect("tempdir");
    let fake_home = tmp.path().join("fake_home");
    let project = tmp.path().join("project");

    // Create fake home with .codex/AGENTS.md
    fs::create_dir_all(fake_home.join(".codex")).expect("create .codex");
    fs::write(fake_home.join(".codex/AGENTS.md"), "user codex config")
        .expect("write user AGENTS.md");

    // Create project with .git
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join(".git"), "gitdir").expect("write .git");

    // Discover files with custom home
    let files = discover_all_instruction_files_with_home(
        &project,
        Some(AgentKindSimple::Codex),
        Some(&fake_home),
    );

    // Should find the home config
    let has_home_config = files
        .iter()
        .any(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"));
    assert!(has_home_config, "Should find ~/.codex/AGENTS.md");

    // Home config should be active for Codex
    let home_file = files
        .iter()
        .find(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"))
        .expect("Should find home AGENTS.md");
    assert!(
        home_file.active,
        "Home AGENTS.md should be active for Codex"
    );
}

#[test]
fn discover_finds_gemini_home_config() {
    // Test that discovery finds ~/.gemini/GEMINI.md for Gemini agents
    let tmp = TempDir::new().expect("tempdir");
    let fake_home = tmp.path().join("fake_home");
    let project = tmp.path().join("project");

    // Create fake home with .gemini/GEMINI.md
    fs::create_dir_all(fake_home.join(".gemini")).expect("create .gemini");
    fs::write(fake_home.join(".gemini/GEMINI.md"), "user gemini config")
        .expect("write user GEMINI.md");

    // Create project with .git
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join(".git"), "gitdir").expect("write .git");

    // Discover files with custom home
    let files = discover_all_instruction_files_with_home(
        &project,
        Some(AgentKindSimple::Gemini),
        Some(&fake_home),
    );

    // Should find the home config
    let has_home_config = files
        .iter()
        .any(|f| f.path.to_string_lossy().contains(".gemini/GEMINI.md"));
    assert!(has_home_config, "Should find ~/.gemini/GEMINI.md");

    // Home config should be active for Gemini
    let home_file = files
        .iter()
        .find(|f| f.path.to_string_lossy().contains(".gemini/GEMINI.md"))
        .expect("Should find home GEMINI.md");
    assert!(
        home_file.active,
        "Home GEMINI.md should be active for Gemini"
    );
}

#[test]
fn discover_home_config_is_inactive_for_other_agents() {
    // Test that Claude home config is inactive when running as Codex agent
    let tmp = TempDir::new().expect("tempdir");
    let fake_home = tmp.path().join("fake_home");
    let project = tmp.path().join("project");

    // Create fake home with all agent configs
    fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
    fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
        .expect("write user CLAUDE.md");
    fs::create_dir_all(fake_home.join(".codex")).expect("create .codex");
    fs::write(fake_home.join(".codex/AGENTS.md"), "user codex config")
        .expect("write user AGENTS.md");

    // Create project with .git
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join(".git"), "gitdir").expect("write .git");

    // Discover files as Codex agent
    let files = discover_all_instruction_files_with_home(
        &project,
        Some(AgentKindSimple::Codex),
        Some(&fake_home),
    );

    // Claude home config should exist but be inactive
    let claude_file = files
        .iter()
        .find(|f| f.path.to_string_lossy().contains(".claude/CLAUDE.md"));
    if let Some(f) = claude_file {
        assert!(
            !f.active,
            "Claude home config should be inactive for Codex agent"
        );
    }

    // Codex home config should be active
    let codex_file = files
        .iter()
        .find(|f| f.path.to_string_lossy().contains(".codex/AGENTS.md"))
        .expect("Should find Codex home config");
    assert!(codex_file.active, "Codex home config should be active");
}

#[test]
fn discover_home_config_order_is_first() {
    // Test that home config appears first in the list (before project configs)
    let tmp = TempDir::new().expect("tempdir");
    let fake_home = tmp.path().join("fake_home");
    let project = tmp.path().join("project");

    // Create fake home with .claude/CLAUDE.md
    fs::create_dir_all(fake_home.join(".claude")).expect("create .claude");
    fs::write(fake_home.join(".claude/CLAUDE.md"), "user claude config")
        .expect("write user CLAUDE.md");

    // Create project with .git and CLAUDE.md
    fs::create_dir_all(&project).expect("create project");
    fs::write(project.join(".git"), "gitdir").expect("write .git");
    fs::write(project.join("CLAUDE.md"), "project claude config").expect("write project CLAUDE.md");

    // Discover files with custom home
    let files = discover_all_instruction_files_with_home(
        &project,
        Some(AgentKindSimple::Claude),
        Some(&fake_home),
    );

    assert!(files.len() >= 2, "Should find at least 2 files");

    // First file should be the home config
    let first_file = &files[0];
    assert!(
        first_file.path.to_string_lossy().contains("fake_home"),
        "First file should be the home config, got: {:?}",
        first_file.path
    );
}

// =========================================================================
// ENHANCED STATUS CARD TESTS
// =========================================================================

#[test]
fn status_card_with_task_summary_renders_summary_at_top() {
    // When a task summary is provided, it should appear near the top of the card
    // (after the title block, before the main info)
    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        Some("Fix authentication bug".to_string()),
        None,
        None,
        None,
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should contain the task summary
    assert!(
        rendered.contains("Task:"),
        "Status card should show 'Task:' label when summary is provided"
    );
    assert!(
        rendered.contains("Fix authentication bug"),
        "Status card should show the task summary text"
    );
}

#[test]
fn status_card_with_tokens_renders_tokens_section() {
    // When token info is provided, a Tokens section should appear
    use codex_acp::TranscriptTokenUsage;

    let token_breakdown = TranscriptTokenUsage {
        input_tokens: 45000,
        output_tokens: 78000,
        cached_tokens: 32000,
        last_context_tokens: None,
    };

    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        None,
        None,
        Some(token_breakdown),
        Some(27),
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should contain the Tokens section
    assert!(
        rendered.contains("Tokens"),
        "Status card should show 'Tokens' section header when token info is provided"
    );
    // Should contain context window info
    assert!(
        rendered.contains("Context:"),
        "Status card should show context window usage"
    );
    assert!(
        rendered.contains("27%"),
        "Status card should show context window percentage"
    );
}

#[test]
fn status_card_with_approval_mode_renders_approval() {
    // When approval mode is provided, it should appear in the info block
    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        None,
        Some("Agent".to_string()),
        None,
        None,
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should contain the approval mode
    assert!(
        rendered.contains("approvals:"),
        "Status card should show 'approvals:' label when approval mode is provided"
    );
    assert!(
        rendered.contains("Agent"),
        "Status card should show the approval mode value"
    );
}

#[test]
fn status_card_without_optional_fields_renders_base_only() {
    // When all optional fields are None, the card should render base info only
    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        None,
        None,
        None,
        None,
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should NOT contain optional sections
    assert!(
        !rendered.contains("Task:"),
        "Status card should NOT show 'Task:' when no summary provided"
    );
    assert!(
        !rendered.contains("Tokens"),
        "Status card should NOT show 'Tokens' section when no token info"
    );
    // Should contain base info
    assert!(
        rendered.contains("Nori CLI"),
        "Status card should show Nori CLI title"
    );
    assert!(
        rendered.contains("directory:"),
        "Status card should show directory"
    );
    assert!(rendered.contains("agent:"), "Status card should show agent");
}

#[test]
fn status_card_truncates_long_task_summary() {
    // Task summary should be truncated to a reasonable length
    let long_summary = "This is an extremely long task summary that goes on and on describing what the user wants to accomplish in great detail with many words";

    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        Some(long_summary.to_string()),
        None,
        None,
        None,
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should contain Task: label
    assert!(
        rendered.contains("Task:"),
        "Status card should show 'Task:' label"
    );
    // The full long summary should NOT appear (it should be truncated)
    assert!(
        !rendered.contains(long_summary),
        "Status card should truncate long task summaries"
    );
    // Should contain some ellipsis or truncation indicator
    assert!(
        rendered.contains("...") || rendered.len() < long_summary.len() + 100,
        "Status card should indicate truncation"
    );
}

#[test]
fn status_card_with_zero_tokens_hides_tokens_section() {
    // When token breakdown has all zeros, the section should be hidden
    use codex_acp::TranscriptTokenUsage;

    let token_breakdown = TranscriptTokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: 0,
        last_context_tokens: None,
    };

    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/tmp/project"),
        None,
        None,
        Some(token_breakdown),
        None,
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should NOT show Tokens section when all zeros
    assert!(
        !rendered.contains("Tokens"),
        "Status card should NOT show 'Tokens' section when all token values are zero"
    );
}

#[test]
fn truncate_summary_handles_multibyte_utf8() {
    // Multi-byte chars: each CJK character is 3 bytes in UTF-8.
    // "修复认证错误" is 6 chars, 18 bytes. With max_len=5 (chars), the
    // old byte-slicing code would slice at byte offset 2 which is inside
    // a multi-byte sequence and panic.
    let summary = "修复认证错误的问题在这里需要更多的文字来触发截断";
    let result = truncate_summary(summary, 10);
    assert!(
        result.ends_with("..."),
        "Should end with ellipsis, got: {result}"
    );
    assert!(
        result.chars().count() <= 10,
        "Should be at most 10 chars, got {} chars: {result}",
        result.chars().count()
    );
}

#[test]
fn context_window_percent_renders_without_token_breakdown() {
    // context_window_percent should render under the Tokens header
    // even when token_breakdown is None
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "test-agent".to_string(),
        directory: PathBuf::from("/tmp/test"),
        nori_profile: None,
        instruction_files: Vec::new(),
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: Some(42),
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    assert!(
        rendered.contains("Tokens"),
        "Should show 'Tokens' section header when context_window_percent is set: {rendered}"
    );
    assert!(
        rendered.contains("42%"),
        "Should show context window percentage: {rendered}"
    );
}

#[test]
fn status_card_full_snapshot() {
    // Mock instruction files for consistent snapshots across machines
    // SAFETY: test-only; set_var is unsafe in edition 2024 due to thread-safety
    // concerns, but snapshot tests run serially with insta.
    unsafe { std::env::set_var("NORI_MOCK_INSTRUCTION_FILES", "1") };

    // Snapshot test with all optional fields provided
    use codex_acp::TranscriptTokenUsage;

    let token_breakdown = TranscriptTokenUsage {
        input_tokens: 45000,
        output_tokens: 78000,
        cached_tokens: 32000,
        last_context_tokens: None,
    };

    let status_output = new_nori_status_output(
        "claude-sonnet",
        PathBuf::from("/home/user/project"),
        Some("Fix auth bug".to_string()),
        Some("Agent".to_string()),
        Some(token_breakdown),
        Some(27),
    );

    let lines = status_output.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    unsafe { std::env::remove_var("NORI_MOCK_INSTRUCTION_FILES") };
    insta::assert_snapshot!(rendered);
}

// =========================================================================
// DisplayMode tests: Compact vs Full
// =========================================================================

fn sample_instruction_files() -> Vec<InstructionFile> {
    vec![
        InstructionFile {
            path: PathBuf::from("/home/user/.claude/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 1200,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/CLAUDE.md"),
            active: true,
            token_count: Some(TokenCount {
                count: 800,
                approximate: true,
            }),
        },
        InstructionFile {
            path: PathBuf::from("/home/user/project/AGENTS.md"),
            active: false,
            token_count: None,
        },
    ]
}

#[test]
fn compact_mode_hides_inactive_files() {
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "claude-code".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("test-profile".to_string()),
        instruction_files: sample_instruction_files(),
        display_mode: DisplayMode::Compact,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should show active files
    assert!(
        rendered.contains(".claude/CLAUDE.md"),
        "Compact mode should show active file .claude/CLAUDE.md: {rendered}"
    );
    assert!(
        rendered.contains("project/CLAUDE.md"),
        "Compact mode should show active file project/CLAUDE.md: {rendered}"
    );

    // Should NOT show inactive files
    assert!(
        !rendered.contains("AGENTS.md"),
        "Compact mode should hide inactive AGENTS.md: {rendered}"
    );
}

#[test]
fn compact_mode_hides_per_file_token_counts() {
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "claude-code".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("test-profile".to_string()),
        instruction_files: sample_instruction_files(),
        display_mode: DisplayMode::Compact,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should NOT show per-file token counts
    assert!(
        !rendered.contains("~1,200 tokens"),
        "Compact mode should not show per-file token counts: {rendered}"
    );
    assert!(
        !rendered.contains("~800 tokens"),
        "Compact mode should not show per-file token counts: {rendered}"
    );

    // Should still show total
    assert!(
        rendered.contains("~2,000 tokens"),
        "Compact mode should still show total token count: {rendered}"
    );
}

#[test]
fn full_mode_shows_inactive_files_and_per_file_counts() {
    let cell = NoriSessionHeaderCell {
        version: "test",
        agent: "claude-code".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("test-profile".to_string()),
        instruction_files: sample_instruction_files(),
        display_mode: DisplayMode::Full,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    // Should show inactive files
    assert!(
        rendered.contains("AGENTS.md"),
        "Full mode should show inactive AGENTS.md: {rendered}"
    );

    // Should show per-file token counts
    assert!(
        rendered.contains("~1,200 tokens"),
        "Full mode should show per-file token count ~1,200: {rendered}"
    );
    assert!(
        rendered.contains("~800 tokens"),
        "Full mode should show per-file token count ~800: {rendered}"
    );

    // Should show total
    assert!(
        rendered.contains("~2,000 tokens"),
        "Full mode should show total token count: {rendered}"
    );
}

#[test]
fn compact_mode_snapshot() {
    let cell = NoriSessionHeaderCell {
        version: "0.1.0",
        agent: "claude-sonnet".to_string(),
        directory: PathBuf::from("/home/user/project"),
        nori_profile: Some("senior-swe".to_string()),
        instruction_files: vec![
            InstructionFile {
                path: PathBuf::from("/home/user/project/AGENTS.md"),
                active: false,
                token_count: None,
            },
            InstructionFile {
                path: PathBuf::from("/home/user/project/.claude/settings.md"),
                active: true,
                token_count: Some(TokenCount {
                    count: 2450,
                    approximate: true,
                }),
            },
        ],
        display_mode: DisplayMode::Compact,
        prompt_summary: None,
        approval_mode_label: None,
        token_breakdown: None,
        context_window_percent: None,
    };

    let lines = cell.display_lines(80);
    let rendered = render_lines(&lines).join("\n");

    insta::assert_snapshot!(rendered);
}
