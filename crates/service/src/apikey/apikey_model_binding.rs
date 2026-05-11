pub(crate) fn model_binding_slugs(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(items) = value.as_array() {
            return dedupe_model_slugs(
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string),
            );
        }
    }

    dedupe_model_slugs(raw.split(',').map(str::to_string))
}

pub(crate) fn forced_model_slug(raw: Option<&str>) -> Option<String> {
    let models = model_binding_slugs(raw);
    if models.len() == 1 {
        models.into_iter().next()
    } else {
        None
    }
}

pub(crate) fn request_model_alias(
    raw_model: Option<&str>,
    binding: Option<&str>,
) -> Option<String> {
    let raw_model = raw_model.map(str::trim).filter(|value| !value.is_empty())?;
    let Some(source_model) = raw_model.strip_prefix("anthropic-") else {
        return None;
    };
    let allowed = model_binding_slugs(binding);
    if allowed.is_empty() || allowed.iter().any(|model| model == source_model) {
        Some(source_model.to_string())
    } else {
        None
    }
}

pub(crate) fn serialize_model_bindings(models: Vec<String>) -> Option<String> {
    let models = dedupe_model_slugs(models);
    match models.as_slice() {
        [] => None,
        [single] => Some(single.clone()),
        _ => serde_json::to_string(&models).ok(),
    }
}

fn dedupe_model_slugs<I>(models: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut result = Vec::new();
    for model in models {
        let model = model.trim();
        if model.is_empty() || model == "auto" {
            continue;
        }
        if !result.iter().any(|item| item == model) {
            result.push(model.to_string());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_single_model() {
        assert_eq!(
            model_binding_slugs(Some("gpt-5.5")),
            vec!["gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parses_json_model_list_and_dedupes() {
        assert_eq!(
            model_binding_slugs(Some(r#"["gpt-5.5","", "gpt-5.5", "glm-5.1"]"#)),
            vec!["gpt-5.5".to_string(), "glm-5.1".to_string()]
        );
    }

    #[test]
    fn serializes_single_model_as_legacy_string() {
        assert_eq!(
            serialize_model_bindings(vec!["gpt-5.5".to_string()]),
            Some("gpt-5.5".to_string())
        );
    }

    #[test]
    fn multi_model_binding_is_not_forced_override() {
        assert_eq!(forced_model_slug(Some(r#"["gpt-5.5","glm-5.1"]"#)), None);
        assert_eq!(
            forced_model_slug(Some("gpt-5.5")),
            Some("gpt-5.5".to_string())
        );
    }

    #[test]
    fn anthropic_alias_maps_back_to_bound_source_model() {
        assert_eq!(
            request_model_alias(
                Some("anthropic-mimo-v2.5-pro"),
                Some(r#"["mimo-v2.5-pro"]"#)
            ),
            Some("mimo-v2.5-pro".to_string())
        );
    }
}
