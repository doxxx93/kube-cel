//! Runtime type dispatch for CEL functions with name collisions.
//!
//! cel-interpreter registers functions by name only (no typed overloads).
//! When the same function name applies to multiple types (e.g., `indexOf` for
//! both strings and lists), this module provides unified dispatch functions
//! that route to the correct implementation based on the runtime type of `this`.

use cel::extractors::{Arguments, This};
use cel::objects::Value;
use cel::{Context, ExecutionError, ResolveResult};

/// Register all dispatch functions. Must be called after individual module registrations
/// to overwrite any conflicting single-type registrations.
pub fn register(ctx: &mut Context<'_>) {
    ctx.add_function("indexOf", index_of);
    ctx.add_function("lastIndexOf", last_index_of);
}

/// `indexOf` — dispatches to string or list implementation.
fn index_of(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    match this {
        #[cfg(feature = "strings")]
        Value::String(s) => crate::strings::string_index_of(This(s), Arguments(args)),

        #[cfg(feature = "lists")]
        Value::List(list) => crate::lists::list_index_of(&list, &args),

        _ => Err(ExecutionError::function_error(
            "indexOf",
            format!("indexOf not supported on type {:?}", this.type_of()),
        )),
    }
}

/// `lastIndexOf` — dispatches to string or list implementation.
fn last_index_of(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    match this {
        #[cfg(feature = "strings")]
        Value::String(s) => crate::strings::string_last_index_of(This(s), Arguments(args)),

        #[cfg(feature = "lists")]
        Value::List(list) => crate::lists::list_last_index_of(&list, &args),

        _ => Err(ExecutionError::function_error(
            "lastIndexOf",
            format!("lastIndexOf not supported on type {:?}", this.type_of()),
        )),
    }
}
