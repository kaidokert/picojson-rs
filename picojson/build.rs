#[cfg(feature = "remote-tests")]
fn generate_conformance_tests() -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    let jsontest_suite_dir = Path::new("tests/data/JSONTestSuite/test_parsing");
    let json_checker_dir = Path::new("tests/data/json_checker");

    // Check if at least one test suite exists
    if !jsontest_suite_dir.exists() && !json_checker_dir.exists() {
        return Ok(()); // Skip if no tests exist yet
    }

    let mut should_pass_tests = String::new();
    let mut should_fail_tests = String::new();
    let mut impl_dependent_tests = String::new();
    let mut test_name_counts: HashMap<String, u32> = HashMap::new();

    // Process JSONTestSuite tests
    if jsontest_suite_dir.exists() {
        for entry in fs::read_dir(jsontest_suite_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().unwrap_or_default() == "json" {
                let filename = path.file_name().unwrap().to_string_lossy();
                let test_name = sanitize_test_name(&filename, &mut test_name_counts);

                // Skip files that contain invalid UTF-8 (include_str! can't handle them)
                if fs::read_to_string(&path).is_err() {
                    continue;
                }

                if filename.starts_with("y_") {
                    should_pass_tests.push_str(&format!(
                        r#"    #[test]
    fn test_should_pass_jsontest_{test_name}() {{
        let content = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/JSONTestSuite/test_parsing/{filename}"));
        let result = run_parser_test(content);
        assert!(result.is_ok(), "JSONTestSuite test {filename} should pass but failed: {{:?}}", result.err());
    }}
"#,
                        test_name = test_name,
                        filename = filename
                    ));
                } else if filename.starts_with("n_") {
                    should_fail_tests.push_str(&format!(
                        r#"    #[test]
    fn test_should_fail_jsontest_{test_name}() {{
        let content = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/JSONTestSuite/test_parsing/{filename}"));
        let result = run_parser_test(content);
        assert!(result.is_err(), "JSONTestSuite test {filename} should fail but passed");
    }}
"#,
                        test_name = test_name,
                        filename = filename
                    ));
                } else if filename.starts_with("i_") {
                    impl_dependent_tests.push_str(&format!(
                        r#"    #[test]
    fn test_impl_dependent_jsontest_{test_name}() {{
        let content = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/JSONTestSuite/test_parsing/{filename}"));
        let result = run_parser_test(content);
        // Implementation dependent - just run it, don't assert result
        println!("JSONTestSuite test {filename}: {{:?}}", result);
    }}
"#,
                        test_name = test_name,
                        filename = filename
                    ));
                }
            }
        }
    }

    // Note: JSON_checker tests are handled by the dedicated tests/json_checker_tests.rs
    // which provides proper categorization and handles known deviations correctly

    let generated_code = format!(
        r#"// Generated by build.rs - DO NOT EDIT
#![allow(non_snake_case)] // Keep test names discoverable and matching original JSON test suite

#[cfg(feature = "remote-tests")]
mod conformance_generated {{
    use picojson::{{Event, ParseError, PullParser, SliceParser}};

    fn run_parser_test(json_content: &str) -> Result<usize, ParseError> {{
        let mut buffer = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(json_content, &mut buffer);
        let mut event_count = 0;

        loop {{
            match parser.next_event() {{
                Ok(Event::EndDocument) => break,
                Ok(_event) => event_count += 1,
                Err(e) => return Err(e),
            }}
        }}
        Ok(event_count)
    }}

    #[cfg(feature = "remote-tests")]
    mod should_pass {{
        use super::run_parser_test;
{should_pass_tests}
    }}

    #[cfg(feature = "remote-tests")]
    mod should_fail {{
        use super::run_parser_test;
{should_fail_tests}
    }}

    #[cfg(feature = "remote-tests")]
    mod impl_dependent {{
        use super::run_parser_test;
{impl_dependent_tests}
    }}
}}
"#,
        should_pass_tests = should_pass_tests,
        should_fail_tests = should_fail_tests,
        impl_dependent_tests = impl_dependent_tests
    );

    fs::write("tests/conformance_generated.rs", generated_code)?;
    println!("cargo:warning=Generated conformance tests");

    Ok(())
}

#[cfg(feature = "remote-tests")]
fn sanitize_test_name(
    filename: &str,
    test_name_counts: &mut std::collections::HashMap<String, u32>,
) -> String {
    let mut result = filename
        .strip_suffix(".json")
        .unwrap_or(filename)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Collapse multiple consecutive underscores to avoid very long names
    while result.contains("__") {
        result = result.replace("__", "_");
    }

    // Handle duplicates by adding a counter suffix
    let count = test_name_counts.entry(result.clone()).or_insert(0);
    *count += 1;

    if *count > 1 {
        format!("{result}_{count}")
    } else {
        result
    }
}

// Configuration from Cargo.toml metadata
#[cfg(feature = "remote-tests")]
fn get_jsontest_suite_url() -> String {
    std::env::var("CARGO_PKG_METADATA_CONFORMANCE_TESTS_JSONTEST_SUITE_URL")
        .unwrap_or_else(|_| "https://github.com/nst/JSONTestSuite/archive/{commit}.zip".to_string())
}

#[cfg(feature = "remote-tests")]
fn get_jsontest_suite_commit() -> String {
    std::env::var("CARGO_PKG_METADATA_CONFORMANCE_TESTS_JSONTEST_SUITE_COMMIT")
        .unwrap_or_else(|_| "1ef36fa01286573e846ac449e8683f8833c5b26a".to_string())
}

#[cfg(feature = "remote-tests")]
fn get_json_checker_url() -> String {
    std::env::var("CARGO_PKG_METADATA_CONFORMANCE_TESTS_JSON_CHECKER_URL")
        .unwrap_or_else(|_| "https://www.json.org/JSON_checker/test.zip".to_string())
}

#[cfg(feature = "remote-tests")]
fn download_json_test_suite() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::io::{self, Read};
    use std::path::Path;

    let output_dir = Path::new("tests/data/JSONTestSuite");

    // Skip download if tests already exist
    if output_dir.exists() && output_dir.join("test_parsing").exists() {
        println!("cargo:warning=JSONTestSuite already exists, skipping download");
        return Ok(());
    }

    println!("cargo:warning=Downloading JSONTestSuite conformance tests...");

    let commit = get_jsontest_suite_commit();
    let url_template = get_jsontest_suite_url();
    let url = url_template.replace("{commit}", &commit);

    println!("cargo:warning=Downloading from: {}", url);

    // Download the ZIP file
    let response = ureq::get(&url).call()?;
    let mut zip_bytes = Vec::new();
    response.into_reader().read_to_end(&mut zip_bytes)?;

    println!("cargo:warning=Downloaded {} bytes", zip_bytes.len());

    // Extract ZIP file
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)?;

    // Create output directory
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
    }

    println!("cargo:warning=Extracting {} files...", archive.len());

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => {
                // Strip the git hash directory prefix (JSONTestSuite-{hash}/)
                let path_str = path.to_string_lossy();
                if let Some(stripped) = path_str.strip_prefix(&format!("JSONTestSuite-{}/", commit))
                {
                    output_dir.join(stripped)
                } else {
                    output_dir.join(path)
                }
            }
            None => continue,
        };

        if file.is_dir() {
            // Directory
            fs::create_dir_all(&outpath)?;
        } else {
            // File
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }

        if i % 500 == 0 {
            println!("cargo:warning=Extracted {} files...", i);
        }
    }

    println!(
        "cargo:warning=JSONTestSuite extraction complete! Test files are in: {}",
        output_dir.display()
    );

    Ok(())
}

#[cfg(feature = "remote-tests")]
fn download_json_checker() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::io::{self, Read};
    use std::path::Path;

    let output_dir = Path::new("tests/data/json_checker");

    // Skip download if tests already exist
    if output_dir.exists() && output_dir.read_dir()?.next().is_some() {
        println!("cargo:warning=JSON_checker already exists, skipping download");
        return Ok(());
    }

    println!("cargo:warning=Downloading JSON_checker tests...");

    let url = get_json_checker_url();
    println!("cargo:warning=Downloading from: {}", url);

    // Download the ZIP file
    let response = ureq::get(&url).call()?;
    let mut zip_bytes = Vec::new();
    response.into_reader().read_to_end(&mut zip_bytes)?;

    println!("cargo:warning=Downloaded {} bytes", zip_bytes.len());

    // Extract ZIP file
    let reader = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(reader)?;

    // Create output directory
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
    }

    println!("cargo:warning=Extracting {} files...", archive.len());

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => {
                // Extract directly to json_checker directory
                let filename = path.file_name().unwrap_or_default();
                output_dir.join(filename)
            }
            None => continue,
        };

        if file.is_dir() {
            // Skip directories
            continue;
        } else {
            // File - only extract .json files
            if let Some(ext) = outpath.extension() {
                if ext == "json" {
                    let mut outfile = fs::File::create(&outpath)?;
                    io::copy(&mut file, &mut outfile)?;
                    println!("cargo:warning=Extracted: {}", outpath.display());
                }
            }
        }
    }

    println!(
        "cargo:warning=JSON_checker extraction complete! Test files are in: {}",
        output_dir.display()
    );

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Download test suites based on features
    #[cfg(feature = "remote-tests")]
    {
        download_json_test_suite()?;
    }

    #[cfg(feature = "remote-tests")]
    {
        download_json_checker()?;
    }

    // Generate conformance tests if any remote test features are enabled
    #[cfg(feature = "remote-tests")]
    {
        generate_conformance_tests()?;
        println!("cargo:warning=You can now run conformance tests with: cargo test --features remote-tests");
    }

    Ok(())
}
