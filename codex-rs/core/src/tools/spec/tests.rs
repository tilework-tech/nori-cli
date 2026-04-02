use super::*;
use pretty_assertions::assert_eq;

#[test]
fn test_mcp_tool_to_openai_tool_conversion() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
        name: "search".to_string(),
        input_schema: ToolInputSchema {
            properties: Some(serde_json::json!({
                "query": {
                    "type": "string",
                    "description": "search query"
                }
            })),
            required: Some(vec!["query".to_string()]),
            r#type: "object".to_string(),
        },
        output_schema: None,
        title: None,
        annotations: None,
        description: Some("Search docs".to_string()),
    };

    let converted = mcp_tool_to_openai_tool("dash/search".to_string(), tool).unwrap();
    assert_eq!(converted.name, "dash/search");
    assert_eq!(converted.description, "Search docs");
    assert_eq!(
        converted.parameters,
        JsonSchema::Object {
            properties: BTreeMap::from([(
                "query".to_string(),
                JsonSchema::String {
                    description: Some("search query".to_string())
                }
            )]),
            required: Some(vec!["query".to_string()]),
            additional_properties: None,
        }
    );
}

#[test]
fn test_mcp_tool_property_missing_type_defaults_to_string() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
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
    };

    let converted = mcp_tool_to_openai_tool("dash/search".to_string(), tool).unwrap();
    let ToolSpec::Function(tool) = ToolSpec::Function(converted) else {
        panic!("expected function tool");
    };
    assert_eq!(
        tool.parameters,
        JsonSchema::Object {
            properties: BTreeMap::from([(
                "query".to_string(),
                JsonSchema::String {
                    description: Some("search query".to_string())
                }
            )]),
            required: None,
            additional_properties: None,
        }
    );
}

#[test]
fn test_mcp_tool_integer_normalized_to_number() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
        name: "count".to_string(),
        input_schema: ToolInputSchema {
            properties: Some(serde_json::json!({
                "n": { "type": "integer", "description": "how many" }
            })),
            required: None,
            r#type: "object".to_string(),
        },
        output_schema: None,
        title: None,
        annotations: None,
        description: Some("Count things".to_string()),
    };

    let converted = mcp_tool_to_openai_tool("ns/count".to_string(), tool).unwrap();
    assert_eq!(
        converted.parameters,
        JsonSchema::Object {
            properties: BTreeMap::from([(
                "n".to_string(),
                JsonSchema::Number {
                    description: Some("how many".to_string())
                }
            )]),
            required: None,
            additional_properties: None,
        }
    );
}

#[test]
fn test_mcp_tool_array_without_items_gets_default_string_items() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
        name: "list".to_string(),
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
        description: Some("List things".to_string()),
    };

    let converted = mcp_tool_to_openai_tool("ns/list".to_string(), tool).unwrap();
    assert_eq!(
        converted.parameters,
        JsonSchema::Object {
            properties: BTreeMap::from([(
                "tags".to_string(),
                JsonSchema::Array {
                    description: None,
                    items: Box::new(JsonSchema::String { description: None }),
                }
            )]),
            required: None,
            additional_properties: None,
        }
    );
}

#[test]
fn test_mcp_tool_anyof_defaults_to_string() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
        name: "flex".to_string(),
        input_schema: ToolInputSchema {
            properties: Some(serde_json::json!({
                "value": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "number" }
                    ]
                }
            })),
            required: None,
            r#type: "object".to_string(),
        },
        output_schema: None,
        title: None,
        annotations: None,
        description: Some("Flex".to_string()),
    };

    let converted = mcp_tool_to_openai_tool("ns/flex".to_string(), tool).unwrap();
    assert_eq!(
        converted.parameters,
        JsonSchema::Object {
            properties: BTreeMap::from([(
                "value".to_string(),
                JsonSchema::String { description: None }
            )]),
            required: None,
            additional_properties: None,
        }
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
fn test_mcp_tool_with_additional_properties_schema() {
    use mcp_types::ToolInputSchema;

    let tool = mcp_types::Tool {
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
    };

    let converted =
        mcp_tool_to_openai_tool("test_server/do_something_cool".to_string(), tool).unwrap();
    assert_eq!(
        ToolSpec::Function(converted),
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
