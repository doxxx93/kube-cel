# Changelog

## [0.2.1] - 2026-02-25

### Fixed

- Gate validation examples with `required-features` (fixes `--no-default-features` build)

### Added

- Examples: `basic`, `validate_crd`, `compiled_schema`
- CHANGELOG.md
- Crate-level doc for `validation` feature

## [0.2.0] - 2026-02-25

### Added

- **CRD Validation Pipeline** (`validation` feature)
  - `values::json_to_cel()` — convert `serde_json::Value` to `cel::Value`
  - `compilation::compile_rule()` / `compile_schema_validations()` — compile `x-kubernetes-validations` CEL rules
  - `compilation::compile_schema()` / `CompiledSchema` — pre-compile entire schema trees for reuse
  - `validation::Validator` — walk schema trees, evaluate rules, collect errors
  - `validation::validate()` / `validate_compiled()` — convenience functions
  - `messageExpression` support with best-effort compilation and static fallback
  - `optionalOldSelf` support (transition rules evaluated on create with `oldSelf = null`)
  - Transition rule detection via `oldSelf` reference analysis
  - Schema tree walking: `properties`, `items`, `additionalProperties`
  - Field path tracking (e.g., `spec.containers[1]`)
  - kube-rs `kube-core::cel::Rule` JSON compatibility

## [0.1.1] - 2026-02-24

### Fixed

- Fix `cel-interpreter` references to `cel` crate after upstream rename

## [0.1.0] - 2026-02-24

### Added

- Kubernetes CEL extension functions: `strings`, `lists`, `sets`, `regex_funcs`, `urls`, `ip`, `semver_funcs`, `format`, `quantity`
- Unified type dispatch for shared function names (`indexOf`, `lastIndexOf`, `isGreaterThan`, `isLessThan`, `compareTo`)
- Feature flags for each function group (all enabled by default)
