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

// ── Phase 4: Comprehensive edge case tests ──────────────────────────

#[test]
fn deeply_nested_objects() {
    let schema = json!({
        "type": "object",
        "properties": {
            "a": {
                "type": "object",
                "properties": {
                    "b": {
                        "type": "object",
                        "properties": {
                            "c": {
                                "type": "object",
                                "properties": {
                                    "value": {"type": "integer"}
                                },
                                "x-kubernetes-validations": [
                                    {"rule": "self.value > 0", "message": "deep value must be positive"}
                                ]
                            }
                        }
                    }
                }
            }
        }
    });

    let obj = json!({"a": {"b": {"c": {"value": -1}}}});
    let errors = validate(&schema, &obj, None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field_path, "a.b.c");
    assert_eq!(errors[0].message, "deep value must be positive");
}

#[test]
fn empty_array_no_item_validation() {
    let schema = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "integer",
                    "x-kubernetes-validations": [
                        {"rule": "self > 0", "message": "must be positive"}
                    ]
                }
            }
        }
    });

    let obj = json!({"items": []});
    let errors = validate(&schema, &obj, None);
    assert!(errors.is_empty());
}

#[test]
fn null_field_value() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.name == null || self.name.size() > 0", "message": "name must be null or non-empty"}
        ],
        "properties": {
            "name": {"type": "string"}
        }
    });

    // null name: passes the rule
    let obj = json!({"name": null});
    assert!(validate(&schema, &obj, None).is_empty());
}

#[test]
fn cel_exists_macro() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.items.exists(x, x > 3)", "message": "need at least one item > 3"}
        ],
        "properties": {
            "items": {"type": "array", "items": {"type": "integer"}}
        }
    });

    let pass = json!({"items": [1, 2, 5]});
    assert!(validate(&schema, &pass, None).is_empty());

    let fail = json!({"items": [1, 2, 3]});
    let errors = validate(&schema, &fail, None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "need at least one item > 3");
}

#[test]
fn cel_all_macro() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.tags.all(t, t.size() > 0)", "message": "all tags must be non-empty"}
        ],
        "properties": {
            "tags": {"type": "array", "items": {"type": "string"}}
        }
    });

    let pass = json!({"tags": ["a", "bb", "ccc"]});
    assert!(validate(&schema, &pass, None).is_empty());

    let fail = json!({"tags": ["a", "", "c"]});
    let errors = validate(&schema, &fail, None);
    assert_eq!(errors.len(), 1);
}

#[test]
fn cel_map_and_filter() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.nums.filter(n, n > 0).size() >= 2", "message": "need at least 2 positive numbers"}
        ],
        "properties": {
            "nums": {"type": "array", "items": {"type": "integer"}}
        }
    });

    let pass = json!({"nums": [-1, 2, 3]});
    assert!(validate(&schema, &pass, None).is_empty());

    let fail = json!({"nums": [-1, 2, -3]});
    let errors = validate(&schema, &fail, None);
    assert_eq!(errors.len(), 1);
}

#[test]
fn cel_ternary_expression() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.enabled ? self.count > 0 : true", "message": "count required when enabled"}
        ],
        "properties": {
            "enabled": {"type": "boolean"},
            "count": {"type": "integer"}
        }
    });

    // enabled=true, count=5: OK
    assert!(validate(&schema, &json!({"enabled": true, "count": 5}), None).is_empty());
    // enabled=false, count=0: OK (skipped by ternary)
    assert!(validate(&schema, &json!({"enabled": false, "count": 0}), None).is_empty());
    // enabled=true, count=0: FAIL
    let errors = validate(&schema, &json!({"enabled": true, "count": 0}), None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "count required when enabled");
}

#[test]
fn mixed_transition_and_non_transition_rules() {
    let schema = json!({
        "type": "object",
        "x-kubernetes-validations": [
            {"rule": "self.replicas >= 0", "message": "non-negative"},
            {"rule": "self.replicas >= oldSelf.replicas", "message": "cannot scale down"}
        ],
        "properties": {
            "replicas": {"type": "integer"}
        }
    });

    // Create (no old): only non-transition rule evaluated
    let errors = validate(&schema, &json!({"replicas": -1}), None);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "non-negative");

    // Update: both rules evaluated
    let errors = validate(
        &schema,
        &json!({"replicas": -1}),
        Some(&json!({"replicas": 3})),
    );
    assert_eq!(errors.len(), 2);
}

#[test]
fn array_length_mismatch_with_old_self() {
    let schema = json!({
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "items": {
                    "type": "integer",
                    "x-kubernetes-validations": [
                        {"rule": "self >= oldSelf", "message": "cannot decrease"}
                    ]
                }
            }
        }
    });

    // New array is longer: items[2] has no oldSelf → transition rule skipped
    let obj = json!({"items": [5, 3, 10]});
    let old = json!({"items": [3, 4]});
    let errors = validate(&schema, &obj, Some(&old));
    // items[0]: 5 >= 3 OK, items[1]: 3 >= 4 FAIL, items[2]: no oldSelf → skipped
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].field_path, "items[1]");
}

#[test]
fn realistic_istio_like_crd() {
    // Simplified Istio VirtualService-like schema
    let schema = json!({
        "type": "object",
        "properties": {
            "spec": {
                "type": "object",
                "x-kubernetes-validations": [
                    {"rule": "size(self.hosts) > 0", "message": "at least one host required"}
                ],
                "properties": {
                    "hosts": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "http": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "x-kubernetes-validations": [
                                {"rule": "size(self.route) > 0", "message": "at least one route required"}
                            ],
                            "properties": {
                                "route": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "weight": {"type": "integer"}
                                        },
                                        "x-kubernetes-validations": [
                                            {
                                                "rule": "self.weight >= 0 && self.weight <= 100",
                                                "message": "weight must be 0-100",
                                                "reason": "FieldValueInvalid"
                                            }
                                        ]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    // Valid config
    let valid = json!({
        "spec": {
            "hosts": ["example.com"],
            "http": [{
                "route": [
                    {"weight": 80},
                    {"weight": 20}
                ]
            }]
        }
    });
    assert!(validate(&schema, &valid, None).is_empty());

    // Multiple failures at different levels
    let invalid = json!({
        "spec": {
            "hosts": [],
            "http": [{
                "route": [
                    {"weight": 150},
                    {"weight": -10}
                ]
            }]
        }
    });
    let errors = validate(&schema, &invalid, None);
    // spec: hosts empty, route[0]: weight 150, route[1]: weight -10
    assert_eq!(errors.len(), 3);
    assert!(errors.iter().any(|e| e.field_path == "spec"));
    assert!(
        errors
            .iter()
            .any(|e| e.field_path == "spec.http[0].route[0]")
    );
    assert!(
        errors
            .iter()
            .any(|e| e.field_path == "spec.http[0].route[1]")
    );
}

#[test]
fn realistic_cert_manager_like_crd() {
    // Simplified cert-manager Certificate-like schema
    let schema = json!({
        "type": "object",
        "properties": {
            "spec": {
                "type": "object",
                "x-kubernetes-validations": [
                    {
                        "rule": "has(self.dnsNames) || has(self.ipAddresses)",
                        "message": "at least one of dnsNames or ipAddresses is required"
                    },
                    {
                        "rule": "self.renewBefore < self.duration",
                        "message": "renewBefore must be less than duration"
                    }
                ],
                "properties": {
                    "dnsNames": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "ipAddresses": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "duration": {"type": "integer"},
                    "renewBefore": {"type": "integer"}
                }
            }
        }
    });

    let valid = json!({
        "spec": {
            "dnsNames": ["example.com"],
            "duration": 90,
            "renewBefore": 30
        }
    });
    assert!(validate(&schema, &valid, None).is_empty());

    let invalid = json!({
        "spec": {
            "duration": 30,
            "renewBefore": 60
        }
    });
    let errors = validate(&schema, &invalid, None);
    // Missing both dnsNames and ipAddresses, AND renewBefore > duration
    assert_eq!(errors.len(), 2);
}

// ── kube-rs compatibility ───────────────────────────────────────────

#[test]
fn kube_core_rule_json_compatibility() {
    use kube_cel::compilation::compile_schema_validations;

    // JSON format matching kube-core's Rule serialization output
    let schema = json!({
        "x-kubernetes-validations": [
            {
                "rule": "self.spec.host == self.url.host",
                "message": "host must match spec.host",
                "fieldPath": "spec.host",
                "reason": "FieldValueInvalid"
            },
            {
                "rule": "oldSelf.name == self.name",
                "messageExpression": "'name changed from ' + oldSelf.name + ' to ' + self.name",
                "reason": "FieldValueForbidden"
            },
            {
                "rule": "self.replicas >= 0"
            }
        ]
    });

    let results = compile_schema_validations(&schema);
    assert_eq!(results.len(), 3);

    // First rule: all fields populated
    let r0 = results[0].as_ref().unwrap();
    assert_eq!(
        r0.rule.message.as_deref(),
        Some("host must match spec.host")
    );
    assert_eq!(r0.rule.field_path.as_deref(), Some("spec.host"));
    assert_eq!(r0.rule.reason.as_deref(), Some("FieldValueInvalid"));
    assert!(!r0.is_transition_rule);

    // Second rule: messageExpression + transition rule
    let r1 = results[1].as_ref().unwrap();
    assert!(r1.rule.message.is_none());
    assert!(r1.rule.message_expression.is_some());
    assert_eq!(r1.rule.reason.as_deref(), Some("FieldValueForbidden"));
    assert!(r1.is_transition_rule);

    // Third rule: minimal (only rule field)
    let r2 = results[2].as_ref().unwrap();
    assert!(r2.rule.message.is_none());
    assert!(r2.rule.reason.is_none());
    assert!(r2.rule.field_path.is_none());
}

#[test]
fn kube_core_reason_values() {
    use kube_cel::compilation::compile_schema_validations;

    // All Reason variants from kube-core
    let schema = json!({
        "x-kubernetes-validations": [
            {"rule": "true", "reason": "FieldValueInvalid"},
            {"rule": "true", "reason": "FieldValueForbidden"},
            {"rule": "true", "reason": "FieldValueRequired"},
            {"rule": "true", "reason": "FieldValueDuplicate"}
        ]
    });

    let results = compile_schema_validations(&schema);
    assert_eq!(results.len(), 4);
    assert_eq!(
        results[0].as_ref().unwrap().rule.reason.as_deref(),
        Some("FieldValueInvalid")
    );
    assert_eq!(
        results[1].as_ref().unwrap().rule.reason.as_deref(),
        Some("FieldValueForbidden")
    );
    assert_eq!(
        results[2].as_ref().unwrap().rule.reason.as_deref(),
        Some("FieldValueRequired")
    );
    assert_eq!(
        results[3].as_ref().unwrap().rule.reason.as_deref(),
        Some("FieldValueDuplicate")
    );
}
