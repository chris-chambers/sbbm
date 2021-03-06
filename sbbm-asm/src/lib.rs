// Copyright 2015, Christopher Chambers
// Distributed under the GNU GPL v3. See COPYING for details.

// WHEN(rust-1.0): Remove feature(core), it is for the currently-unstable FnBox
#![feature(core, plugin)]
#![plugin(regex_macros)]

extern crate regex;

pub mod assembler;
pub mod ast;
pub mod commands;
pub mod fab;
pub mod hw;
pub mod types;
pub mod layout;
pub mod lexer;
pub mod nbt;
pub mod parser;
