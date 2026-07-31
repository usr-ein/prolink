// SPDX-License-Identifier: GPL-3.0-only

//! Generates the C++ side of the bridge.
//!
//! `cxx-build` compiles the glue and emits `lib.rs.h` beside it, which is what
//! a host includes. Because both sides come from `src/lib.rs`, a field added
//! to a shared struct is a compile error in C++ as well as in Rust — the drift
//! a hand-written header allows cannot happen.

fn main() {
    cxx_build::bridge("src/lib.rs")
        .std("c++17")
        .compile("prolink-cxx");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
