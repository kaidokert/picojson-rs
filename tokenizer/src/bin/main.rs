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
    let mut f = File::open(path).expect("Unable to open file");

    match f.read_to_string(&mut s) {
        Err(_) => std::process::exit(1),
        Ok(_) => println!("{}", s),
    }

    let mut parser = ujson::Tokenizer::<u32, u8>::new();
    match parser.parse_full(s.as_bytes(), &mut |_, _| {}) {
        Err(_e) => std::process::exit(1),
        Ok(_) => std::process::exit(0),
    };
}
