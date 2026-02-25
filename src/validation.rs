//! Schema tree walking and CEL rule evaluation for Kubernetes CRD validation.
//!
//! This module provides [`Validator`] which recursively walks an OpenAPI schema,
//! compiles `x-kubernetes-validations` rules, evaluates them against object data,
//! and collects [`ValidationError`]s.

use crate::compilation::{CompilationError, CompilationResult, compile_schema_validations};
use crate::values::json_to_cel;
use cel::Context;

/// An error produced when a CEL validation rule fails.
#[derive(Clone, Debug)]
pub struct ValidationError {
    /// The CEL expression that failed.
    pub rule: String,
    /// Human-readable error message.
    pub message: String,
    /// JSON path to the field (e.g., "spec.replicas").
    pub field_path: String,
    /// Machine-readable reason (e.g., "FieldValueInvalid").
    pub reason: Option<String>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.field_path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.field_path, self.message)
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validates Kubernetes objects against CRD schema CEL validation rules.
///
/// Walks the OpenAPI schema tree, compiles `x-kubernetes-validations` rules at
/// each node, and evaluates them against the corresponding object values.
///
/// Currently stateless — Phase 5 may add compilation caching.
pub struct Validator {
    _private: (),
}

impl Validator {
    /// Create a new `Validator`.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Validate an object against a CRD schema's CEL validation rules.
    ///
    /// Recursively walks the schema tree, evaluating `x-kubernetes-validations`
    /// at each node. Transition rules (referencing `oldSelf`) are only evaluated
    /// when `old_object` is provided.
    pub fn validate(
        &self,
        schema: &serde_json::Value,
        object: &serde_json::Value,
        old_object: Option<&serde_json::Value>,
    ) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        self.walk_schema(schema, object, old_object, String::new(), &mut errors);
        errors
    }

    fn walk_schema(
        &self,
        schema: &serde_json::Value,
        value: &serde_json::Value,
        old_value: Option<&serde_json::Value>,
        path: String,
        errors: &mut Vec<ValidationError>,
    ) {
        // Evaluate validations at this node
        self.evaluate_validations(schema, value, old_value, &path, errors);

        // Recurse into properties (only if value is an object)
        if let (Some(properties), Some(obj)) = (
            schema.get("properties").and_then(|p| p.as_object()),
            value.as_object(),
        ) {
            for (prop_name, prop_schema) in properties {
                if let Some(child_value) = obj.get(prop_name) {
                    let child_old = old_value.and_then(|o| o.get(prop_name));
                    let child_path = if path.is_empty() {
                        prop_name.clone()
                    } else {
                        format!("{path}.{prop_name}")
                    };
                    self.walk_schema(prop_schema, child_value, child_old, child_path, errors);
                }
            }
        }

        // Recurse into array items (only if value is an array)
        if let (Some(items_schema), Some(arr)) = (schema.get("items"), value.as_array()) {
            for (i, item) in arr.iter().enumerate() {
                let old_item = old_value.and_then(|o| o.as_array()).and_then(|a| a.get(i));
                let item_path = if path.is_empty() {
                    format!("[{i}]")
                } else {
                    format!("{path}[{i}]")
                };
                self.walk_schema(items_schema, item, old_item, item_path, errors);
            }
        }

        // Recurse into additionalProperties (only for keys not in properties)
        if let (Some(additional_schema), Some(obj)) = (
            schema.get("additionalProperties").filter(|a| a.is_object()),
            value.as_object(),
        ) {
            let known: std::collections::HashSet<&str> = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .map(|p| p.keys().map(|k| k.as_str()).collect())
                .unwrap_or_default();

            for (key, val) in obj {
                if known.contains(key.as_str()) {
                    continue;
                }
                let old_val = old_value.and_then(|o| o.get(key));
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                self.walk_schema(additional_schema, val, old_val, child_path, errors);
            }
        }
    }

    fn evaluate_validations(
        &self,
        schema: &serde_json::Value,
        value: &serde_json::Value,
        old_value: Option<&serde_json::Value>,
        path: &str,
        errors: &mut Vec<ValidationError>,
    ) {
        let compiled = compile_schema_validations(schema);

        for result in compiled {
            match result {
                Ok(cr) => {
                    self.evaluate_rule(&cr, value, old_value, path, errors);
                }
                Err(CompilationError::Parse { rule, source }) => {
                    errors.push(ValidationError {
                        rule: rule.clone(),
                        message: format!("failed to compile rule \"{rule}\": {source}"),
                        field_path: path.to_string(),
                        reason: None,
                    });
                }
                Err(CompilationError::InvalidRule(e)) => {
                    errors.push(ValidationError {
                        rule: String::new(),
                        message: format!("invalid rule definition: {e}"),
                        field_path: path.to_string(),
                        reason: None,
                    });
                }
            }
        }
    }

    fn evaluate_rule(
        &self,
        cr: &CompilationResult,
        value: &serde_json::Value,
        old_value: Option<&serde_json::Value>,
        path: &str,
        errors: &mut Vec<ValidationError>,
    ) {
        // Skip transition rules when no old value is available
        if cr.is_transition_rule && old_value.is_none() {
            return;
        }

        let mut ctx = Context::default();
        crate::register_all(&mut ctx);
        ctx.add_variable_from_value("self", json_to_cel(value));

        if let Some(old) = old_value {
            ctx.add_variable_from_value("oldSelf", json_to_cel(old));
        }

        match cr.program.execute(&ctx) {
            Ok(cel::Value::Bool(true)) => {
                // Validation passed
            }
            Ok(cel::Value::Bool(false)) => {
                let message = cr
                    .rule
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("failed rule: {}", cr.rule.rule));
                errors.push(ValidationError {
                    rule: cr.rule.rule.clone(),
                    message,
                    field_path: path.to_string(),
                    reason: cr.rule.reason.clone(),
                });
            }
            Ok(_) => {
                errors.push(ValidationError {
                    rule: cr.rule.rule.clone(),
                    message: format!("rule \"{}\" did not evaluate to bool", cr.rule.rule),
                    field_path: path.to_string(),
                    reason: None,
                });
            }
            Err(e) => {
                errors.push(ValidationError {
                    rule: cr.rule.rule.clone(),
                    message: format!("rule evaluation error: {e}"),
                    field_path: path.to_string(),
                    reason: None,
                });
            }
        }
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to validate without creating a [`Validator`] instance.
///
/// See [`Validator::validate`] for details.
pub fn validate(
    schema: &serde_json::Value,
    object: &serde_json::Value,
    old_object: Option<&serde_json::Value>,
) -> Vec<ValidationError> {
    Validator::new().validate(schema, object, old_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_schema(validations: serde_json::Value) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "replicas": {"type": "integer"},
                "name": {"type": "string"}
            },
            "x-kubernetes-validations": validations
        })
    }

    #[test]
    fn validation_passes() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= 0", "message": "must be non-negative"}
        ]));
        let obj = json!({"replicas": 3, "name": "app"});
        let errors = validate(&schema, &obj, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn validation_fails() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= 0", "message": "must be non-negative"}
        ]));
        let obj = json!({"replicas": -1, "name": "app"});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "must be non-negative");
        assert_eq!(errors[0].rule, "self.replicas >= 0");
    }

    #[test]
    fn default_message_when_none() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= 0"}
        ]));
        let obj = json!({"replicas": -1, "name": "app"});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("self.replicas >= 0"));
    }

    #[test]
    fn reason_preserved() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= 0", "message": "bad", "reason": "FieldValueInvalid"}
        ]));
        let obj = json!({"replicas": -1, "name": "app"});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors[0].reason.as_deref(), Some("FieldValueInvalid"));
    }

    #[test]
    fn transition_rule_skipped_without_old_object() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= oldSelf.replicas", "message": "cannot scale down"}
        ]));
        let obj = json!({"replicas": 1, "name": "app"});
        // No old_object → transition rule is skipped
        let errors = validate(&schema, &obj, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn transition_rule_evaluated_with_old_object() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= oldSelf.replicas", "message": "cannot scale down"}
        ]));
        let obj = json!({"replicas": 1, "name": "app"});
        let old = json!({"replicas": 3, "name": "app"});
        let errors = validate(&schema, &obj, Some(&old));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "cannot scale down");
    }

    #[test]
    fn transition_rule_passes() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= oldSelf.replicas", "message": "cannot scale down"}
        ]));
        let obj = json!({"replicas": 5, "name": "app"});
        let old = json!({"replicas": 3, "name": "app"});
        let errors = validate(&schema, &obj, Some(&old));
        assert!(errors.is_empty());
    }

    #[test]
    fn nested_property_field_path() {
        let schema = json!({
            "type": "object",
            "properties": {
                "spec": {
                    "type": "object",
                    "properties": {
                        "replicas": {
                            "type": "integer",
                            "x-kubernetes-validations": [
                                {"rule": "self >= 0", "message": "must be non-negative"}
                            ]
                        }
                    }
                }
            }
        });
        let obj = json!({"spec": {"replicas": -1}});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field_path, "spec.replicas");
        assert_eq!(errors[0].message, "must be non-negative");
    }

    #[test]
    fn array_items_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        },
                        "x-kubernetes-validations": [
                            {"rule": "self.name.size() > 0", "message": "name required"}
                        ]
                    }
                }
            }
        });
        let obj = json!({
            "items": [
                {"name": "good"},
                {"name": ""},
                {"name": "also-good"}
            ]
        });
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field_path, "items[1]");
        assert_eq!(errors[0].message, "name required");
    }

    #[test]
    fn missing_field_not_validated() {
        let schema = json!({
            "type": "object",
            "properties": {
                "optional_field": {
                    "type": "integer",
                    "x-kubernetes-validations": [
                        {"rule": "self >= 0", "message": "must be non-negative"}
                    ]
                }
            }
        });
        // optional_field is not present in the object
        let obj = json!({});
        let errors = validate(&schema, &obj, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn multiple_rules_partial_failure() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >= 0", "message": "non-negative"},
            {"rule": "self.name.size() > 0", "message": "name required"}
        ]));
        let obj = json!({"replicas": -1, "name": ""});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn compilation_error_reported() {
        let schema = make_schema(json!([
            {"rule": "self.replicas >="}
        ]));
        let obj = json!({"replicas": 1, "name": "app"});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("failed to compile"));
    }

    #[test]
    fn no_validations_no_errors() {
        let schema = json!({
            "type": "object",
            "properties": {
                "replicas": {"type": "integer"}
            }
        });
        let obj = json!({"replicas": -1});
        let errors = validate(&schema, &obj, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn display_with_field_path() {
        let err = ValidationError {
            rule: "self >= 0".into(),
            message: "must be non-negative".into(),
            field_path: "spec.replicas".into(),
            reason: None,
        };
        assert_eq!(err.to_string(), "spec.replicas: must be non-negative");
    }

    #[test]
    fn display_without_field_path() {
        let err = ValidationError {
            rule: "self >= 0".into(),
            message: "must be non-negative".into(),
            field_path: String::new(),
            reason: None,
        };
        assert_eq!(err.to_string(), "must be non-negative");
    }

    #[test]
    fn validator_default() {
        let v = Validator::default();
        let schema = make_schema(json!([{"rule": "self.replicas >= 0"}]));
        let obj = json!({"replicas": 1, "name": "app"});
        assert!(v.validate(&schema, &obj, None).is_empty());
    }

    #[test]
    fn additional_properties_walking() {
        let schema = json!({
            "type": "object",
            "additionalProperties": {
                "type": "integer",
                "x-kubernetes-validations": [
                    {"rule": "self >= 0", "message": "must be non-negative"}
                ]
            }
        });
        let obj = json!({"a": 1, "b": -1, "c": 5});
        let errors = validate(&schema, &obj, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field_path, "b");
    }
}
