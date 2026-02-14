use super::*;
use crate::client_common::tools::FreeformTool;
use crate::model_family::find_family_for_model;
use crate::tools::registry::ConfiguredToolSpec;
use mcp_types::ToolInputSchema;
use pretty_assertions::assert_eq;

fn tool_name(tool: &ToolSpec) -> &str {
    match tool {
        ToolSpec::Function(ResponsesApiTool { name, .. }) => name,
        ToolSpec::LocalShell {} => "local_shell",
        ToolSpec::WebSearch {} => "web_search",
        ToolSpec::Freeform(FreeformTool { name, .. }) => name,
    }
}

// Avoid order-based assertions; compare via set containment instead.
fn assert_contains_tool_names(tools: &[ConfiguredToolSpec], expected_subset: &[&str]) {
    use std::collections::HashSet;
    let mut names = HashSet::new();
    let mut duplicates = Vec::new();
    for name in tools.iter().map(|t| tool_name(&t.spec)) {
        if !names.insert(name) {
            duplicates.push(name);
        }
    }
    assert!(
        duplicates.is_empty(),
        "duplicate tool entries detected: {duplicates:?}"
    );
    for expected in expected_subset {
        assert!(
            names.contains(expected),
            "expected tool {expected} to be present; had: {names:?}"
        );
    }
}

fn shell_tool_name(config: &ToolsConfig) -> Option<&'static str> {
    match config.shell_type {
        ConfigShellToolType::Default => Some("shell"),
        ConfigShellToolType::Local => Some("local_shell"),
        ConfigShellToolType::UnifiedExec => None,
        ConfigShellToolType::Disabled => None,
        ConfigShellToolType::ShellCommand => Some("shell_command"),
    }
}

fn find_tool<'a>(tools: &'a [ConfiguredToolSpec], expected_name: &str) -> &'a ConfiguredToolSpec {
    tools
        .iter()
        .find(|tool| tool_name(&tool.spec) == expected_name)
        .unwrap_or_else(|| panic!("expected tool {expected_name}"))
}

fn strip_descriptions_schema(schema: &mut JsonSchema) {
    match schema {
        JsonSchema::Boolean { description }
        | JsonSchema::String { description }
        | JsonSchema::Number { description } => {
            *description = None;
        }
        JsonSchema::Array { items, description } => {
            strip_descriptions_schema(items);
            *description = None;
        }
        JsonSchema::Object {
            properties,
            required: _,
            additional_properties,
        } => {
            for v in properties.values_mut() {
                strip_descriptions_schema(v);
            }
            if let Some(AdditionalProperties::Schema(s)) = additional_properties {
                strip_descriptions_schema(s);
            }
        }
    }
}

fn strip_descriptions_tool(spec: &mut ToolSpec) {
    match spec {
        ToolSpec::Function(ResponsesApiTool { parameters, .. }) => {
            strip_descriptions_schema(parameters);
        }
        ToolSpec::Freeform(_) | ToolSpec::LocalShell {} | ToolSpec::WebSearch {} => {}
    }
}

#[test]
fn test_full_toolset_specs_for_gpt5_codex_unified_exec_web_search() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    features.enable(Feature::ViewImageTool);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(&config, None).build();

    // Build actual map name -> spec
    use std::collections::BTreeMap;
    use std::collections::HashSet;
    let mut actual: BTreeMap<String, ToolSpec> = BTreeMap::new();
    let mut duplicate_names = Vec::new();
    for t in &tools {
        let name = tool_name(&t.spec).to_string();
        if actual.insert(name.clone(), t.spec.clone()).is_some() {
            duplicate_names.push(name);
        }
    }
    assert!(
        duplicate_names.is_empty(),
        "duplicate tool entries detected: {duplicate_names:?}"
    );

    // Build expected from the same helpers used by the builder.
    let mut expected: BTreeMap<String, ToolSpec> = BTreeMap::new();
    for spec in [
        create_exec_command_tool(),
        create_write_stdin_tool(),
        create_list_mcp_resources_tool(),
        create_list_mcp_resource_templates_tool(),
        create_read_mcp_resource_tool(),
        PLAN_TOOL.clone(),
        create_apply_patch_freeform_tool(),
        ToolSpec::WebSearch {},
        create_view_image_tool(),
    ] {
        expected.insert(tool_name(&spec).to_string(), spec);
    }

    // Exact name set match — this is the only test allowed to fail when tools change.
    let actual_names: HashSet<_> = actual.keys().cloned().collect();
    let expected_names: HashSet<_> = expected.keys().cloned().collect();
    assert_eq!(actual_names, expected_names, "tool name set mismatch");

    // Compare specs ignoring human-readable descriptions.
    for name in expected.keys() {
        let mut a = actual.get(name).expect("present").clone();
        let mut e = expected.get(name).expect("present").clone();
        strip_descriptions_tool(&mut a);
        strip_descriptions_tool(&mut e);
        assert_eq!(a, e, "spec mismatch for {name}");
    }
}

fn assert_model_tools(model_family: &str, features: &Features, expected_tools: &[&str]) {
    let model_family = find_family_for_model(model_family)
        .unwrap_or_else(|| panic!("{model_family} should be a valid model family"));
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features,
    });
    let (tools, _) = build_specs(&config, Some(HashMap::new())).build();
    let tool_names = tools.iter().map(|t| t.spec.name()).collect::<Vec<_>>();
    assert_eq!(&tool_names, &expected_tools,);
}

#[test]
fn test_build_specs_gpt5_codex_default() {
    assert_model_tools(
        "gpt-5-codex",
        &Features::with_defaults(),
        &[
            "shell_command",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "view_image",
        ],
    );
}

#[test]
fn test_build_specs_gpt51_codex_default() {
    assert_model_tools(
        "gpt-5.1-codex",
        &Features::with_defaults(),
        &[
            "shell_command",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "view_image",
        ],
    );
}

#[test]
fn test_build_specs_gpt5_codex_unified_exec_web_search() {
    assert_model_tools(
        "gpt-5-codex",
        Features::with_defaults()
            .enable(Feature::UnifiedExec)
            .enable(Feature::WebSearchRequest),
        &[
            "exec_command",
            "write_stdin",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "web_search",
            "view_image",
        ],
    );
}

#[test]
fn test_build_specs_gpt51_codex_unified_exec_web_search() {
    assert_model_tools(
        "gpt-5.1-codex",
        Features::with_defaults()
            .enable(Feature::UnifiedExec)
            .enable(Feature::WebSearchRequest),
        &[
            "exec_command",
            "write_stdin",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "web_search",
            "view_image",
        ],
    );
}

#[test]
fn test_codex_mini_defaults() {
    assert_model_tools(
        "codex-mini-latest",
        &Features::with_defaults(),
        &[
            "local_shell",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "view_image",
        ],
    );
}

#[test]
fn test_codex_5_1_mini_defaults() {
    assert_model_tools(
        "gpt-5.1-codex-mini",
        &Features::with_defaults(),
        &[
            "shell_command",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "view_image",
        ],
    );
}

#[test]
fn test_gpt_5_defaults() {
    assert_model_tools(
        "gpt-5",
        &Features::with_defaults(),
        &[
            "shell",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "view_image",
        ],
    );
}

#[test]
fn test_gpt_5_1_defaults() {
    assert_model_tools(
        "gpt-5.1",
        &Features::with_defaults(),
        &[
            "shell_command",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "view_image",
        ],
    );
}

#[test]
fn test_exp_5_1_defaults() {
    assert_model_tools(
        "exp-5.1",
        &Features::with_defaults(),
        &[
            "exec_command",
            "write_stdin",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "apply_patch",
            "view_image",
        ],
    );
}

#[test]
fn test_codex_mini_unified_exec_web_search() {
    assert_model_tools(
        "codex-mini-latest",
        Features::with_defaults()
            .enable(Feature::UnifiedExec)
            .enable(Feature::WebSearchRequest),
        &[
            "exec_command",
            "write_stdin",
            "list_mcp_resources",
            "list_mcp_resource_templates",
            "read_mcp_resource",
            "update_plan",
            "web_search",
            "view_image",
        ],
    );
}

#[test]
fn test_build_specs_default_shell_present() {
    let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::WebSearchRequest);
    features.enable(Feature::UnifiedExec);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(&config, Some(HashMap::new())).build();

    // Only check the shell variant and a couple of core tools.
    let mut subset = vec!["exec_command", "write_stdin", "update_plan"];
    if let Some(shell_tool) = shell_tool_name(&config) {
        subset.push(shell_tool);
    }
    assert_contains_tool_names(&tools, &subset);
}

#[test]
#[ignore]
fn test_parallel_support_flags() {
    let model_family = find_family_for_model("gpt-5-codex")
        .expect("codex-mini-latest should be a valid model family");
    let mut features = Features::with_defaults();
    features.disable(Feature::ViewImageTool);
    features.enable(Feature::UnifiedExec);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(&config, None).build();

    assert!(!find_tool(&tools, "exec_command").supports_parallel_tool_calls);
    assert!(!find_tool(&tools, "write_stdin").supports_parallel_tool_calls);
    assert!(find_tool(&tools, "grep_files").supports_parallel_tool_calls);
    assert!(find_tool(&tools, "list_dir").supports_parallel_tool_calls);
    assert!(find_tool(&tools, "read_file").supports_parallel_tool_calls);
}

#[test]
fn test_test_model_family_includes_sync_tool() {
    let model_family = find_family_for_model("test-gpt-5-codex")
        .expect("test-gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.disable(Feature::ViewImageTool);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(&config, None).build();

    assert!(
        tools
            .iter()
            .any(|tool| tool_name(&tool.spec) == "test_sync_tool")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool_name(&tool.spec) == "read_file")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool_name(&tool.spec) == "grep_files")
    );
    assert!(tools.iter().any(|tool| tool_name(&tool.spec) == "list_dir"));
}

#[test]
fn test_build_specs_mcp_tools_converted() {
    let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "test_server/do_something_cool".to_string(),
            mcp_types::Tool {
                name: "do_something_cool".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "string_argument": {
                            "type": "string",
                        },
                        "number_argument": {
                            "type": "number",
                        },
                        "object_argument": {
                            "type": "object",
                            "properties": {
                                "string_property": { "type": "string" },
                                "number_property": { "type": "number" },
                            },
                            "required": [
                                "string_property",
                                "number_property",
                            ],
                            "additionalProperties": Some(false),
                        },
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("Do something cool".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "test_server/do_something_cool");
    assert_eq!(
        &tool.spec,
        &ToolSpec::Function(ResponsesApiTool {
            name: "test_server/do_something_cool".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([
                    (
                        "string_argument".to_string(),
                        JsonSchema::String { description: None }
                    ),
                    (
                        "number_argument".to_string(),
                        JsonSchema::Number { description: None }
                    ),
                    (
                        "object_argument".to_string(),
                        JsonSchema::Object {
                            properties: BTreeMap::from([
                                (
                                    "string_property".to_string(),
                                    JsonSchema::String { description: None }
                                ),
                                (
                                    "number_property".to_string(),
                                    JsonSchema::Number { description: None }
                                ),
                            ]),
                            required: Some(vec![
                                "string_property".to_string(),
                                "number_property".to_string(),
                            ]),
                            additional_properties: Some(false.into()),
                        },
                    ),
                ]),
                required: None,
                additional_properties: None,
            },
            description: "Do something cool".to_string(),
            strict: false,
        })
    );
}

#[test]
fn test_build_specs_mcp_tools_sorted_by_name() {
    let model_family = find_family_for_model("o3").expect("o3 should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });

    // Intentionally construct a map with keys that would sort alphabetically.
    let tools_map: HashMap<String, mcp_types::Tool> = HashMap::from([
        (
            "test_server/do".to_string(),
            mcp_types::Tool {
                name: "a".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({})),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("a".to_string()),
            },
        ),
        (
            "test_server/something".to_string(),
            mcp_types::Tool {
                name: "b".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({})),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("b".to_string()),
            },
        ),
        (
            "test_server/cool".to_string(),
            mcp_types::Tool {
                name: "c".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({})),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("c".to_string()),
            },
        ),
    ]);

    let (tools, _) = build_specs(&config, Some(tools_map)).build();

    // Only assert that the MCP tools themselves are sorted by fully-qualified name.
    let mcp_names: Vec<_> = tools
        .iter()
        .map(|t| tool_name(&t.spec).to_string())
        .filter(|n| n.starts_with("test_server/"))
        .collect();
    let expected = vec![
        "test_server/cool".to_string(),
        "test_server/do".to_string(),
        "test_server/something".to_string(),
    ];
    assert_eq!(mcp_names, expected);
}

#[test]
fn test_mcp_tool_property_missing_type_defaults_to_string() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });

    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "dash/search".to_string(),
            mcp_types::Tool {
                name: "search".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "query": {
                            "description": "search query"
                        }
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("Search docs".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "dash/search");
    assert_eq!(
        tool.spec,
        ToolSpec::Function(ResponsesApiTool {
            name: "dash/search".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([(
                    "query".to_string(),
                    JsonSchema::String {
                        description: Some("search query".to_string())
                    }
                )]),
                required: None,
                additional_properties: None,
            },
            description: "Search docs".to_string(),
            strict: false,
        })
    );
}

#[test]
fn test_mcp_tool_integer_normalized_to_number() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });

    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "dash/paginate".to_string(),
            mcp_types::Tool {
                name: "paginate".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "page": { "type": "integer" }
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("Pagination".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "dash/paginate");
    assert_eq!(
        tool.spec,
        ToolSpec::Function(ResponsesApiTool {
            name: "dash/paginate".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([(
                    "page".to_string(),
                    JsonSchema::Number { description: None }
                )]),
                required: None,
                additional_properties: None,
            },
            description: "Pagination".to_string(),
            strict: false,
        })
    );
}

#[test]
fn test_mcp_tool_array_without_items_gets_default_string_items() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    features.enable(Feature::ApplyPatchFreeform);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });

    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "dash/tags".to_string(),
            mcp_types::Tool {
                name: "tags".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "tags": { "type": "array" }
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("Tags".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "dash/tags");
    assert_eq!(
        tool.spec,
        ToolSpec::Function(ResponsesApiTool {
            name: "dash/tags".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([(
                    "tags".to_string(),
                    JsonSchema::Array {
                        items: Box::new(JsonSchema::String { description: None }),
                        description: None
                    }
                )]),
                required: None,
                additional_properties: None,
            },
            description: "Tags".to_string(),
            strict: false,
        })
    );
}

#[test]
fn test_mcp_tool_anyof_defaults_to_string() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });

    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "dash/value".to_string(),
            mcp_types::Tool {
                name: "value".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "value": { "anyOf": [ { "type": "string" }, { "type": "number" } ] }
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("AnyOf Value".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "dash/value");
    assert_eq!(
        tool.spec,
        ToolSpec::Function(ResponsesApiTool {
            name: "dash/value".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([(
                    "value".to_string(),
                    JsonSchema::String { description: None }
                )]),
                required: None,
                additional_properties: None,
            },
            description: "AnyOf Value".to_string(),
            strict: false,
        })
    );
}

#[test]
fn test_shell_tool() {
    let tool = super::create_shell_tool();
    let ToolSpec::Function(ResponsesApiTool {
        description, name, ..
    }) = &tool
    else {
        panic!("expected function tool");
    };
    assert_eq!(name, "shell");

    let expected = if cfg!(windows) {
        r#"Runs a Powershell command (Windows) and returns its output. Arguments to `shell` will be passed to CreateProcessW(). Most commands should be prefixed with ["powershell.exe", "-Command"].
    
Examples of valid command strings:

- ls -a (show hidden): ["powershell.exe", "-Command", "Get-ChildItem -Force"]
- recursive find by name: ["powershell.exe", "-Command", "Get-ChildItem -Recurse -Filter *.py"]
- recursive grep: ["powershell.exe", "-Command", "Get-ChildItem -Path C:\\myrepo -Recurse | Select-String -Pattern 'TODO' -CaseSensitive"]
- ps aux | grep python: ["powershell.exe", "-Command", "Get-Process | Where-Object { $_.ProcessName -like '*python*' }"]
- setting an env var: ["powershell.exe", "-Command", "$env:FOO='bar'; echo $env:FOO"]
- running an inline Python script: ["powershell.exe", "-Command", "@'\\nprint('Hello, world!')\\n'@ | python -"]"#
    } else {
        r#"Runs a shell command and returns its output.
- The arguments to `shell` will be passed to execvp(). Most terminal commands should be prefixed with ["bash", "-lc"].
- Always set the `workdir` param when using the shell function. Do not use `cd` unless absolutely necessary."#
    }.to_string();
    assert_eq!(description, &expected);
}

#[test]
fn test_shell_command_tool() {
    let tool = super::create_shell_command_tool();
    let ToolSpec::Function(ResponsesApiTool {
        description, name, ..
    }) = &tool
    else {
        panic!("expected function tool");
    };
    assert_eq!(name, "shell_command");

    let expected = if cfg!(windows) {
        r#"Runs a Powershell command (Windows) and returns its output.
    
Examples of valid command strings:

- ls -a (show hidden): "Get-ChildItem -Force"
- recursive find by name: "Get-ChildItem -Recurse -Filter *.py"
- recursive grep: "Get-ChildItem -Path C:\\myrepo -Recurse | Select-String -Pattern 'TODO' -CaseSensitive"
- ps aux | grep python: "Get-Process | Where-Object { $_.ProcessName -like '*python*' }"
- setting an env var: "$env:FOO='bar'; echo $env:FOO"
- running an inline Python script: "@'\\nprint('Hello, world!')\\n'@ | python -"#.to_string()
    } else {
        r#"Runs a shell command and returns its output.
- Always set the `workdir` param when using the shell_command function. Do not use `cd` unless absolutely necessary."#.to_string()
    };
    assert_eq!(description, &expected);
}

#[test]
fn test_get_openai_tools_mcp_tools_with_additional_properties_schema() {
    let model_family =
        find_family_for_model("gpt-5-codex").expect("gpt-5-codex should be a valid model family");
    let mut features = Features::with_defaults();
    features.enable(Feature::UnifiedExec);
    features.enable(Feature::WebSearchRequest);
    let config = ToolsConfig::new(&ToolsConfigParams {
        model_family: &model_family,
        features: &features,
    });
    let (tools, _) = build_specs(
        &config,
        Some(HashMap::from([(
            "test_server/do_something_cool".to_string(),
            mcp_types::Tool {
                name: "do_something_cool".to_string(),
                input_schema: ToolInputSchema {
                    properties: Some(serde_json::json!({
                        "string_argument": {
                            "type": "string",
                        },
                        "number_argument": {
                            "type": "number",
                        },
                        "object_argument": {
                            "type": "object",
                            "properties": {
                                "string_property": { "type": "string" },
                                "number_property": { "type": "number" },
                            },
                            "required": [
                                "string_property",
                                "number_property",
                            ],
                            "additionalProperties": {
                                "type": "object",
                                "properties": {
                                    "addtl_prop": { "type": "string" },
                                },
                                "required": [
                                    "addtl_prop",
                                ],
                                "additionalProperties": false,
                            },
                        },
                    })),
                    required: None,
                    r#type: "object".to_string(),
                },
                output_schema: None,
                title: None,
                annotations: None,
                description: Some("Do something cool".to_string()),
            },
        )])),
    )
    .build();

    let tool = find_tool(&tools, "test_server/do_something_cool");
    assert_eq!(
        tool.spec,
        ToolSpec::Function(ResponsesApiTool {
            name: "test_server/do_something_cool".to_string(),
            parameters: JsonSchema::Object {
                properties: BTreeMap::from([
                    (
                        "string_argument".to_string(),
                        JsonSchema::String { description: None }
                    ),
                    (
                        "number_argument".to_string(),
                        JsonSchema::Number { description: None }
                    ),
                    (
                        "object_argument".to_string(),
                        JsonSchema::Object {
                            properties: BTreeMap::from([
                                (
                                    "string_property".to_string(),
                                    JsonSchema::String { description: None }
                                ),
                                (
                                    "number_property".to_string(),
                                    JsonSchema::Number { description: None }
                                ),
                            ]),
                            required: Some(vec![
                                "string_property".to_string(),
                                "number_property".to_string(),
                            ]),
                            additional_properties: Some(
                                JsonSchema::Object {
                                    properties: BTreeMap::from([(
                                        "addtl_prop".to_string(),
                                        JsonSchema::String { description: None }
                                    ),]),
                                    required: Some(vec!["addtl_prop".to_string(),]),
                                    additional_properties: Some(false.into()),
                                }
                                .into()
                            ),
                        },
                    ),
                ]),
                required: None,
                additional_properties: None,
            },
            description: "Do something cool".to_string(),
            strict: false,
        })
    );
}
