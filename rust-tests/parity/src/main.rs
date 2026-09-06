#![deny(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

mod case;
mod compare;
mod filesystem;
mod http;
mod mcp;
mod process;
mod sqlite;

fn main() {
    println!("parity harness: use cargo test -p parity for the deterministic harness checks");
}
