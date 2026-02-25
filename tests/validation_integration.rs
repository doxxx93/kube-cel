#![cfg(feature = "validation")]

//! Integration tests for the validation module.
//!
//! End-to-end tests with realistic CRD schemas, matching the plan's
//! usage example and covering nested schemas, transition rules, and arrays.

use kube_cel::validation::{Validator, validate};
use serde_json::json;

#[test]
fn plan_usage_example() {
    // Matches the usage example from VALIDATION_PIPELINE_PLAN.md
    let schema: serde_json::Value = serde_json::from_str(
        r#"{
      "type": "object",
      "properties": {
        "spec": {
          "type": "object",
          "properties": {
            "replicas": {
              "type": "integer",
              "x-kubernetes-validations": [
                {"rule": "self >= 0", "message": "replicas must be non-negative"}
              ]
            },
            "minReplicas": {
              "type": "integer"
            }
          },
          "x-kubernetes-validations": [
            {"rule": "self.minReplicas <= self.replicas"}
          ]
        }
      }
    }"#,
    )
    .unwrap();

    let object = json!({
        "spec": {
            "replicas": -1,
            "minReplicas": 0
        }
    });

    let validator = Validator::new();
    let errors = validator.validate(&schema, &object, None);

    assert_eq!(errors.len(), 2);

    // spec-level: minReplicas <= replicas fails (0 <= -1 is false)
    let spec_err = errors.iter().find(|e| e.field_path == "spec").unwrap();
    assert!(
        spec_err
            .message
            .contains("self.minReplicas <= self.replicas")
    );

    // replicas-level: self >= 0 fails
    let rep_err = errors
        .iter()
        .find(|e| e.field_path == "spec.replicas")
        .unwrap();
    assert_eq!(rep_err.message, "replicas must be non-negative");
}

#[test]
fn full_crd_schema_passing() {
    let schema = json!({
        "type": "object",
        "properties": {
            "spec": {
                "type": "object",
                "properties": {
                    "replicas": {
                        "type": "integer",
                        "x-kubernetes-validations": [
                            {"rule": "self >= 0", "message": "replicas must be non-negative"}
                        ]
                    }
                },
                "x-kubernetes-validations": [
                    {"rule": "self.replicas >= 1", "message": "at least one replica"}
                ]
            }
        }
    });

    let obj = json!({"spec": {"replicas": 3}});
    let errors = validate(&schema, &obj, None);
    assert!(errors.is_empty());
}

#[test]
fn transition_rule_end_to_end() {
    let schema = json!({
        "type": "object",
        "properties": {
            "spec": {
                "type": "object",
                "properties": {
                    "replicas": {"type": "integer"}
                },
                "x-kubernetes-validations": [
                    {
                        "rule": "self.replicas >= oldSelf.replicas",
                        "message": "cannot scale down",
                        "reason": "FieldValueForbidden"
                    }
                ]
            }
        }
    });

    // Scale up: OK
    let obj = json!({"spec": {"replicas": 5}});
    let old = json!({"spec": {"replicas": 3}});
    let errors = validate(&schema, &obj, Some(&old));
    assert!(errors.is_empty());

    // Scale down: fails
    let obj2 = json!({"spec": {"replicas": 1}});
    let errors2 = validate(&schema, &obj2, Some(&old));
    assert_eq!(errors2.len(), 1);
    assert_eq!(errors2[0].message, "cannot scale down");
    assert_eq!(errors2[0].reason.as_deref(), Some("FieldValueForbidden"));
    assert_eq!(errors2[0].field_path, "spec");

    // Create (no old): transition rule skipped
    let errors3 = validate(&schema, &obj2, None);
    assert!(errors3.is_empty());
}

#[test]
fn nested_array_items_validation() {
    let schema = json!({
        "type": "object",
        "properties": {
            "spec": {
                "type": "object",
                "properties": {
                    "containers": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "image": {"type": "string"}
                            },
                            "x-kubernetes-validations": [
                                {"rule": "self.name.size() > 0", "message": "container name required"},
                                {"rule": "self.image.size() > 0", "message": "container image required"}
                            ]
                        }
                    }
                }
            }
        }
    });

    let obj = json!({
        "spec": {
            "containers": [
                {"name": "nginx", "image": "nginx:latest"},
                {"name": "", "image": "busybox"},
                {"name": "sidecar", "image": ""}
            ]
        }
    });

    let errors = validate(&schema, &obj, None);
    assert_eq!(errors.len(), 2);

    let err0 = errors
        .iter()
        .find(|e| e.field_path == "spec.containers[1]")
        .unwrap();
    assert_eq!(err0.message, "container name required");

    let err1 = errors
        .iter()
        .find(|e| e.field_path == "spec.containers[2]")
        .unwrap();
    assert_eq!(err1.message, "container image required");
}

#[test]
fn multi_level_validations() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "has(self.spec)", "message": "spec is required"}
        ],
        "properties": {
            "spec": {
                "type": "object",
                "x-kubernetes-validations": [
                    {"rule": "self.replicas >= self.minReplicas", "message": "replicas >= minReplicas"}
                ],
                "properties": {
                    "replicas": {
                        "type": "integer",
                        "x-kubernetes-validations": [
                            {"rule": "self >= 0", "message": "non-negative replicas"}
                        ]
                    },
                    "minReplicas": {"type": "integer"}
                }
            }
        }
    });

    // All valid
    let obj = json!({"spec": {"replicas": 3, "minReplicas": 1}});
    assert!(validate(&schema, &obj, None).is_empty());

    // Multiple failures at different levels
    let obj2 = json!({"spec": {"replicas": -1, "minReplicas": 2}});
    let errors = validate(&schema, &obj2, None);
    // spec level: -1 >= 2 fails, replicas level: -1 >= 0 fails
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().any(|e| e.field_path == "spec"));
    assert!(errors.iter().any(|e| e.field_path == "spec.replicas"));
}

#[test]
fn convenience_function_works() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.x > 0", "message": "x must be positive"}
        ],
        "properties": {
            "x": {"type": "integer"}
        }
    });
    let obj = json!({"x": -1});
    let errors = validate(&schema, &obj, None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "x must be positive");
    assert!(errors[0].field_path.is_empty()); // root level
}

#[test]
fn empty_schema_no_errors() {
    let schema = json!({"type": "object"});
    let obj = json!({"anything": "goes"});
    assert!(validate(&schema, &obj, None).is_empty());
}

#[test]
#[cfg(feature = "strings")]
fn extension_functions_in_validation() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "x-kubernetes-validations": [
                    {"rule": "self.trim().lowerAscii().size() > 0", "message": "name must not be blank"}
                ]
            }
        }
    });

    let obj = json!({"name": "  Hello  "});
    assert!(validate(&schema, &obj, None).is_empty());

    let obj2 = json!({"name": "   "});
    let errors = validate(&schema, &obj2, None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "name must not be blank");
}

#[test]
fn array_items_with_transition_rule() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tags": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "integer"}
                    },
                    "x-kubernetes-validations": [
                        {"rule": "self.value >= oldSelf.value", "message": "tag value cannot decrease"}
                    ]
                }
            }
        }
    });

    let obj = json!({"tags": [{"value": 5}, {"value": 2}]});
    let old = json!({"tags": [{"value": 3}, {"value": 4}]});
    let errors = validate(&schema, &obj, Some(&old));

    // tags[0]: 5 >= 3 → OK
    // tags[1]: 2 >= 4 → FAIL
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field_path, "tags[1]");
    assert_eq!(errors[0].message, "tag value cannot decrease");
}
