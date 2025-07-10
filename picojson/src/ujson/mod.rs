// SPDX-License-Identifier: Apache-2.0

mod bitstack;
// User-facing API exports
pub use bitstack::ArrayBitStack;

// Main API: BitStack configuration system
pub use bitstack::ArrayBitBucket;
pub use bitstack::BitBucket;
pub use bitstack::BitStackConfig;
pub use bitstack::BitStackStruct;
pub use bitstack::DefaultConfig;
pub use bitstack::DepthCounter;

pub(super) use tokenizer::Tokenizer;

pub use tokenizer::Error;
pub(super) use tokenizer::Event;
pub(super) use tokenizer::EventToken;

mod tokenizer;
