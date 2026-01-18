// Example demonstrating PushParser with SAX-style event handling

use picojson::{DefaultConfig, Event, ParseError, PushParseError, PushParser, PushParserHandler};

/// A simple event handler that prints JSON events as they arrive
struct JsonEventPrinter {
    indent: usize,
    event_count: usize,
}

impl JsonEventPrinter {
    fn new() -> Self {
        Self {
            indent: 0,
            event_count: 0,
        }
    }

    fn indent_str(&self) -> String {
        "  ".repeat(self.indent)
    }
}

impl<'input, 'scratch> PushParserHandler<'input, 'scratch, ParseError> for JsonEventPrinter {
    fn handle_event(&mut self, event: Event<'input, 'scratch>) -> Result<(), ParseError> {
        self.event_count += 1;

        match event {
            Event::StartObject => {
                println!("{}🏁 StartObject", self.indent_str());
                self.indent += 1;
            }
            Event::EndObject => {
                self.indent = self.indent.saturating_sub(1);
                println!("{}🏁 EndObject", self.indent_str());
            }
            Event::StartArray => {
                println!("{}📋 StartArray", self.indent_str());
                self.indent += 1;
            }
            Event::EndArray => {
                self.indent = self.indent.saturating_sub(1);
                println!("{}📋 EndArray", self.indent_str());
            }
            Event::Key(key) => {
                println!("{}🔑 Key: '{}'", self.indent_str(), key.as_str());
            }
            Event::String(s) => {
                println!("{}📝 String: '{}'", self.indent_str(), s.as_str());
            }
            Event::Number(num) => {
                println!("{}🔢 Number: {}", self.indent_str(), num);
            }
            Event::Bool(b) => {
                println!("{}✅ Bool: {}", self.indent_str(), b);
            }
            Event::Null => {
                println!("{}⭕ Null", self.indent_str());
            }
            Event::EndDocument => {
                println!("{}🏁 EndDocument", self.indent_str());
            }
            // ContentSpan events should not reach user code - they get converted by PushParser
            Event::ContentSpan { .. }
            | Event::PartialContentSpanStart { .. }
            | Event::PartialContentSpanEnd { .. } => {
                panic!("Internal ContentSpan events should not reach user handlers")
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), PushParseError<ParseError>> {
    println!("🚀 PushParser Demo - SAX-style JSON Processing");
    println!("===============================================");
    println!();

    // Example JSON with various features to demonstrate push parsing
    let json_chunks = vec![
        br#"{"name": "Pic"#.as_slice(),
        br#"oJSON", "version": 1.0, "#.as_slice(),
        br#""features": ["fast", "no_std""#.as_slice(),
        br#", "zero\u0041lloc"], "escapes": "hello\nworld", "#.as_slice(),
        br#""nested": {"data": [1, 2.5, true, null]}}"#.as_slice(),
    ];

    let full_json = json_chunks.concat();
    let json_str = std::str::from_utf8(&full_json)?;

    println!("📄 Input JSON: {}", json_str);
    println!("📏 Total size: {} bytes", full_json.len());
    println!(
        "📦 Processing in {} chunks (simulates streaming)",
        json_chunks.len()
    );
    println!();

    // Create handler and parser
    let handler = JsonEventPrinter::new();
    let mut buffer = [0u8; 512]; // Scratch buffer for escape processing
    let buffer_size = buffer.len();
    let mut parser = PushParser::<_, DefaultConfig>::new(handler, &mut buffer);

    println!("🔄 Starting PushParser with incremental data feeding:");
    println!("   Buffer size: {} bytes", buffer_size);
    println!();

    // Feed data chunk by chunk to demonstrate streaming capability
    for (i, chunk) in json_chunks.iter().enumerate() {
        println!("📨 Processing chunk {} ({} bytes):", i + 1, chunk.len());
        let chunk_str = std::str::from_utf8(chunk)?;
        println!("   Chunk data: {:?}", chunk_str);

        // Write chunk to parser - events are handled immediately
        parser.write(chunk)?;
        println!();
    }

    // Signal end of input and retrieve the handler
    println!("🔚 Finishing parsing...");
    let handler = parser.finish()?;

    println!();
    println!(
        "✅ Successfully processed {} events with PushParser!",
        handler.event_count
    );

    Ok(())
}
