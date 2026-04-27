#![cfg(feature = "defmt")]

use picojson::{ChunkReader, Event, JsonNumber, NumberResult, ParseError, PushParseError, String};

fn assert_defmt_format<T: defmt::Format>(_: &T) {}

#[test]
fn format_json_number() {
    let num = JsonNumber::from_slice(b"123.45").unwrap();
    assert_defmt_format(&num);
}

#[test]
fn format_number_result() {
    let number = NumberResult::Integer(42);
    assert_defmt_format(&number);
}

#[test]
fn format_json_string() {
    let s = String::Borrowed("Hello, world!");
    assert_defmt_format(&s);
}

#[test]
fn format_event() {
    let event = Event::Number(JsonNumber::from_slice(b"123").unwrap());
    assert_defmt_format(&event);
}

#[test]
fn format_parse_error() {
    let err = ParseError::InvalidNumber;
    assert_defmt_format(&err);
}

#[test]
fn format_push_parse_error() {
    let err = PushParseError::<u8>::Handler(7);
    assert_defmt_format(&err);
}

#[test]
fn format_chunk_reader() {
    let reader = ChunkReader::new(br#"{"k":1}"#, 4);
    assert_defmt_format(&reader);
}
