pub mod mbr;
pub mod gpt;

pub const BOOTSTRAPER_LENGTH: usize = 446;
pub const GPT_PARTITION_TYPE: u8 = 0xee;