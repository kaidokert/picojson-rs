fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only download conformance tests if the remote-tests feature is enabled
    #[cfg(feature = "remote-tests")]
    {
        use std::fs;
        use std::io::{self, Read};
        use std::path::Path;
        let conformance_dir = Path::new("conformance_tests");

        // Skip download if conformance tests already exist
        if conformance_dir.exists() && conformance_dir.join("test_parsing").exists() {
            println!("cargo:warning=Conformance tests already exist, skipping download");
            return Ok(());
        }

        println!("cargo:warning=Downloading JSONTestSuite conformance tests...");

        let commit = "1ef36fa01286573e846ac449e8683f8833c5b26a";
        let url = format!("https://github.com/nst/JSONTestSuite/archive/{commit}.zip");

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
        let output_dir = Path::new("conformance_tests");
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
                    if let Some(stripped) =
                        path_str.strip_prefix(&format!("JSONTestSuite-{}/", commit))
                    {
                        output_dir.join(stripped)
                    } else {
                        output_dir.join(path)
                    }
                }
                None => continue,
            };

            if file.name().ends_with('/') {
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
            "cargo:warning=Extraction complete! Test files are in: {}",
            output_dir.display()
        );
        println!("cargo:warning=You can now run conformance tests with: cargo test --features remote-tests");
    }

    Ok(())
}
