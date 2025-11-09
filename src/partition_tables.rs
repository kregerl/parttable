pub mod mbr;
pub mod gpt;

pub const BOOTSTRAPER_LENGTH: usize = 446;
pub const GPT_PARTITION_TYPE: u8 = 0xee;
pub const NTFS_GPT_PARTITION_TYPE: &'static str = "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7";
pub const NTFS_MBR_PARTITION_TYPE: u8 = 0x07; 