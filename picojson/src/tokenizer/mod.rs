// SPDX-License-Identifier: Apache-2.0

mod bitstack;
// Legacy exports for backward compatibility
pub use bitstack::ArrayBitStack;
pub use bitstack::BitBucket as BitStack;

// Main API: BitStack configuration system
pub use bitstack::ArrayBitBucket;
pub use bitstack::BitBucket;
pub use bitstack::BitStackConfig;
pub use bitstack::BitStackStruct;
pub use bitstack::DefaultConfig;
pub use bitstack::DepthCounter;

pub(super) use tokenizer::Tokenizer;

pub(super) use tokenizer::Event;
pub(super) use tokenizer::EventToken;

mod tokenizer;
