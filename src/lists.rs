//! Kubernetes CEL list extension functions.
//!
//! Provides list functions available in Kubernetes CEL expressions,
//! matching the behavior of `k8s.io/apiserver/pkg/cel/library/lists.go`.

use cel::extractors::This;
use cel::objects::Value;
use cel::{Context, ExecutionError, ResolveResult};
use std::sync::Arc;

/// Register all list extension functions.
pub fn register(ctx: &mut Context<'_>) {
    ctx.add_function("isSorted", is_sorted);
    ctx.add_function("sum", sum);
    // Note: min/max are already built-in to cel-interpreter as variadic functions.
    // We register list-member versions here that operate on `<list>.min()` / `<list>.max()`.
    ctx.add_function("min", list_min);
    ctx.add_function("max", list_max);
    // indexOf/lastIndexOf are registered via dispatch module to handle
    // name collision between string and list versions.
    ctx.add_function("slice", slice);
    ctx.add_function("flatten", flatten);
    ctx.add_function("reverse", list_reverse);
    ctx.add_function("distinct", distinct);
}

/// `<list>.isSorted() -> bool`
///
/// Returns true if the list elements are in sorted (ascending) order.
fn is_sorted(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    for window in this.windows(2) {
        if !val_le(&window[0], &window[1])? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

/// `<list>.sum() -> T`
///
/// Returns the sum of all elements. Empty list returns 0 for int, 0u for uint, 0.0 for double.
fn sum(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    if this.is_empty() {
        return Ok(Value::Int(0));
    }

    let mut acc = this[0].clone();
    for item in this.iter().skip(1) {
        acc = val_add(&acc, item)?;
    }
    Ok(acc)
}

/// `<list>.min() -> T`
///
/// Returns the minimum element. Errors on empty list.
fn list_min(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    if this.is_empty() {
        return Err(cel::ExecutionError::function_error(
            "min",
            "cannot call min on empty list",
        ));
    }
    let mut result = this[0].clone();
    for item in this.iter().skip(1) {
        if val_lt(item, &result)? {
            result = item.clone();
        }
    }
    Ok(result)
}

/// `<list>.max() -> T`
///
/// Returns the maximum element. Errors on empty list.
fn list_max(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    if this.is_empty() {
        return Err(cel::ExecutionError::function_error(
            "max",
            "cannot call max on empty list",
        ));
    }
    let mut result = this[0].clone();
    for item in this.iter().skip(1) {
        if val_lt(&result, item)? {
            result = item.clone();
        }
    }
    Ok(result)
}

/// `<list>.indexOf(T) -> int`
///
/// Returns the index of the first occurrence of the value, or -1 if not found.
pub(crate) fn list_index_of(list: &[Value], args: &[Value]) -> ResolveResult {
    let target = args
        .first()
        .ok_or_else(|| ExecutionError::function_error("indexOf", "expected argument"))?;
    for (i, item) in list.iter().enumerate() {
        if val_eq(item, target) {
            return Ok(Value::Int(i as i64));
        }
    }
    Ok(Value::Int(-1))
}

/// `<list>.lastIndexOf(T) -> int`
///
/// Returns the index of the last occurrence of the value, or -1 if not found.
pub(crate) fn list_last_index_of(list: &[Value], args: &[Value]) -> ResolveResult {
    let target = args
        .first()
        .ok_or_else(|| ExecutionError::function_error("lastIndexOf", "expected argument"))?;
    let mut result: i64 = -1;
    for (i, item) in list.iter().enumerate() {
        if val_eq(item, target) {
            result = i as i64;
        }
    }
    Ok(Value::Int(result))
}

/// `<list>.slice(int, int) -> list`
///
/// Returns a sub-list from start (inclusive) to end (exclusive).
fn slice(This(this): This<Arc<Vec<Value>>>, start: i64, end: i64) -> ResolveResult {
    let len = this.len() as i64;
    if start < 0 || start > len || end < start || end > len {
        return Err(cel::ExecutionError::function_error(
            "slice",
            format!("slice({start}, {end}) out of range for list of length {len}"),
        ));
    }
    let result: Vec<Value> = this[start as usize..end as usize].to_vec();
    Ok(Value::List(Arc::new(result)))
}

/// `<list>.flatten() -> list`
///
/// Flattens a list of lists by one level.
fn flatten(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    let mut result = Vec::new();
    for item in this.iter() {
        match item {
            Value::List(inner) => result.extend(inner.iter().cloned()),
            other => result.push(other.clone()),
        }
    }
    Ok(Value::List(Arc::new(result)))
}

/// `<list>.reverse() -> list`
///
/// Returns a new list with elements in reverse order.
fn list_reverse(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    let mut result: Vec<Value> = this.iter().cloned().collect();
    result.reverse();
    Ok(Value::List(Arc::new(result)))
}

/// `<list>.distinct() -> list`
///
/// Returns a new list with duplicate elements removed, preserving order.
fn distinct(This(this): This<Arc<Vec<Value>>>) -> ResolveResult {
    let mut seen = Vec::new();
    let mut result = Vec::new();
    for item in this.iter() {
        if !seen.iter().any(|s| val_eq(s, item)) {
            seen.push(item.clone());
            result.push(item.clone());
        }
    }
    Ok(Value::List(Arc::new(result)))
}

// --- Helper functions for value comparison and arithmetic ---

fn val_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::UInt(a), Value::UInt(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        _ => false,
    }
}

fn val_lt(a: &Value, b: &Value) -> Result<bool, cel::ExecutionError> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Ok(a < b),
        (Value::UInt(a), Value::UInt(b)) => Ok(a < b),
        (Value::Float(a), Value::Float(b)) => Ok(a < b),
        (Value::String(a), Value::String(b)) => Ok(a < b),
        (Value::Bool(a), Value::Bool(b)) => Ok(!a & b),
        _ => Err(cel::ExecutionError::function_error(
            "compare",
            "cannot compare values of different types",
        )),
    }
}

fn val_le(a: &Value, b: &Value) -> Result<bool, cel::ExecutionError> {
    Ok(val_eq(a, b) || val_lt(a, b)?)
}

fn val_add(a: &Value, b: &Value) -> Result<Value, cel::ExecutionError> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::UInt(a), Value::UInt(b)) => Ok(Value::UInt(a + b)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        _ => Err(cel::ExecutionError::function_error(
            "sum",
            "cannot sum values of this type",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cel::Program;

    fn eval(expr: &str) -> Value {
        let mut ctx = Context::default();
        register(&mut ctx);
        crate::dispatch::register(&mut ctx);
        Program::compile(expr).unwrap().execute(&ctx).unwrap()
    }

    #[test]
    fn test_is_sorted() {
        assert_eq!(eval("[1, 2, 3].isSorted()"), Value::Bool(true));
        assert_eq!(eval("[3, 1, 2].isSorted()"), Value::Bool(false));
        assert_eq!(eval("[].isSorted()"), Value::Bool(true));
        assert_eq!(eval("['a', 'b', 'c'].isSorted()"), Value::Bool(true));
    }

    #[test]
    fn test_sum() {
        assert_eq!(eval("[1, 2, 3].sum()"), Value::Int(6));
        assert_eq!(eval("[1.5, 2.5].sum()"), Value::Float(4.0));
        assert_eq!(eval("[].sum()"), Value::Int(0));
    }

    #[test]
    fn test_min_max() {
        assert_eq!(eval("[3, 1, 2].min()"), Value::Int(1));
        assert_eq!(eval("[3, 1, 2].max()"), Value::Int(3));
    }

    #[test]
    fn test_index_of() {
        assert_eq!(eval("[1, 2, 3, 2].indexOf(2)"), Value::Int(1));
        assert_eq!(eval("[1, 2, 3].indexOf(4)"), Value::Int(-1));
    }

    #[test]
    fn test_last_index_of() {
        assert_eq!(eval("[1, 2, 3, 2].lastIndexOf(2)"), Value::Int(3));
    }

    #[test]
    fn test_slice() {
        assert_eq!(
            eval("[1, 2, 3, 4].slice(1, 3)"),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)]))
        );
    }

    #[test]
    fn test_flatten() {
        assert_eq!(
            eval("[[1, 2], [3, 4]].flatten()"),
            Value::List(Arc::new(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]))
        );
    }

    #[test]
    fn test_reverse() {
        assert_eq!(
            eval("[1, 2, 3].reverse()"),
            Value::List(Arc::new(vec![Value::Int(3), Value::Int(2), Value::Int(1)]))
        );
    }

    #[test]
    fn test_distinct() {
        assert_eq!(
            eval("[1, 2, 2, 3, 1].distinct()"),
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]))
        );
    }
}
