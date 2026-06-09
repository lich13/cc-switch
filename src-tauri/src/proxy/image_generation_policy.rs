use crate::settings::DisableImageGenerationMode;
use serde_json::Value;

const IMAGE_GENERATION_TOOL: &str = "image_generation";

pub(crate) fn apply_disable_image_generation_policy(
    mut body: Value,
    endpoint: &str,
    mode: DisableImageGenerationMode,
) -> Value {
    if mode == DisableImageGenerationMode::Off
        || (mode == DisableImageGenerationMode::Chat && is_images_endpoint_path(endpoint))
    {
        return body;
    }

    remove_image_generation_tool(&mut body);
    remove_image_generation_tool_choice(&mut body);
    body
}

fn is_images_endpoint_path(endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(endpoint)
        .trim();
    if path.is_empty() {
        return false;
    }

    matches!(path, "/v1/images/generations" | "/v1/images/edits")
        || path.ends_with("/v1/images/generations")
        || path.ends_with("/v1/images/edits")
        || path.ends_with("/images/generations")
        || path.ends_with("/images/edits")
}

fn remove_image_generation_tool(body: &mut Value) {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };

    tools.retain(|tool| {
        tool.get("type")
            .and_then(Value::as_str)
            .is_none_or(|tool_type| tool_type != IMAGE_GENERATION_TOOL)
    });
}

fn remove_image_generation_tool_choice(body: &mut Value) {
    let should_remove = match body.get("tool_choice") {
        Some(Value::String(choice)) => choice.trim().eq_ignore_ascii_case(IMAGE_GENERATION_TOOL),
        Some(Value::Object(choice)) => {
            let choice_type = choice
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            choice_type.eq_ignore_ascii_case(IMAGE_GENERATION_TOOL)
                || (choice_type.eq_ignore_ascii_case("tool")
                    && choice
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| {
                            name.trim().eq_ignore_ascii_case(IMAGE_GENERATION_TOOL)
                        }))
        }
        _ => false,
    };

    if should_remove {
        body.as_object_mut().map(|obj| obj.remove("tool_choice"));
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::DisableImageGenerationMode;
    use serde_json::json;

    use super::apply_disable_image_generation_policy;

    #[test]
    fn off_leaves_payload_byte_for_byte_equivalent() {
        let payload = json!({
            "model": "gpt-5.4",
            "tools": [
                {"type": "image_generation", "output_format": "png"},
                {"type": "function", "function": {"name": "lookup"}}
            ],
            "tool_choice": {"type": "image_generation"},
            "parallel_tool_calls": true
        });

        let result = apply_disable_image_generation_policy(
            payload.clone(),
            "/v1/responses",
            DisableImageGenerationMode::Off,
        );

        assert_eq!(result, payload);
    }

    #[test]
    fn chat_removes_image_generation_on_chat_and_responses_endpoints() {
        for endpoint in [
            "/v1/responses",
            "/responses",
            "/v1/chat/completions",
            "/chat/completions",
        ] {
            let payload = image_payload();
            let result = apply_disable_image_generation_policy(
                payload,
                endpoint,
                DisableImageGenerationMode::Chat,
            );

            assert_eq!(
                result["tools"],
                json!([
                    {"type": "function", "function": {"name": "lookup"}},
                    {"type": "web_search"}
                ]),
                "endpoint={endpoint}"
            );
            assert!(result.get("tool_choice").is_none(), "endpoint={endpoint}");
        }
    }

    #[test]
    fn chat_keeps_image_generation_on_images_endpoints() {
        for endpoint in [
            "/v1/images/generations",
            "/images/generations",
            "/v1/images/edits",
            "/images/edits",
        ] {
            let payload = image_payload();
            let result = apply_disable_image_generation_policy(
                payload.clone(),
                endpoint,
                DisableImageGenerationMode::Chat,
            );

            assert_eq!(result, payload, "endpoint={endpoint}");
        }
    }

    #[test]
    fn all_removes_image_generation_on_chat_endpoint() {
        let result = apply_disable_image_generation_policy(
            image_payload(),
            "/v1/chat/completions",
            DisableImageGenerationMode::All,
        );

        assert_eq!(
            result["tools"],
            json!([
                {"type": "function", "function": {"name": "lookup"}},
                {"type": "web_search"}
            ])
        );
        assert!(result.get("tool_choice").is_none());
    }

    #[test]
    fn all_removes_image_generation_on_images_endpoints() {
        for endpoint in ["/v1/images/generations", "/v1/images/edits"] {
            let result = apply_disable_image_generation_policy(
                image_payload(),
                endpoint,
                DisableImageGenerationMode::All,
            );

            assert_eq!(
                result["tools"],
                json!([
                    {"type": "function", "function": {"name": "lookup"}},
                    {"type": "web_search"}
                ]),
                "endpoint={endpoint}"
            );
            assert!(result.get("tool_choice").is_none(), "endpoint={endpoint}");
        }
    }

    #[test]
    fn removes_tool_choice_by_image_generation_type_or_tool_name_but_not_auto_none_required() {
        for tool_choice in [
            json!({"type": "image_generation"}),
            json!({"type": "tool", "name": "image_generation"}),
            json!("image_generation"),
        ] {
            let mut payload = image_payload();
            payload["tool_choice"] = tool_choice;

            let result = apply_disable_image_generation_policy(
                payload,
                "/v1/responses",
                DisableImageGenerationMode::Chat,
            );

            assert!(result.get("tool_choice").is_none());
        }

        for tool_choice in [json!("auto"), json!("none"), json!("required")] {
            let mut payload = image_payload();
            payload["tool_choice"] = tool_choice.clone();

            let result = apply_disable_image_generation_policy(
                payload,
                "/v1/responses",
                DisableImageGenerationMode::Chat,
            );

            assert_eq!(result["tool_choice"], tool_choice);
        }

        let function_tool_choice = json!({"type": "function", "name": "image_generation"});
        let mut payload = image_payload();
        payload["tool_choice"] = function_tool_choice.clone();
        let result = apply_disable_image_generation_policy(
            payload,
            "/v1/responses",
            DisableImageGenerationMode::Chat,
        );
        assert_eq!(result["tool_choice"], function_tool_choice);
    }

    #[test]
    fn keeps_function_tool_named_image_generation() {
        let payload = json!({
            "tools": [
                {"type": "function", "function": {"name": "image_generation"}},
                {"type": "image_generation"},
                {"type": "function", "name": "image_generation"}
            ]
        });

        let result = apply_disable_image_generation_policy(
            payload,
            "/responses",
            DisableImageGenerationMode::Chat,
        );

        assert_eq!(
            result["tools"],
            json!([
                {"type": "function", "function": {"name": "image_generation"}},
                {"type": "function", "name": "image_generation"}
            ])
        );
    }

    #[test]
    fn preserves_remaining_tool_order_and_keeps_empty_tools_array() {
        let payload = json!({
            "tools": [
                {"type": "image_generation"},
                {"type": "function", "function": {"name": "first"}},
                {"type": "image_generation"},
                {"type": "web_search"},
                {"type": "function", "function": {"name": "last"}}
            ]
        });

        let result = apply_disable_image_generation_policy(
            payload,
            "/v1/responses",
            DisableImageGenerationMode::Chat,
        );

        assert_eq!(
            result["tools"],
            json!([
                {"type": "function", "function": {"name": "first"}},
                {"type": "web_search"},
                {"type": "function", "function": {"name": "last"}}
            ])
        );

        let only_image = json!({"tools": [{"type": "image_generation"}]});
        let result = apply_disable_image_generation_policy(
            only_image,
            "/v1/responses",
            DisableImageGenerationMode::Chat,
        );
        assert_eq!(result["tools"], json!([]));
    }

    fn image_payload() -> serde_json::Value {
        json!({
            "model": "gpt-5.4",
            "tools": [
                {"type": "image_generation", "output_format": "png"},
                {"type": "function", "function": {"name": "lookup"}},
                {"type": "web_search"}
            ],
            "tool_choice": {"type": "image_generation"},
            "metadata": {"client": "codex"}
        })
    }
}
