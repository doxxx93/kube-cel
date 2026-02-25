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

    // Comparison/arithmetic: shared between semver_funcs and quantity
    ctx.add_function("isGreaterThan", is_greater_than);
    ctx.add_function("isLessThan", is_less_than);
    ctx.add_function("compareTo", compare_to);

    #[cfg(feature = "quantity")]
    {
        ctx.add_function("add", add);
        ctx.add_function("sub", sub);
    }
}

// ---------------------------------------------------------------------------
// indexOf / lastIndexOf
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
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

#[allow(unused_variables)]
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

// ---------------------------------------------------------------------------
// isGreaterThan / isLessThan / compareTo
// ---------------------------------------------------------------------------

#[allow(unused_variables)]
fn is_greater_than(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    let arg = args
        .first()
        .cloned()
        .ok_or_else(|| ExecutionError::function_error("isGreaterThan", "missing argument"))?;

    match &this {
        #[cfg(feature = "semver_funcs")]
        Value::Opaque(o)
            if o.downcast_ref::<crate::semver_funcs::KubeSemver>()
                .is_some() =>
        {
            crate::semver_funcs::semver_is_greater_than(This(this), arg)
        }
        #[cfg(feature = "quantity")]
        Value::Opaque(o) if o.downcast_ref::<crate::quantity::KubeQuantity>().is_some() => {
            crate::quantity::cel_is_greater_than(This(this), arg)
        }
        _ => Err(ExecutionError::function_error(
            "isGreaterThan",
            format!("isGreaterThan not supported on type {:?}", this.type_of()),
        )),
    }
}

#[allow(unused_variables)]
fn is_less_than(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    let arg = args
        .first()
        .cloned()
        .ok_or_else(|| ExecutionError::function_error("isLessThan", "missing argument"))?;

    match &this {
        #[cfg(feature = "semver_funcs")]
        Value::Opaque(o)
            if o.downcast_ref::<crate::semver_funcs::KubeSemver>()
                .is_some() =>
        {
            crate::semver_funcs::semver_is_less_than(This(this), arg)
        }
        #[cfg(feature = "quantity")]
        Value::Opaque(o) if o.downcast_ref::<crate::quantity::KubeQuantity>().is_some() => {
            crate::quantity::cel_is_less_than(This(this), arg)
        }
        _ => Err(ExecutionError::function_error(
            "isLessThan",
            format!("isLessThan not supported on type {:?}", this.type_of()),
        )),
    }
}

#[allow(unused_variables)]
fn compare_to(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    let arg = args
        .first()
        .cloned()
        .ok_or_else(|| ExecutionError::function_error("compareTo", "missing argument"))?;

    match &this {
        #[cfg(feature = "semver_funcs")]
        Value::Opaque(o)
            if o.downcast_ref::<crate::semver_funcs::KubeSemver>()
                .is_some() =>
        {
            crate::semver_funcs::semver_compare_to(This(this), arg)
        }
        #[cfg(feature = "quantity")]
        Value::Opaque(o) if o.downcast_ref::<crate::quantity::KubeQuantity>().is_some() => {
            crate::quantity::cel_compare_to(This(this), arg)
        }
        _ => Err(ExecutionError::function_error(
            "compareTo",
            format!("compareTo not supported on type {:?}", this.type_of()),
        )),
    }
}

// ---------------------------------------------------------------------------
// add / sub (quantity only, but accepts Quantity or int)
// ---------------------------------------------------------------------------

#[cfg(feature = "quantity")]
fn add(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    crate::quantity::cel_add(This(this), Arguments(args))
}

#[cfg(feature = "quantity")]
fn sub(This(this): This<Value>, Arguments(args): Arguments) -> ResolveResult {
    crate::quantity::cel_sub(This(this), Arguments(args))
}
