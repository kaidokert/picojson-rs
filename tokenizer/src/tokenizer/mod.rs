// SPDX-License-Identifier: Apache-2.0

use crate::bitstack::BitStack;
use crate::BitStackCore;

use log::{debug, info};

#[derive(Debug, Clone)]
struct ParseContext<T: BitStack, D> {
    /// Keeps track of the depth of the object/array
    depth: D,
    /// Keeps track of the stack of objects/arrays
    stack: T,
    /// Keeps track of the last comma and its position
    after_comma: Option<(u8, usize)>,
}

impl<T: BitStack, D: BitStackCore> ParseContext<T, D> {
    // We can expect an unsigned with From<u8> requirement
    // So this math usually works
    fn max_depth() -> D {
        D::from(0u8).not()
    }
    fn new() -> Self {
        ParseContext {
            depth: 0u8.into(),
            stack: T::default(),
            after_comma: None,
        }
    }
    fn enter_object(&mut self, data: u8, pos: usize) -> Result<(), Error> {
        if self.depth == Self::max_depth() {
            return Error::new(ErrKind::MaxDepthReached, data, pos);
        }
        self.stack.push(true);
        self.depth += 1u8.into();
        Ok(())
    }
    fn exit_object(&mut self, pos: usize) -> Result<(), Error> {
        if self.depth == 0u8.into() {
            return Error::new(ErrKind::UnopenedObject, b'}', pos);
        }
        self.stack.pop();
        self.depth -= 1u8.into();
        Ok(())
    }
    fn enter_array(&mut self, data: u8, pos: usize) -> Result<(), Error> {
        if self.depth == Self::max_depth() {
            return Error::new(ErrKind::MaxDepthReached, data, pos);
        }
        self.stack.push(false);
        self.depth += 1u8.into();
        Ok(())
    }
    fn exit_array(&mut self, pos: usize) -> Result<(), Error> {
        if self.depth == 0u8.into() {
            return Error::new(ErrKind::UnopenedArray, b']', pos);
        }
        self.stack.pop();
        self.depth -= 1u8.into();
        Ok(())
    }
    fn is_object(&self) -> bool {
        if self.depth == 0u8.into() {
            return false;
        }
        self.stack.top()
    }
    fn is_array(&self) -> bool {
        if self.depth == 0u8.into() {
            return false;
        }
        !self.stack.top()
    }
}

#[derive(Debug, Clone)]
enum State {
    Idle,
    String { state: String, key: bool },
    Number { state: Num },
    Token { token: Token },
    Object { expect: Object },
    Array { expect: Array },
    Finished,
}

#[derive(Debug, Clone)]
enum String {
    Normal,
    Escaping,
    Unicode0, // Just tracks number of hex digits seen (0-3)
    Unicode1,
    Unicode2,
    Unicode3,
}

#[derive(Debug, Clone)]
enum Num {
    Sign,
    LeadingZero,
    BeforeDecimalPoint,
    Decimal,
    AfterDecimalPoint,
    Exponent,
    ExponentSign,
    AfterExponent,
}

#[derive(Debug, Clone)]
enum True {
    R,
    U,
    E,
}
#[derive(Debug, Clone)]
enum False {
    A,
    L,
    S,
    E,
}
#[derive(Debug, Clone)]
enum Null {
    U,
    L1,
    L2,
}

#[derive(Debug, Clone)]
enum Token {
    True(True),
    False(False),
    Null(Null),
}

#[derive(Debug, Clone, PartialEq)]
enum Object {
    Key,
    Colon,
    Value,
    CommaOrEnd,
}

#[derive(Debug, Clone, PartialEq)]
enum Array {
    ItemOrEnd,
    CommaOrEnd,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventToken {
    True,
    False,
    Null,
    String,
    Key,
    Number,
    NumberAndArray,  // used for closing arrays after numbers
    NumberAndObject, // used for closing objects after numbers
    UnicodeEscape,
    EscapeSequence, // emitted when \ is encountered (start of any escape)
    // Simple escape sequences
    EscapeQuote,          // \"
    EscapeBackslash,      // \\
    EscapeSlash,          // \/
    EscapeBackspace,      // \b
    EscapeFormFeed,       // \f
    EscapeNewline,        // \n
    EscapeCarriageReturn, // \r
    EscapeTab,            // \t
}

// todo: expose number events: sign, decimal, fraction, exponent
// update when a part of number has finished tokenizing ?

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Begin(EventToken),
    End(EventToken),
    ObjectStart,
    ObjectEnd,
    ArrayStart,
    ArrayEnd,
    #[cfg(test)]
    Uninitialized,
}

pub struct Tokenizer<T: BitStack = u32, D = u8> {
    state: State,
    total_consumed: usize,
    context: ParseContext<T, D>,
}

#[derive(PartialEq)]
pub struct Error {
    kind: ErrKind,
    character: u8,
    position: usize,
}

#[derive(PartialEq, Debug)]
pub enum ErrKind {
    EmptyStream,
    UnfinishedStream,
    InvalidRoot,
    InvalidToken,
    UnescapedControlCharacter,
    TrailingComma,
    ContentEnded,
    UnopenedArray,
    UnopenedObject,
    MaxDepthReached,
    InvalidNumber,
    InvalidUnicodeEscape,
    InvalidStringEscape,
    ExpectedObjectKey,
    ExpectedObjectValue,
    ExpectedColon,
    ExpectedArrayItem,
}

impl Error {
    pub fn new<T>(kind: ErrKind, character: u8, position: usize) -> Result<T, Self> {
        Err(Self {
            kind,
            character,
            position,
        })
    }
}

impl core::fmt::Debug for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:?}({}) at {}",
            self.kind, self.character as char, self.position
        )
    }
}

impl Default for Tokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: BitStack + core::fmt::Debug, D: BitStackCore> Tokenizer<T, D> {
    pub fn new() -> Self {
        Tokenizer {
            state: State::Idle,
            total_consumed: 0,
            context: ParseContext::new(),
        }
    }

    fn check_trailing_comma(&mut self, data: u8) -> Result<(), Error> {
        // Check for trailing comma if we're at a closing bracket/brace
        if (data == b']' || data == b'}') && self.context.after_comma.is_some() {
            let (c, pos) = self.context.after_comma.unwrap();
            return Error::new(ErrKind::TrailingComma, c, pos);
        }

        // Only reset after_comma for non-whitespace characters
        if !matches!(data, b' ' | b'\t' | b'\n' | b'\r') {
            self.context.after_comma = None;
        }
        Ok(())
    }

    pub fn parse_full(
        &mut self,
        data: &[u8],
        callback: &mut dyn FnMut(Event, usize),
    ) -> Result<usize, Error> {
        self.parse_chunk(data, callback)?;
        self.finish(callback)
    }

    pub fn finish<F>(&mut self, callback: &mut F) -> Result<usize, Error>
    where
        F: FnMut(Event, usize) + ?Sized,
    {
        // we check that parser was idle, at zero nesting depth
        if self.context.depth != 0u8.into() {
            return Error::new(ErrKind::UnfinishedStream, b' ', self.total_consumed);
        }
        if self.total_consumed == 0 {
            return Error::new(ErrKind::EmptyStream, b' ', self.total_consumed);
        }

        debug!("--finished-- {}", self.total_consumed);
        match &self.state {
            State::Finished => Ok(self.total_consumed),
            State::Number {
                state: Num::LeadingZero,
            }
            | State::Number {
                state: Num::BeforeDecimalPoint,
            }
            | State::Number {
                state: Num::AfterDecimalPoint,
            }
            | State::Number {
                state: Num::AfterExponent,
            } => {
                callback(Event::End(EventToken::Number), self.total_consumed);
                Ok(self.total_consumed)
            }
            _ => Error::new(ErrKind::UnfinishedStream, b' ', self.total_consumed),
        }
    }

    pub fn parse_chunk<F>(&mut self, data: &[u8], callback: &mut F) -> Result<usize, Error>
    where
        F: FnMut(Event, usize) + ?Sized,
    {
        self.p(data, callback)?;
        Ok(self.total_consumed)
    }

    // testing helper
    #[cfg(test)]
    fn t(&mut self, data: &[u8]) -> Result<usize, Error> {
        self.p(data, &mut |_, _| {})
    }
    // testing helper
    fn p<F>(&mut self, data: &[u8], callback: &mut F) -> Result<usize, Error>
    where
        F: FnMut(Event, usize) + ?Sized,
    {
        let consumed = self.parse_chunk_inner(data, callback)?;
        self.total_consumed += consumed;
        Ok(consumed)
    }

    fn maybe_exit_level(&self) -> State {
        if self.context.is_object() {
            State::Object {
                expect: Object::CommaOrEnd,
            }
        } else if self.context.is_array() {
            State::Array {
                expect: Array::CommaOrEnd,
            }
        } else if self.context.depth == 0u8.into() {
            State::Finished
        } else {
            State::Idle
        }
    }

    fn saw_a_comma_now_what(&mut self) -> State {
        if self.context.is_object() {
            State::Object {
                expect: Object::Key,
            }
        } else if self.context.is_array() {
            State::Array {
                expect: Array::ItemOrEnd,
            }
        } else {
            State::Idle
        }
    }

    fn start_token(
        &mut self,
        token: u8,
        pos: usize,
        callback: &mut dyn FnMut(Event, usize),
    ) -> Result<State, Error> {
        match token {
            b't' => {
                callback(Event::Begin(EventToken::True), pos);
                Ok(State::Token {
                    token: Token::True(True::R),
                })
            }
            b'f' => {
                callback(Event::Begin(EventToken::False), pos);
                Ok(State::Token {
                    token: Token::False(False::A),
                })
            }
            b'n' => {
                callback(Event::Begin(EventToken::Null), pos);
                Ok(State::Token {
                    token: Token::Null(Null::U),
                })
            }
            _ => Error::new(ErrKind::InvalidToken, token, pos),
        }
    }

    fn parse_chunk_inner<F>(&mut self, data: &[u8], mut callback: &mut F) -> Result<usize, Error>
    where
        F: FnMut(Event, usize) + ?Sized,
    {
        let mut pos = 0;
        while pos < data.len() {
            info!(
                "Pos: {}, Byte: {:?}, State: {:?}, Context: {:?}",
                pos, data[pos] as char, self.state, self.context
            );

            // Special case - this needs to be done for every Array match arm
            if let State::Array {
                expect: Array::ItemOrEnd,
            } = &self.state
            {
                self.check_trailing_comma(data[pos])?;
            }

            self.state = match (&self.state, data[pos]) {
                (State::Number { state: Num::Sign }, b'0') => State::Number {
                    state: Num::LeadingZero,
                },
                (State::Number { state: Num::Sign }, b'1'..=b'9') => State::Number {
                    state: Num::BeforeDecimalPoint,
                },
                (State::Number { state: Num::Sign }, _) => {
                    return Error::new(ErrKind::InvalidNumber, data[pos], pos);
                }
                (
                    State::Number {
                        state: Num::LeadingZero,
                    },
                    b'e' | b'E',
                ) => State::Number {
                    state: Num::Exponent,
                },
                (
                    State::Number {
                        state: Num::LeadingZero,
                    },
                    b'.',
                ) => State::Number {
                    state: Num::Decimal,
                },
                (
                    State::Number {
                        state: Num::BeforeDecimalPoint,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::BeforeDecimalPoint,
                },
                (
                    State::Number {
                        state: Num::BeforeDecimalPoint,
                    },
                    b'.',
                ) => State::Number {
                    state: Num::Decimal,
                },
                (
                    State::Number {
                        state: Num::BeforeDecimalPoint,
                    },
                    b'e' | b'E',
                ) => State::Number {
                    state: Num::Exponent,
                },
                (
                    State::Number {
                        state: Num::Decimal,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::AfterDecimalPoint,
                },
                (
                    State::Number {
                        state: Num::Decimal,
                    },
                    _,
                ) => {
                    return Error::new(ErrKind::InvalidNumber, data[pos], pos);
                }
                (
                    State::Number {
                        state: Num::AfterDecimalPoint,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::AfterDecimalPoint,
                },
                (
                    State::Number {
                        state: Num::AfterDecimalPoint,
                    },
                    b'e' | b'E',
                ) => State::Number {
                    state: Num::Exponent,
                },
                (
                    State::Number {
                        state: Num::Exponent,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::AfterExponent,
                },
                (
                    State::Number {
                        state: Num::Exponent,
                    },
                    b'+' | b'-',
                ) => State::Number {
                    state: Num::ExponentSign,
                },
                (
                    State::Number {
                        state: Num::Exponent,
                    },
                    _,
                ) => {
                    return Error::new(ErrKind::InvalidNumber, data[pos], pos);
                }
                (
                    State::Number {
                        state: Num::ExponentSign,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::AfterExponent,
                },
                (
                    State::Number {
                        state: Num::ExponentSign,
                    },
                    _,
                ) => {
                    return Error::new(ErrKind::InvalidNumber, data[pos], pos);
                }
                (
                    State::Number {
                        state: Num::AfterExponent,
                    },
                    b'0'..=b'9',
                ) => State::Number {
                    state: Num::AfterExponent,
                },
                (State::Number { state: _ }, b',') => {
                    callback(Event::End(EventToken::Number), pos);
                    self.context.after_comma = Some((data[pos], pos));
                    self.saw_a_comma_now_what()
                }
                (State::Number { state: _ }, b' ' | b'\t' | b'\n' | b'\r') => {
                    callback(Event::End(EventToken::Number), pos);
                    self.maybe_exit_level()
                }
                (State::Number { state: _ }, b']') => {
                    callback(Event::End(EventToken::NumberAndArray), pos);
                    callback(Event::ArrayEnd, pos);
                    self.context.exit_array(pos)?;
                    self.maybe_exit_level()
                }
                (State::Number { state: _ }, b'}') => {
                    callback(Event::End(EventToken::NumberAndObject), pos);
                    callback(Event::ObjectEnd, pos);
                    self.context.exit_object(pos)?;
                    self.maybe_exit_level()
                }
                (State::Number { state: _ }, _) => {
                    return Error::new(ErrKind::InvalidNumber, data[pos], pos);
                }
                (
                    State::String {
                        state: String::Normal,
                        key,
                    },
                    b'"',
                ) => {
                    if *key {
                        callback(Event::End(EventToken::Key), pos);
                        State::Object {
                            expect: Object::Colon,
                        }
                    } else {
                        callback(Event::End(EventToken::String), pos);
                        self.maybe_exit_level()
                    }
                }
                (
                    State::String {
                        state: String::Normal,
                        key,
                    },
                    b'\\',
                ) => {
                    callback(Event::Begin(EventToken::EscapeSequence), pos);
                    State::String {
                        state: String::Escaping,
                        key: *key,
                    }
                }
                (
                    State::String {
                        state: String::Normal,
                        key: _,
                    },
                    b'\x00'..=b'\x1F',
                ) => {
                    return Error::new(ErrKind::UnescapedControlCharacter, data[pos], pos);
                }
                (
                    State::String {
                        state: String::Normal,
                        key: _,
                    },
                    _,
                ) => self.state.clone(),
                // Handle simple escape sequences with lookup table
                (
                    State::String {
                        state: String::Escaping,
                        key,
                    },
                    escape_char @ (b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't'),
                ) => {
                    let escape_token = match escape_char {
                        b'"' => EventToken::EscapeQuote,
                        b'\\' => EventToken::EscapeBackslash,
                        b'/' => EventToken::EscapeSlash,
                        b'b' => EventToken::EscapeBackspace,
                        b'f' => EventToken::EscapeFormFeed,
                        b'n' => EventToken::EscapeNewline,
                        b'r' => EventToken::EscapeCarriageReturn,
                        b't' => EventToken::EscapeTab,
                        _ => unreachable!(),
                    };
                    callback(Event::Begin(escape_token.clone()), pos);
                    callback(Event::End(escape_token), pos);
                    State::String {
                        state: String::Normal,
                        key: *key,
                    }
                }
                (
                    State::String {
                        state: String::Escaping,
                        key,
                    },
                    b'u',
                ) => State::String {
                    state: String::Unicode0,
                    key: *key,
                },
                (
                    State::String {
                        state: String::Unicode0,
                        key,
                    },
                    b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F',
                ) => {
                    callback(Event::Begin(EventToken::UnicodeEscape), pos);
                    State::String {
                        state: String::Unicode1,
                        key: *key,
                    }
                }
                (
                    State::String {
                        state: String::Unicode1,
                        key,
                    },
                    b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F',
                ) => State::String {
                    state: String::Unicode2,
                    key: *key,
                },
                (
                    State::String {
                        state: String::Unicode2,
                        key,
                    },
                    b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F',
                ) => State::String {
                    state: String::Unicode3,
                    key: *key,
                },
                (
                    State::String {
                        state: String::Unicode3,
                        key,
                    },
                    b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F',
                ) => {
                    callback(Event::End(EventToken::UnicodeEscape), pos);
                    State::String {
                        state: String::Normal,
                        key: *key,
                    }
                }
                (
                    State::String {
                        state: String::Unicode0,
                        key: _,
                    }
                    | State::String {
                        state: String::Unicode1,
                        key: _,
                    }
                    | State::String {
                        state: String::Unicode2,
                        key: _,
                    }
                    | State::String {
                        state: String::Unicode3,
                        key: _,
                    },
                    _,
                ) => {
                    return Error::new(ErrKind::InvalidUnicodeEscape, data[pos], pos);
                }
                (
                    State::Idle
                    | State::Object { expect: _ }
                    | State::Array { expect: _ }
                    | State::Finished,
                    b' ' | b'\t' | b'\n' | b'\r',
                ) => self.state.clone(),
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'[',
                ) => {
                    self.context.enter_array(data[pos], pos)?;
                    callback(Event::ArrayStart, pos);
                    State::Array {
                        expect: Array::ItemOrEnd,
                    }
                }
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'{',
                ) => {
                    self.context.enter_object(data[pos], pos)?;
                    callback(Event::ObjectStart, pos);
                    State::Object {
                        expect: Object::Key,
                    }
                }
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'"',
                ) => {
                    callback(Event::Begin(EventToken::String), pos);
                    State::String {
                        state: String::Normal,
                        key: false,
                    }
                }
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b't' | b'f' | b'n',
                ) => self.start_token(data[pos], pos, &mut callback)?,
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'-', /*| b'+' */
                ) => {
                    callback(Event::Begin(EventToken::Number), pos);
                    State::Number { state: Num::Sign }
                }
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'0',
                ) => {
                    callback(Event::Begin(EventToken::Number), pos);
                    State::Number {
                        state: Num::LeadingZero,
                    }
                }
                (
                    State::Idle
                    | State::Object {
                        expect: Object::Value,
                    }
                    | State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b'1'..=b'9',
                ) => {
                    callback(Event::Begin(EventToken::Number), pos);
                    State::Number {
                        state: Num::BeforeDecimalPoint,
                    }
                }
                (
                    State::Object {
                        expect: Object::Value,
                    },
                    _,
                ) => return Error::new(ErrKind::ExpectedObjectValue, data[pos], pos),
                (
                    State::Array {
                        expect: Array::ItemOrEnd,
                    },
                    b']',
                ) => {
                    callback(Event::ArrayEnd, pos);
                    self.context.exit_array(pos)?;
                    self.maybe_exit_level()
                }
                (
                    State::Object {
                        expect: Object::Key,
                    },
                    b'"',
                ) => {
                    callback(Event::Begin(EventToken::Key), pos);
                    State::String {
                        state: String::Normal,
                        key: true,
                    }
                }
                (
                    State::Object {
                        expect: Object::Key,
                    },
                    b'}',
                ) => {
                    if self.context.after_comma.is_some() {
                        return Error::new(
                            ErrKind::TrailingComma,
                            self.context.after_comma.unwrap().0,
                            pos,
                        );
                    }
                    self.context.exit_object(pos)?;
                    callback(Event::ObjectEnd, pos);
                    self.maybe_exit_level()
                }
                (
                    State::Object {
                        expect: Object::Colon,
                    },
                    b':',
                ) => State::Object {
                    expect: Object::Value,
                },
                (
                    State::Object {
                        expect: Object::CommaOrEnd,
                    },
                    b',',
                ) => State::Object {
                    expect: Object::Key,
                },
                (
                    State::Object {
                        expect: Object::CommaOrEnd,
                    },
                    b'}',
                ) => {
                    self.context.exit_object(pos)?;
                    callback(Event::ObjectEnd, pos);
                    self.maybe_exit_level()
                }
                (
                    State::Array {
                        expect: Array::CommaOrEnd,
                    },
                    b',',
                ) => {
                    self.context.after_comma = Some((data[pos], pos));
                    State::Array {
                        expect: Array::ItemOrEnd,
                    }
                }
                (
                    State::Array {
                        expect: Array::CommaOrEnd,
                    },
                    b']',
                ) => {
                    callback(Event::ArrayEnd, pos);
                    self.context.exit_array(pos)?;
                    self.maybe_exit_level()
                }
                (
                    State::Token {
                        token: Token::True(True::R),
                    },
                    b'r',
                ) => State::Token {
                    token: Token::True(True::U),
                },
                (
                    State::Token {
                        token: Token::True(True::U),
                    },
                    b'u',
                ) => State::Token {
                    token: Token::True(True::E),
                },
                (
                    State::Token {
                        token: Token::True(True::E),
                    },
                    b'e',
                ) => {
                    callback(Event::End(EventToken::True), pos);
                    self.maybe_exit_level()
                }
                (
                    State::Token {
                        token: Token::False(False::A),
                    },
                    b'a',
                ) => State::Token {
                    token: Token::False(False::L),
                },
                (
                    State::Token {
                        token: Token::False(False::L),
                    },
                    b'l',
                ) => State::Token {
                    token: Token::False(False::S),
                },
                (
                    State::Token {
                        token: Token::False(False::S),
                    },
                    b's',
                ) => State::Token {
                    token: Token::False(False::E),
                },
                (
                    State::Token {
                        token: Token::False(False::E),
                    },
                    b'e',
                ) => {
                    callback(Event::End(EventToken::False), pos);
                    self.maybe_exit_level()
                }
                (
                    State::Token {
                        token: Token::Null(Null::U),
                    },
                    b'u',
                ) => State::Token {
                    token: Token::Null(Null::L1),
                },
                (
                    State::Token {
                        token: Token::Null(Null::L1),
                    },
                    b'l',
                ) => State::Token {
                    token: Token::Null(Null::L2),
                },
                (
                    State::Token {
                        token: Token::Null(Null::L2),
                    },
                    b'l',
                ) => {
                    callback(Event::End(EventToken::Null), pos);
                    self.maybe_exit_level()
                }

                // Wrong tokens
                (State::Idle, _) => {
                    return Error::new(ErrKind::InvalidRoot, data[pos], pos);
                }
                (
                    State::String {
                        state: String::Escaping,
                        key: _,
                    },
                    _,
                ) => return Error::new(ErrKind::InvalidStringEscape, data[pos], pos),
                (
                    State::Object {
                        expect: Object::Key,
                    },
                    _,
                ) => return Error::new(ErrKind::ExpectedObjectKey, data[pos], pos),
                (
                    State::Object {
                        expect: Object::Colon,
                    },
                    _,
                ) => return Error::new(ErrKind::ExpectedColon, data[pos], pos),
                (
                    State::Object {
                        expect: Object::CommaOrEnd,
                    },
                    _,
                ) => return Error::new(ErrKind::ExpectedObjectValue, data[pos], pos),
                (
                    State::Array {
                        expect: Array::ItemOrEnd,
                    }
                    | State::Array {
                        expect: Array::CommaOrEnd,
                    },
                    _,
                ) => return Error::new(ErrKind::ExpectedArrayItem, data[pos], pos),
                (State::Finished, _) => return Error::new(ErrKind::ContentEnded, data[pos], pos),
                (State::Token { token: _ }, _) => {
                    return Error::new(ErrKind::InvalidToken, data[pos], pos)
                }
            };
            pos += 1;
        }
        debug!("Consumed: {}", pos);
        Ok(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::warn;
    use test_log::test;

    #[test]
    fn test_zero_input() {
        let res = Tokenizer::<u32, u8>::new().t(b"");
        assert_eq!(res, Ok(0));
    }
    #[test]
    fn test_root_is_garbage() {
        assert_eq!(
            Tokenizer::<u32, u8>::new().t(b"a"),
            Error::new(ErrKind::InvalidRoot, b'a', 0)
        );
        assert_eq!(
            Tokenizer::<u32, u8>::new().t(b" a"),
            Error::new(ErrKind::InvalidRoot, b'a', 1)
        );
    }
    #[test]
    fn test_root_is_a_token() {
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"t"), Ok(1));
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"f"), Ok(1));
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"n"), Ok(1));
    }
    #[test]
    fn test_root_is_an_object() {
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"{"), Ok(1));
    }
    #[test]
    fn test_root_is_an_array() {
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"["), Ok(1));
    }
    #[test]
    fn test_root_is_a_string() {
        assert_eq!(Tokenizer::<u32, u8>::new().t(b"\"a\""), Ok(3));
    }

    #[test]
    fn test_no_garbage_after_root() {
        let mut parser = Tokenizer::new();
        let mut events: [Event; 16] = core::array::from_fn(|_| Event::Uninitialized);
        let result = collect_with_result(&mut parser, b"true extra", &mut events);
        assert_eq!(result, Error::new(ErrKind::ContentEnded, b'e', 5));
    }

    fn collect<'a, 'b, 'c>(
        parser: &'c mut Tokenizer,
        data: &'b [u8],
        store: &'a mut [Event],
    ) -> (usize, &'a [Event])
    where
        'b: 'a,
    {
        let mut index = 0;
        let consumed = parser
            .p(data, &mut |event, _pos| {
                warn!("Event: {:?}", event);
                store[index] = event.clone();
                index += 1;
            })
            .unwrap();
        (consumed, &store[..index])
    }

    fn collect_with_result<'a, 'b, 'c>(
        parser: &'c mut Tokenizer,
        data: &'b [u8],
        store: &'a mut [Event],
    ) -> Result<(usize, &'a [Event]), Error> {
        let mut index = 0;
        let consumed = parser.p(data, &mut |event, _pos| {
            warn!("Event: {:?}", event);
            store[index] = event.clone();
            index += 1;
        })?;
        Ok((consumed, &store[..index]))
    }

    #[test]
    fn test_parse_root_token_true() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b" true ", &mut m);
        assert_eq!(
            r,
            (
                6,
                [Event::Begin(EventToken::True), Event::End(EventToken::True),].as_slice()
            )
        );

        // sending the same in two, three chunks should yield the same
        let mut parser = Tokenizer::<u32, u8>::new();
        parser
            .p(b" tr", &mut |ev, _pos| {
                assert_eq!(ev, Event::Begin(EventToken::True));
            })
            .unwrap();
        parser
            .p(b"ue  ", &mut |ev, _pos| {
                assert_eq!(ev, Event::End(EventToken::True));
            })
            .unwrap();
    }

    #[test]
    fn test_after_root_should_not_accept_comma() {
        let mut m: [Event; 2] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b" true,", &mut m);
        assert_eq!(r, Error::new(ErrKind::ContentEnded, b',', 5));
    }

    #[test]
    fn test_parse_root_token_false() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b" false ", &mut m);
        assert_eq!(
            r,
            (
                7,
                [
                    Event::Begin(EventToken::False),
                    Event::End(EventToken::False),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_parse_root_token_null() {
        let mut m: [Event; 4] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"\tnull\n\r", &mut m);
        assert_eq!(
            r,
            (
                7,
                [Event::Begin(EventToken::Null), Event::End(EventToken::Null),].as_slice()
            )
        );
    }

    #[test]
    fn test_parse_root_token_string() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b" \"a\" ", &mut m);
        assert_eq!(
            r,
            (
                5,
                [
                    Event::Begin(EventToken::String),
                    Event::End(EventToken::String),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_boolean_null() {
        let mut parser = Tokenizer::new();
        let mut events: [Event; 16] = core::array::from_fn(|_| Event::Uninitialized);
        let (consumed, result) = collect(&mut parser, b"{\"flag\":true,\"nil\":null}", &mut events);
        assert_eq!(consumed, 24);
        assert_eq!(
            result,
            [
                Event::ObjectStart,
                Event::Begin(EventToken::Key),
                Event::End(EventToken::Key),
                Event::Begin(EventToken::True),
                Event::End(EventToken::True),
                Event::Begin(EventToken::Key),
                Event::End(EventToken::Key),
                Event::Begin(EventToken::Null),
                Event::End(EventToken::Null),
                Event::ObjectEnd,
            ]
        );
    }

    #[test]
    fn test_empty_object() {
        let mut m: [Event; 2] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"{}", &mut m);
        assert_eq!(r, (2, [Event::ObjectStart, Event::ObjectEnd].as_slice()));
    }

    #[test]
    fn test_object_with_whitespace() {
        let mut m: [Event; 2] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"{ \n\t\r}", &mut m);
        assert_eq!(r, (6, [Event::ObjectStart, Event::ObjectEnd].as_slice()));
    }

    #[test]
    fn test_invalid_object_key() {
        let mut m: [Event; 1] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"{true", &mut m);
        assert_eq!(r, Error::new(ErrKind::ExpectedObjectKey, b't', 1));
    }

    #[test]
    fn test_object_missing_colon() {
        let mut m: [Event; 3] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"{\"key\"true}", &mut m);
        assert_eq!(r, Error::new(ErrKind::ExpectedColon, b't', 6));
    }

    #[test]
    fn test_object_missing_value() {
        let mut m: [Event; 3] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"{\"key\":}", &mut m);
        assert_eq!(r, Error::new(ErrKind::ExpectedObjectValue, b'}', 7));
    }

    #[test]
    fn test_object_missing_comma() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"{\"a\":true\"b\":true}", &mut m);
        assert_eq!(r, Error::new(ErrKind::ExpectedObjectValue, b'"', 9));
    }

    #[test]
    fn test_nested_empty_objects() {
        let mut m: [Event; 10] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"{\"a\":{}}", &mut m);
        assert_eq!(
            r,
            (
                8,
                [
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::ObjectEnd,
                    Event::ObjectEnd,
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_deeply_nested_object() {
        let mut m: [Event; 16] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(
            &mut Tokenizer::new(),
            b"{\"a\":{\"b\":{\"c\":true}}}",
            &mut m,
        );
        assert_eq!(
            r,
            (
                22,
                [
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::True),
                    Event::End(EventToken::True),
                    Event::ObjectEnd,
                    Event::ObjectEnd,
                    Event::ObjectEnd,
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_multiple_nested_objects() {
        let mut m: [Event; 20] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(
            &mut Tokenizer::new(),
            b"{\"a\":{\"x\":true},\"b\":{\"y\":null}}",
            &mut m,
        );
        assert_eq!(
            r,
            (
                31,
                [
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::True),
                    Event::End(EventToken::True),
                    Event::ObjectEnd,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::Null),
                    Event::End(EventToken::Null),
                    Event::ObjectEnd,
                    Event::ObjectEnd,
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_partial_nested_object() {
        let mut m: [Event; 10] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"{\"a\":{\"b\":true", &mut m);
        assert_eq!(
            r,
            (
                14,
                [
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::True),
                    Event::End(EventToken::True),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_simple_array() {
        let mut m: [Event; 8] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"[true, false, null]", &mut m);
        assert_eq!(
            r,
            (
                19,
                [
                    Event::ArrayStart,
                    Event::Begin(EventToken::True),
                    Event::End(EventToken::True),
                    Event::Begin(EventToken::False),
                    Event::End(EventToken::False),
                    Event::Begin(EventToken::Null),
                    Event::End(EventToken::Null),
                    Event::ArrayEnd,
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_array_with_objects() {
        let mut m: [Event; 14] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(
            &mut Tokenizer::new(),
            b"[{\"a\":true}, {\"b\":null}]",
            &mut m,
        );
        assert_eq!(
            r,
            (
                24,
                [
                    Event::ArrayStart,
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::True),
                    Event::End(EventToken::True),
                    Event::ObjectEnd,
                    Event::ObjectStart,
                    Event::Begin(EventToken::Key),
                    Event::End(EventToken::Key),
                    Event::Begin(EventToken::Null),
                    Event::End(EventToken::Null),
                    Event::ObjectEnd,
                    Event::ArrayEnd,
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_empty_array() {
        let mut m: [Event; 2] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"[]", &mut m);
        assert_eq!(r, (2, [Event::ArrayStart, Event::ArrayEnd].as_slice()));
    }

    #[test]
    fn test_array_with_trailing_comma() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"[1,]", &mut m);
        assert_eq!(r, Error::new(ErrKind::TrailingComma, b',', 2));
    }

    #[test]
    fn test_array_with_trailing_comma_true() {
        let mut m: [Event; 6] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"[true,]", &mut m);
        assert_eq!(r, Error::new(ErrKind::TrailingComma, b',', 5));
    }

    #[test]
    fn test_array_with_trailing_comma_in_nested_array() {
        let mut m: [Event; 16] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"{ \"d\": [\"f\",\"b\",] }", &mut m);
        assert_eq!(r, Error::new(ErrKind::TrailingComma, b',', 15));
    }

    #[test]
    fn test_unicode_escape() {
        let mut m: [Event; 5] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect(&mut Tokenizer::new(), b"\"\\u0041\"", &mut m);
        assert_eq!(
            r,
            (
                8,
                [
                    Event::Begin(EventToken::String),
                    Event::Begin(EventToken::EscapeSequence),
                    Event::Begin(EventToken::UnicodeEscape),
                    Event::End(EventToken::UnicodeEscape),
                    Event::End(EventToken::String),
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn test_invalid_unicode_escape() {
        let mut m: [Event; 4] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"\"\\u00g\"", &mut m);
        assert_eq!(r, Error::new(ErrKind::InvalidUnicodeEscape, b'g', 5));
    }

    #[test]
    fn test_incomplete_unicode_escape() {
        let mut m: [Event; 4] = core::array::from_fn(|_| Event::Uninitialized);
        let r = collect_with_result(&mut Tokenizer::new(), b"\"\\u001\"", &mut m);
        assert_eq!(r, Error::new(ErrKind::InvalidUnicodeEscape, b'"', 6));
    }

    #[test]
    fn test_u8_bitstack() {
        // Test BitStack with u8 type (8-bit depth)
        let mut parser: Tokenizer<u8, u8> = Tokenizer::new();

        // Test simple array - should work with 8-bit depth
        let mut events = Vec::new();
        let result = parser.parse_full(b"[1,2,3]", &mut |event, _pos| {
            events.push(event);
        });

        assert!(result.is_ok());
        assert_eq!(events.len(), 8); // ArrayStart + 3*(Begin+End Number) + ArrayEnd
    }

    #[test]
    fn test_u64_bitstack() {
        // Test BitStack with u64 type (64-bit depth = much deeper nesting)
        let mut parser: Tokenizer<u64, u16> = Tokenizer::new();

        // Test deeply nested structure
        let json = b"[[[[1]]]]"; // 4 levels of nesting
        let mut events = Vec::new();
        let result = parser.parse_full(json, &mut |event, _pos| {
            events.push(event);
        });

        assert!(result.is_ok());
        // Should handle deep nesting easily with 64-bit storage
        assert!(events.len() > 8); // Multiple ArrayStart/End + Number events
    }

    // TODO: Array BitStack support needs custom implementation
    // Arrays don't implement the required bit operations for BitStack trait
}

#[cfg(test)]
mod conformance {
    use super::*;
    use test_log::test;

    fn assert_check(
        actual: (Result<usize, Error>, &[(Event, usize)]),
        expected: (Result<usize, Error>, &[(Event, usize)]),
        file: &str,
        line: u32,
    ) {
        if actual != expected {
            panic!(
                "assertion failed at {}:{}\n  left: {:?}\n right: {:?}",
                file, line, actual, expected
            );
        }
    }

    fn check_impl(
        data: &[u8],
        expect: Result<usize, Error>,
        expected_events: &[(Event, usize)],
        file: &str,
        line: u32,
    ) {
        let mut parser = Tokenizer::<u32, u8>::new();
        let mut results: [(Event, usize); 1024] =
            core::array::from_fn(|_| (Event::Uninitialized, 0));
        let mut received = 0;
        let parse_result = parser.parse_full(data, &mut |event, pos| {
            results[received] = (event, pos);
            received += 1;
        });
        let result_slice = &results[0..received];
        assert_check(
            (parse_result, result_slice),
            (expect, expected_events),
            file,
            line,
        );
    }

    macro_rules! check {
        ($data:expr, $expect:expr, $events:expr) => {
            check_impl($data, $expect, $events, file!(), line!())
        };
    }

    #[test]
    fn test_conformance_null() {
        check!(
            b"[null] ",
            Ok(7),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Null), 1),
                (Event::End(EventToken::Null), 4),
                (Event::ArrayEnd, 5)
            ]
        );
        check!(
            b"[true] ",
            Ok(7),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::True), 1),
                (Event::End(EventToken::True), 4),
                (Event::ArrayEnd, 5)
            ]
        );
        check!(
            b"[false] ",
            Ok(8),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::False), 1),
                (Event::End(EventToken::False), 5),
                (Event::ArrayEnd, 6)
            ]
        );
        check!(
            b"[\"a\"] ",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::End(EventToken::String), 3),
                (Event::ArrayEnd, 4)
            ]
        );
    }

    #[test]
    fn test_conformance_1() {
        check!(
            b"[2] ",
            Ok(4),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 2),
                (Event::ArrayEnd, 2)
            ]
        );
    }

    #[test]
    fn test_negative_number() {
        check!(
            b"[-1]",
            Ok(4),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 3),
                (Event::ArrayEnd, 3)
            ]
        );
        check!(
            b"[-1.0]",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 5),
                (Event::ArrayEnd, 5)
            ]
        );
    }

    // Add some tests for string escape sequences
    #[test]
    fn test_conformance_string_escape_sequences() {
        check!(
            b"[\"\\\"\"]",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
                (Event::Begin(EventToken::EscapeQuote), 3),
                (Event::End(EventToken::EscapeQuote), 3),
                (Event::End(EventToken::String), 4),
                (Event::ArrayEnd, 5)
            ]
        );
    }

    #[test]
    fn test_confformance_invalid_string_escape() {
        // valid escapes are \\, \t and \n and so on, lets do \x
        check!(
            b"[\"\\x\"]",
            Error::new(ErrKind::InvalidStringEscape, b'x', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
            ]
        );
    }

    // Try leaving an array and an object with a "broken" numer that ends in sign
    // or an exponent
    #[test]
    fn test_conformance_broken_numbers_in_array() {
        // leave at minus sign
        check!(
            b"[-]",
            Error::new(ErrKind::InvalidNumber, b']', 2),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
            ]
        );
        // leave at decimal point
        check!(
            b"[123.]",
            Error::new(ErrKind::InvalidNumber, b']', 5),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
            ]
        );
        // leave at exponent
        check!(
            b"[123e]",
            Error::new(ErrKind::InvalidNumber, b']', 5),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
            ]
        );
    }

    // number followed by space, tab, newline
    #[test]
    fn test_conformance_number_followed_by_space_tab_newline() {
        check!(
            b"123 ",
            Ok(4),
            &[
                (Event::Begin(EventToken::Number), 0),
                (Event::End(EventToken::Number), 3),
            ]
        );
        check!(
            b"123.42\t",
            Ok(7),
            &[
                (Event::Begin(EventToken::Number), 0),
                (Event::End(EventToken::Number), 6),
            ]
        );
    }

    // Same tests for objects
    #[test]
    fn test_conformance_broken_numbers_in_object() {
        // leave at minus sign
        check!(
            b"{ \"a\" : -}",
            Error::new(ErrKind::InvalidNumber, b'}', 9),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 2),
                (Event::End(EventToken::Key), 4),
                (Event::Begin(EventToken::Number), 8),
            ]
        );
        // leave at decimal point
        check!(
            b"{ \"a\" : 123.}",
            Error::new(ErrKind::InvalidNumber, b'}', 12),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 2),
                (Event::End(EventToken::Key), 4),
                (Event::Begin(EventToken::Number), 8),
            ]
        );
        // leave at exponent sign
        check!(
            b"{ \"a\" : 123e+}",
            Error::new(ErrKind::InvalidNumber, b'}', 13),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 2),
                (Event::End(EventToken::Key), 4),
                (Event::Begin(EventToken::Number), 8),
            ]
        );

        // leave at exponent
        check!(
            b"{ \"a\" : 123e}",
            Error::new(ErrKind::InvalidNumber, b'}', 12),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 2),
                (Event::End(EventToken::Key), 4),
                (Event::Begin(EventToken::Number), 8),
            ]
        );
    }

    #[test]
    fn test_confformance_2_str() {
        check!(
            b"[\"a\",,\"b\"]",
            Error::new(ErrKind::ExpectedArrayItem, b',', 5),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::End(EventToken::String), 3)
            ]
        );
    }

    #[test]
    fn test_confformance_2_num() {
        check!(
            b"[1,,2]",
            Error::new(ErrKind::ExpectedArrayItem, b',', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::Number), 2)
            ]
        );
    }

    #[test]
    fn test_conformance_unopened_array() {
        check!(
            b"1]",
            Error::new(ErrKind::UnopenedArray, b']', 1),
            &[
                (Event::Begin(EventToken::Number), 0),
                (Event::End(EventToken::NumberAndArray), 1),
                (Event::ArrayEnd, 1)
            ]
        );
    }

    #[test]
    fn test_conformance_lonely_int() {
        check!(
            b"42",
            Ok(2),
            &[
                (Event::Begin(EventToken::Number), 0),
                (Event::End(EventToken::Number), 2)
            ]
        );
    }

    #[test]
    fn test_conformance_trailing_object_comm() {
        check!(
            b"{\"id\":0,}",
            Error::new(ErrKind::TrailingComma, b',', 8),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 1),
                (Event::End(EventToken::Key), 4),
                (Event::Begin(EventToken::Number), 6),
                (Event::End(EventToken::Number), 7)
            ]
        );
    }

    #[test]
    fn test_conformance_double_array() {
        check!(
            b"false false",
            Error::new(ErrKind::ContentEnded, b'f', 6),
            &[
                (Event::Begin(EventToken::False), 0),
                (Event::End(EventToken::False), 4)
            ]
        );
    }

    #[test]
    fn test_conformance_i_structure_500_nested_arrays() {
        let data = include_bytes!("testdata/i_structure_500_nested_arrays.json");
        let starts: [(Event, usize); 255] = core::array::from_fn(|x: usize| (Event::ArrayStart, x));
        check!(
            data,
            Error::new(ErrKind::MaxDepthReached, b'[', 255),
            starts.as_slice()
        );
    }

    #[test]
    fn concormance_test_n_array_just_minus() {
        check!(
            b"[-]",
            Error::new(ErrKind::InvalidNumber, b']', 2),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_number_real_without_fractional_part() {
        check!(
            b"[1.]",
            Error::new(ErrKind::InvalidNumber, b']', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_number_plus_one() {
        check!(
            b"[+1]",
            Error::new(ErrKind::ExpectedArrayItem, b'+', 1),
            &[(Event::ArrayStart, 0)]
        );
    }

    #[test]
    fn conformance_test_n_number_minus_zero_one() {
        check!(
            b"[-01]",
            Error::new(ErrKind::InvalidNumber, b'1', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_number_neg_int_starting_with_zero() {
        check!(
            b"[-012]",
            Error::new(ErrKind::InvalidNumber, b'1', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_number_with_leading_zero() {
        check!(
            b"[012]",
            Error::new(ErrKind::InvalidNumber, b'1', 2),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number() {
        check!(
            b"[123e65]",
            Ok(8),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 7),
                (Event::ArrayEnd, 7)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number_0e_plus_1() {
        check!(
            b"[0e+1]",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 5),
                (Event::ArrayEnd, 5)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number_0e_1() {
        check!(
            b"[0e1]",
            Ok(5),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 4),
                (Event::ArrayEnd, 4)
            ]
        );
    }

    #[test]
    fn conformance_testy_number_0e_1_with_object() {
        check!(
            b"{\"a\":0e1}",
            Ok(9),
            &[
                (Event::ObjectStart, 0),
                (Event::Begin(EventToken::Key), 1),
                (Event::End(EventToken::Key), 3),
                (Event::Begin(EventToken::Number), 5),
                (Event::End(EventToken::NumberAndObject), 8),
                (Event::ObjectEnd, 8)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number_int_with_exp() {
        check!(
            b"[20e1]",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 5),
                (Event::ArrayEnd, 5)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number_real_capital_e() {
        check!(
            b"[1E22]",
            Ok(6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 5),
                (Event::ArrayEnd, 5)
            ]
        );
    }

    #[test]
    fn conformance_test_y_number_real_fraction_exponent() {
        check!(
            b"[123.456e78]",
            Ok(12),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1),
                (Event::End(EventToken::NumberAndArray), 11),
                (Event::ArrayEnd, 11)
            ]
        );
    }

    #[test]
    fn conformance_test_n_number_1_0e_minus() {
        check!(
            b"[1.0e-]",
            Error::new(ErrKind::InvalidNumber, b']', 6),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::Number), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_y_structure_lonely_negative_real() {
        check!(
            b"-0.1",
            Ok(4),
            &[
                (Event::Begin(EventToken::Number), 0),
                (Event::End(EventToken::Number), 4)
            ]
        );
    }

    #[test]
    fn conformance_n_structure_no_data() {
        check!(b"", Error::new(ErrKind::EmptyStream, b' ', 0), &[]);
    }

    #[test]
    fn conformance_n_string_unescaped_tab() {
        check!(
            b"[\"\t\"]",
            Error::new(ErrKind::UnescapedControlCharacter, b'\t', 2),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1)
            ]
        );
    }
    #[test]
    fn conformance_n_unescaped_ctrl_char() {
        check!(
            b"[\"a\x00a\"]",
            Error::new(ErrKind::UnescapedControlCharacter, b'\x00', 3),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_single_space() {
        check!(b" ", Error::new(ErrKind::UnfinishedStream, b' ', 1), &[]);
    }

    #[test]
    fn conformance_test_n_string_1_surrogate_then_escape_u1() {
        check!(
            b"[\"\\uD800\\u1\"]",
            Error::new(ErrKind::InvalidUnicodeEscape, b'"', 11),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
                (Event::Begin(EventToken::UnicodeEscape), 4),
                (Event::End(EventToken::UnicodeEscape), 7),
                (Event::Begin(EventToken::EscapeSequence), 8),
                (Event::Begin(EventToken::UnicodeEscape), 10)
            ]
        );
    }

    #[test]
    fn conformance_test_n_string_1_surrogate_then_escape_u1x() {
        check!(
            b"[\"\\uD800\\u1x\"]",
            Error::new(ErrKind::InvalidUnicodeEscape, b'x', 11),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
                (Event::Begin(EventToken::UnicodeEscape), 4),
                (Event::End(EventToken::UnicodeEscape), 7),
                (Event::Begin(EventToken::EscapeSequence), 8),
                (Event::Begin(EventToken::UnicodeEscape), 10)
            ]
        );
    }

    #[test]
    fn conformance_test_n_string_unescaped_tab() {
        check!(
            b"[\"\t\"]",
            Error::new(ErrKind::UnescapedControlCharacter, b'\t', 2),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1)
            ]
        );
    }

    #[test]
    fn conformance_test_n_string_incomplete_escaped_character() {
        check!(
            b"[\"\\u00A\"]",
            Error::new(ErrKind::InvalidUnicodeEscape, b'"', 7),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
                (Event::Begin(EventToken::UnicodeEscape), 4),
            ]
        );
    }

    #[test]
    fn conformance_test_n_string_incomplete_surrogate() {
        check!(
            b"[\"\\uD834\\uDd\"]",
            Error::new(ErrKind::InvalidUnicodeEscape, b'"', 12),
            &[
                (Event::ArrayStart, 0),
                (Event::Begin(EventToken::String), 1),
                (Event::Begin(EventToken::EscapeSequence), 2),
                (Event::Begin(EventToken::UnicodeEscape), 4),
                (Event::End(EventToken::UnicodeEscape), 7),
                (Event::Begin(EventToken::EscapeSequence), 8),
                (Event::Begin(EventToken::UnicodeEscape), 10)
            ]
        );
    }
}
