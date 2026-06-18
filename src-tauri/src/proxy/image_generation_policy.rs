use serde_json::Value;

const IMAGE_GENERATION_DISABLED_MESSAGE: &str = "Image generation is not enabled for this group";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageGenerationPolicyMode {
    Off,
    Chat,
    All,
}

impl From<crate::settings::DisableImageGenerationMode> for ImageGenerationPolicyMode {
    fn from(value: crate::settings::DisableImageGenerationMode) -> Self {
        match value {
            crate::settings::DisableImageGenerationMode::Off => Self::Off,
            crate::settings::DisableImageGenerationMode::All => Self::All,
            crate::settings::DisableImageGenerationMode::Chat => Self::Chat,
        }
    }
}

impl ImageGenerationPolicyMode {
    pub fn from_provider(provider: &crate::provider::Provider) -> ImageGenerationPolicyMode {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.disable_image_generation)
            .map(ImageGenerationPolicyMode::from)
            .unwrap_or(ImageGenerationPolicyMode::Off)
    }
}

pub fn apply_image_generation_policy(
    mode: ImageGenerationPolicyMode,
    endpoint: &str,
    body: &mut Value,
) -> bool {
    if !should_apply(mode, endpoint) {
        return false;
    }

    let Some(object) = body.as_object_mut() else {
        return false;
    };

    let mut changed = false;

    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        let before = tools.len();
        tools.retain(|tool| !is_image_generation_tool(tool));
        if tools.len() != before {
            changed = true;
        }
    }

    let remove_empty_tools = object
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    if remove_empty_tools {
        object.remove("tools");
    }

    if object
        .get("tool_choice")
        .is_some_and(is_image_generation_tool_choice)
    {
        object.remove("tool_choice");
        changed = true;
    }

    changed
}

pub fn request_has_image_generation_tool(endpoint: &str, body: &Value) -> bool {
    if !is_chat_endpoint(endpoint) {
        return false;
    }

    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(is_image_generation_tool))
        || body
            .get("tool_choice")
            .is_some_and(is_image_generation_tool_choice)
}

pub fn is_image_generation_disabled_403(status: u16, body: Option<&str>) -> bool {
    status == 403
        && body
            .map(|body| body.contains(IMAGE_GENERATION_DISABLED_MESSAGE))
            .unwrap_or(false)
}

fn should_apply(mode: ImageGenerationPolicyMode, endpoint: &str) -> bool {
    match mode {
        ImageGenerationPolicyMode::Off => false,
        ImageGenerationPolicyMode::All | ImageGenerationPolicyMode::Chat => {
            is_chat_endpoint(endpoint)
        }
    }
}

fn is_chat_endpoint(endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path)
        .trim_end_matches('/')
        .to_ascii_lowercase();

    matches!(path.as_str(), "/chat/completions" | "/v1/chat/completions")
        || path == "/responses"
        || path.starts_with("/responses/")
        || path == "/v1/responses"
        || path.starts_with("/v1/responses/")
}

fn is_image_generation_tool(tool: &Value) -> bool {
    tool.get("type")
        .and_then(Value::as_str)
        .is_some_and(|tool_type| tool_type.eq_ignore_ascii_case("image_generation"))
}

fn is_image_generation_tool_choice(tool_choice: &Value) -> bool {
    match tool_choice {
        Value::String(choice) => choice.trim().eq_ignore_ascii_case("image_generation"),
        Value::Object(choice) => {
            let choice_type = choice
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            choice_type.eq_ignore_ascii_case("image_generation")
                || (choice_type.eq_ignore_ascii_case("tool")
                    && choice
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| name.trim().eq_ignore_ascii_case("image_generation")))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_mode_removes_image_generation_tool_and_matching_choice_from_responses() {
        let mut body = json!({
            "model": "gpt-5.1",
            "tools": [
                { "type": "image_generation" },
                { "type": "function", "name": "read_file" }
            ],
            "tool_choice": { "type": "image_generation" }
        });

        let changed = apply_image_generation_policy(
            ImageGenerationPolicyMode::Chat,
            "/v1/responses",
            &mut body,
        );

        assert!(changed);
        assert_eq!(
            body["tools"],
            json!([{ "type": "function", "name": "read_file" }])
        );
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn provider_mode_defaults_off_and_uses_provider_meta() {
        let provider =
            crate::provider::Provider::with_id("p".to_string(), "P".to_string(), json!({}), None);
        assert_eq!(
            ImageGenerationPolicyMode::from_provider(&provider),
            ImageGenerationPolicyMode::Off
        );

        let mut provider = provider;
        provider.meta = Some(crate::provider::ProviderMeta {
            disable_image_generation: Some(crate::settings::DisableImageGenerationMode::Chat),
            ..Default::default()
        });

        assert_eq!(
            ImageGenerationPolicyMode::from_provider(&provider),
            ImageGenerationPolicyMode::Chat
        );
    }

    #[test]
    fn chat_mode_removes_image_generation_tool_from_local_responses_routes() {
        for endpoint in ["/responses", "/responses/compact"] {
            let mut body = json!({
                "tools": [{ "type": "image_generation" }],
                "tool_choice": { "type": "image_generation" }
            });

            let changed =
                apply_image_generation_policy(ImageGenerationPolicyMode::Chat, endpoint, &mut body);

            assert!(changed, "endpoint {endpoint} should be treated as chat");
            assert!(body.get("tools").is_none());
            assert!(body.get("tool_choice").is_none());
        }
    }

    #[test]
    fn chat_mode_does_not_affect_images_generation_endpoint() {
        let original = json!({
            "model": "gpt-image-1",
            "prompt": "draw a cat",
            "tools": [{ "type": "image_generation" }],
            "tool_choice": { "type": "image_generation" }
        });
        let mut body = original.clone();

        let changed = apply_image_generation_policy(
            ImageGenerationPolicyMode::Chat,
            "/v1/images/generations",
            &mut body,
        );

        assert!(!changed);
        assert_eq!(body, original);
    }

    #[test]
    fn all_mode_removes_image_generation_tool_from_chat_endpoint() {
        let mut body = json!({
            "tools": [{ "type": "image_generation" }],
            "tool_choice": { "type": "image_generation" }
        });

        let changed = apply_image_generation_policy(
            ImageGenerationPolicyMode::All,
            "/v1/chat/completions",
            &mut body,
        );

        assert!(changed);
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn all_mode_does_not_affect_images_generation_endpoint() {
        let original = json!({
            "tools": [{ "type": "image_generation" }],
            "tool_choice": { "type": "image_generation" }
        });
        let mut body = original.clone();

        let changed = apply_image_generation_policy(
            ImageGenerationPolicyMode::All,
            "/v1/images/generations",
            &mut body,
        );

        assert!(!changed);
        assert_eq!(body, original);
    }

    #[test]
    fn removes_tool_choice_string_and_tool_name_variants() {
        for tool_choice in [
            json!("image_generation"),
            json!({"type": "image_generation"}),
            json!({"type": "tool", "name": "image_generation"}),
        ] {
            let mut body = json!({
                "tools": [{ "type": "image_generation" }],
                "tool_choice": tool_choice
            });

            let changed = apply_image_generation_policy(
                ImageGenerationPolicyMode::Chat,
                "/v1/responses",
                &mut body,
            );

            assert!(changed);
            assert!(body.get("tool_choice").is_none());
        }
    }

    #[test]
    fn request_detection_requires_chat_endpoint_and_image_generation_tool_or_choice() {
        assert!(request_has_image_generation_tool(
            "/v1/responses",
            &json!({ "tools": [{ "type": "image_generation" }] })
        ));
        assert!(request_has_image_generation_tool(
            "/v1/chat/completions",
            &json!({ "tool_choice": { "type": "tool", "name": "image_generation" } })
        ));
        assert!(!request_has_image_generation_tool(
            "/v1/images/generations",
            &json!({
                "tools": [{ "type": "image_generation" }],
                "tool_choice": "image_generation"
            })
        ));
        assert!(!request_has_image_generation_tool(
            "/v1/responses",
            &json!({ "tools": [{ "type": "function", "name": "read_file" }] })
        ));
    }

    #[test]
    fn disabled_error_detection_requires_403_and_exact_message() {
        assert!(is_image_generation_disabled_403(
            403,
            Some(r#"{"error":{"message":"Image generation is not enabled for this group"}}"#)
        ));
        assert!(!is_image_generation_disabled_403(
            400,
            Some(r#"{"error":{"message":"Image generation is not enabled for this group"}}"#)
        ));
        assert!(!is_image_generation_disabled_403(
            403,
            Some(r#"{"error":{"message":"image generation unavailable"}}"#)
        ));
        assert!(!is_image_generation_disabled_403(403, None));
    }
}
