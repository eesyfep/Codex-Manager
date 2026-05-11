use super::*;
use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};
use serde_json::Value;

/// 函数 `serialize_models_response_outputs_official_shape`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn serialize_models_response_outputs_official_shape() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "gpt-5.3-codex".to_string(),
                display_name: "GPT-5.3 Codex".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "gpt-4o".to_string(),
                display_name: "GPT-4o".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
        ],
        extra: std::collections::BTreeMap::from([(
            "etag".to_string(),
            serde_json::json!("\"abc\""),
        )]),
        ..Default::default()
    };
    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");
    assert_eq!(models.len(), 3);
    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("gpt-5.3-codex")
    );
    assert_eq!(
        models[1].get("slug").and_then(Value::as_str),
        Some("gpt-4o")
    );
    assert_eq!(
        models[0].get("display_name").and_then(Value::as_str),
        Some("GPT-5.3 Codex")
    );
    assert_eq!(
        models[1].get("visibility").and_then(Value::as_str),
        Some("list")
    );
    assert_eq!(
        models[2].get("slug").and_then(Value::as_str),
        Some("gpt-image-2")
    );
    assert_eq!(value.as_object().map(|object| object.len()), Some(1));
    assert!(value.get("etag").is_none());
}

#[test]
fn serialize_models_response_preserves_description_for_codex_clients() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "gpt-5.3-codex".to_string(),
            display_name: "GPT-5.3 Codex".to_string(),
            description: Some("Latest frontier agentic coding model.".to_string()),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get("description").and_then(Value::as_str),
        Some("Latest frontier agentic coding model.")
    );
    assert_eq!(
        models[0].get("shell_type").and_then(Value::as_str),
        Some("shell_command")
    );
    assert_eq!(
        models[0]
            .get("truncation_policy")
            .and_then(Value::as_object)
            .and_then(|policy| policy.get("mode"))
            .and_then(Value::as_str),
        Some("tokens")
    );
}

#[test]
fn serialize_openai_models_response_uses_bound_model_data_shape() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "mimo-v2.5-pro".to_string(),
            display_name: "MiMo V2.5 Pro".to_string(),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_openai_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(value.get("object").and_then(Value::as_str), Some("list"));
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .expect("data array");
    assert_eq!(data.len(), 1);
    assert_eq!(
        data[0].get("id").and_then(Value::as_str),
        Some("mimo-v2.5-pro")
    );
    assert_eq!(
        data[0].get("display_name").and_then(Value::as_str),
        Some("MiMo V2.5 Pro")
    );
}

#[test]
fn serialize_anthropic_models_response_prefixes_non_claude_ids() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "mimo-v2.5-pro".to_string(),
                display_name: "MiMo V2.5 Pro".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "claude-sonnet-4-6".to_string(),
                display_name: "Claude Sonnet 4.6".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let output = serialize_anthropic_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .expect("data array");
    assert_eq!(data.len(), 2);
    assert_eq!(
        data[0].get("id").and_then(Value::as_str),
        Some("anthropic-mimo-v2.5-pro")
    );
    assert_eq!(
        data[1].get("id").and_then(Value::as_str),
        Some("claude-sonnet-4-6")
    );
    assert_eq!(value.get("has_more").and_then(Value::as_bool), Some(false));
    assert_eq!(
        value.get("first_id").and_then(Value::as_str),
        Some("anthropic-mimo-v2.5-pro")
    );
}

#[test]
fn filter_models_for_platform_key_keeps_only_bound_models() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "gpt-5.5".to_string(),
                display_name: "GPT 5.5".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "mimo-v2.5-pro".to_string(),
                display_name: "MiMo V2.5 Pro".to_string(),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let filtered = filter_models_for_platform_key(&items, Some(r#"["mimo-v2.5-pro"]"#));
    assert_eq!(filtered.models.len(), 1);
    assert_eq!(filtered.models[0].slug, "mimo-v2.5-pro");
}

#[test]
fn serialize_models_response_filters_hidden_and_non_api_models() {
    let items = ModelsResponse {
        models: vec![
            ModelInfo {
                slug: "mimo-v2.5-pro".to_string(),
                display_name: "mimo-v2.5-pro".to_string(),
                description: Some("Third-party visible model".to_string()),
                supported_in_api: true,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "hidden-third-party".to_string(),
                display_name: "hidden-third-party".to_string(),
                supported_in_api: true,
                visibility: Some("hide".to_string()),
                ..Default::default()
            },
            ModelInfo {
                slug: "disabled-third-party".to_string(),
                display_name: "disabled-third-party".to_string(),
                supported_in_api: false,
                visibility: Some("list".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");

    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("mimo-v2.5-pro")
    );
    assert_eq!(
        models[1].get("slug").and_then(Value::as_str),
        Some("gpt-image-2")
    );
}

#[test]
fn serialize_models_response_appends_codex_image_tool_model_once() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "gpt-image-2".to_string(),
            display_name: "GPT Image 2".to_string(),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");

    assert_eq!(models.len(), 1);
    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("gpt-image-2")
    );
}

#[test]
fn models_etag_header_uses_extra_etag_value() {
    let items = ModelsResponse {
        models: vec![],
        extra: std::collections::BTreeMap::from([(
            "etag".to_string(),
            serde_json::json!("\"remote-etag\""),
        )]),
    };

    let header = models_etag_header(&items)
        .expect("etag header should build")
        .expect("etag header should exist");

    assert!(header.field.equiv("etag"));
    assert_eq!(header.value.as_str(), "\"remote-etag\"");
}

#[test]
fn serialize_models_response_keeps_visible_third_party_models_with_codex_defaults() {
    let items = ModelsResponse {
        models: vec![ModelInfo {
            slug: "mimo-v2.5-pro".to_string(),
            display_name: "MiMo V2.5 Pro".to_string(),
            description: Some("chat adapter route".to_string()),
            supported_in_api: true,
            visibility: Some("list".to_string()),
            extra: std::collections::BTreeMap::from([(
                "source_kind".to_string(),
                serde_json::json!("remote"),
            )]),
            ..Default::default()
        }],
        ..Default::default()
    };

    let output = serialize_models_response(&items);
    let value: Value = serde_json::from_str(&output).expect("valid json");
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .expect("models array");

    assert_eq!(
        models[0].get("slug").and_then(Value::as_str),
        Some("mimo-v2.5-pro")
    );
    assert_eq!(
        models[0].get("source_kind").and_then(Value::as_str),
        Some("remote")
    );
    assert_eq!(
        models[0].get("supported_in_api").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        models[0]
            .get("input_modalities")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(2)
    );
    assert_eq!(
        models[0].get("base_instructions").and_then(Value::as_str),
        Some("You are Codex, a helpful AI assistant. Follow the user's instructions.")
    );
    assert_eq!(
        models[0]
            .get("model_messages")
            .and_then(Value::as_object)
            .and_then(|value| value.get("instructions_template"))
            .and_then(Value::as_str),
        Some("You are Codex, a coding agent based on GPT-5. You and the user share one workspace, and your job is to collaborate with them until their goal is genuinely handled.\n\n{{personality}}\n")
    );
    assert_eq!(
        models[0]
            .get("model_messages")
            .and_then(Value::as_object)
            .and_then(|value| value.get("instructions_variables"))
            .and_then(Value::as_object)
            .and_then(|value| value.get("personality_pragmatic"))
            .and_then(Value::as_str)
            .map(|value| value.contains("deeply pragmatic, effective software engineer")),
        Some(true)
    );
}
