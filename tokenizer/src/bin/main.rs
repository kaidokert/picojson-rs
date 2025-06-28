// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::File;
use std::io::Read;
//use std::process;

fn main() {
    println!("Hello, world!");

    let args: Vec<_> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} file.json", args[0]);
        std::process::exit(1);
    }
    let path = &args[1];
    let mut s = String::new();
    let mut f = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Error: Unable to open file '{}': {}", path, e);
            std::process::exit(1);
        }
    };

    match f.read_to_string(&mut s) {
        Err(e) => {
            eprintln!("Error: Unable to read file '{}': {}", path, e);
            std::process::exit(1);
        }
        Ok(_) => println!("{}", s),
    }

    let mut parser = ujson::Tokenizer::<u32, u8>::new();
    match parser.parse_full(s.as_bytes(), &mut |_, _| {}) {
        Err(e) => {
            eprintln!("Error: JSON parsing failed: {:?}", e);
            std::process::exit(1);
        }
        Ok(_) => std::process::exit(0),
    };
}
