use picojson::{Event, PullParser, SliceParser};

fn main() {
    // Test the original failing case: ["\uD801\udc37"]
    let json = r#"["\uD801\udc37"]"#;
    let mut buffer = [0u8; 1024];
    let mut parser = SliceParser::with_buffer(json, &mut buffer);

    let mut string_found = false;
    let mut string_bytes = Vec::new();

    loop {
        match parser.next_event() {
            Ok(Event::EndDocument) => break,
            Ok(Event::String(s)) => {
                string_found = true;
                string_bytes = s.as_bytes().to_vec();
                println!("Found string: {:?}", s);
                println!("String content as bytes: {:?}", string_bytes);
            }
            Ok(event) => {
                println!("Event: {:?}", event);
            }
            Err(e) => panic!("Parser error: {:?}", e),
        }
    }

    println!("Successfully parsed surrogate pair JSON");

    // The string should be decoded to U+10437 which is encoded as UTF-8: [0xF0, 0x90, 0x90, 0xB7]
    if string_found {
        assert_eq!(
            string_bytes,
            vec![0xF0, 0x90, 0x90, 0xB7],
            "Surrogate pair should decode to U+10437"
        );
        println!("✅ Surrogate pair correctly decoded!");
    } else {
        panic!("Expected String event not found");
    }

    // Test additional surrogate pairs
    let test_cases = vec![
        (r#"["\uD834\uDD1E"]"#, "Musical G clef symbol"),
        (r#"["\uD83D\uDE00"]"#, "Grinning face emoji"),
    ];

    for (json, description) in test_cases {
        println!("\nTesting {}: {}", description, json);
        let mut buffer = [0u8; 1024];
        let mut parser = SliceParser::with_buffer(json, &mut buffer);

        let mut success = false;
        loop {
            match parser.next_event() {
                Ok(Event::EndDocument) => break,
                Ok(Event::String(_)) => success = true,
                Ok(_) => {}
                Err(e) => panic!("Parser error for {}: {:?}", description, e),
            }
        }

        if success {
            println!("✅ Successfully parsed {}", description);
        } else {
            panic!("Failed to find string in {}", description);
        }
    }
}
