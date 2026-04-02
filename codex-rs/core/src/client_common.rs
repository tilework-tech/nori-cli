#[cfg(test)]
pub(crate) mod tools {
    use crate::tools::spec::JsonSchema;
    use serde::Serialize;

    /// When serialized as JSON, this produces a valid "Tool" in the OpenAI
    /// Responses API.
    #[derive(Debug, Clone, Serialize, PartialEq)]
    #[serde(tag = "type")]
    pub(crate) enum ToolSpec {
        #[serde(rename = "function")]
        Function(ResponsesApiTool),
        #[serde(rename = "local_shell")]
        LocalShell {},
        #[serde(rename = "web_search")]
        WebSearch {},
        #[serde(rename = "custom")]
        Freeform(FreeformTool),
    }

    #[derive(Debug, Clone, Serialize, PartialEq)]
    pub struct FreeformTool {
        pub(crate) name: String,
        pub(crate) description: String,
        pub(crate) format: FreeformToolFormat,
    }

    #[derive(Debug, Clone, Serialize, PartialEq)]
    pub struct FreeformToolFormat {
        pub(crate) r#type: String,
        pub(crate) syntax: String,
        pub(crate) definition: String,
    }

    #[derive(Debug, Clone, Serialize, PartialEq)]
    pub struct ResponsesApiTool {
        pub(crate) name: String,
        pub(crate) description: String,
        pub(crate) strict: bool,
        pub(crate) parameters: JsonSchema,
    }
}

#[cfg(test)]
mod tests {
    use crate::model_family::find_family_for_model;
    use codex_apply_patch::APPLY_PATCH_TOOL_INSTRUCTIONS;
    use pretty_assertions::assert_eq;

    use super::tools::ToolSpec;

    struct InstructionsTestCase {
        pub slug: &'static str,
        pub expects_apply_patch_instructions: bool,
    }

    #[test]
    fn get_full_instructions_no_user_content() {
        let test_cases = vec![
            InstructionsTestCase {
                slug: "gpt-3.5",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-4.1",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-4o",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-5",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-5.1",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "codex-mini-latest",
                expects_apply_patch_instructions: true,
            },
            InstructionsTestCase {
                slug: "gpt-oss:120b",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex",
                expects_apply_patch_instructions: false,
            },
            InstructionsTestCase {
                slug: "gpt-5.1-codex-max",
                expects_apply_patch_instructions: false,
            },
        ];
        for test_case in test_cases {
            let model_family = find_family_for_model(test_case.slug).expect("known model slug");
            let has_apply_patch_instructions = model_family.needs_special_apply_patch_instructions;

            assert_eq!(
                has_apply_patch_instructions, test_case.expects_apply_patch_instructions,
                "model {} apply_patch instructions mismatch",
                test_case.slug
            );

            let tools: Vec<ToolSpec> = vec![];
            let is_apply_patch_tool_present = tools.iter().any(|tool| match tool {
                ToolSpec::Function(f) => f.name == "apply_patch",
                ToolSpec::Freeform(f) => f.name == "apply_patch",
                _ => false,
            });
            assert!(
                !is_apply_patch_tool_present,
                "empty tools should not contain apply_patch"
            );

            if test_case.expects_apply_patch_instructions {
                assert!(
                    !APPLY_PATCH_TOOL_INSTRUCTIONS.is_empty(),
                    "APPLY_PATCH_TOOL_INSTRUCTIONS should not be empty"
                );
            }
        }
    }
}
