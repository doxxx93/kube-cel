//! Basic usage of Kubernetes CEL extension functions.
//!
//! Run with: `cargo run --example basic`

use cel::{Context, Program};
use kube_cel::register_all;

fn main() {
    let mut ctx = Context::default();
    register_all(&mut ctx);

    // String functions
    let result = Program::compile("'hello world'.upperAscii()")
        .unwrap()
        .execute(&ctx)
        .unwrap();
    println!("upperAscii: {result:?}");

    // List functions
    let result = Program::compile("[3, 1, 2].isSorted()")
        .unwrap()
        .execute(&ctx)
        .unwrap();
    println!("isSorted: {result:?}");

    // Quantity comparison
    let result = Program::compile("quantity('1Gi').isGreaterThan(quantity('500Mi'))")
        .unwrap()
        .execute(&ctx)
        .unwrap();
    println!("1Gi > 500Mi: {result:?}");

    // Semver
    let result = Program::compile("semver('2.0.0').isGreaterThan(semver('1.9.9'))")
        .unwrap()
        .execute(&ctx)
        .unwrap();
    println!("2.0.0 > 1.9.9: {result:?}");

    // String formatting
    let result = Program::compile("'hello %s, you have %d items'.format(['world', 5])")
        .unwrap()
        .execute(&ctx)
        .unwrap();
    println!("format: {result:?}");
}
