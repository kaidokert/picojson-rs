use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;

// This function replaces the Python script.
fn generate_nested_json(depth: usize) -> String {
    let mut nested_part = String::from(r#"{"surprise":"whoo"}"#);
    for _ in 0..depth {
        nested_part = format!("[{}]", nested_part);
    }

    format!(
        r#"{{"id":999,"test_depth":{},"deep_array":{},"status":"deep"}}"#,
        depth, nested_part
    )
}

// Define all possible depth configurations in a constant array.
const DEPTHS: &[usize] = &[
    7, 9, 31, 33, 63, 65, 127, 129, 255, 257, 511, 513, 1023, 1025,
];

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("test.json");

    // Iterate through the configurations to find the active feature.
    let mut depth = 0; // Default depth
    for value in DEPTHS {
        // Construct the feature name on the fly.
        let feature_name = format!("CARGO_FEATURE_DEPTH_{}", value);
        if env::var(feature_name).is_ok() {
            depth = *value;
            break;
        }
    }

    let mut json_string = if depth > 0 {
        generate_nested_json(depth)
    } else {
        // A simple, non-nested JSON for the default case.
        String::from(r#"{"id":0,"status":"default"}"#)
    };

    // Pad the string with spaces to a fixed length to normalize input size.
    const PADDED_LENGTH: usize = 2200;
    while json_string.len() < PADDED_LENGTH {
        json_string.push(' ');
    }

    let mut f = File::create(&dest_path).unwrap();
    f.write_all(json_string.as_bytes()).unwrap();

    // This tells Cargo to re-run the build script if build.rs changes.
    println!("cargo:rerun-if-changed=build.rs");
}
