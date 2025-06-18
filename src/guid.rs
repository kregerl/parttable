use binary_struct::{prelude::*, BinaryStruct};

// https://www.ietf.org/rfc/rfc4122.txt
// 4.1.2.  Layout and Byte Order
#[derive(BinaryStruct, Debug)]
pub struct Guid {
    // The low field of the timestamp
    time_low: u32,
    // The middle field of the timestamp
    time_mid: u16,
    // The high field of the timestamp multiplexed with the version number
    time_high_and_version: u16,
    // The high field of the clock sequence multiplexed with the variant
    clock_seq_high_and_reserved: u8,
    // The low field of the clock sequence
    clock_seq_low: u8,
    // The spatially unique node identifier
    node_identifier: [u8; 6],
}

impl ToString for Guid {
    fn to_string(&self) -> String {
        let clock_seq = u16::from_be_bytes([self.clock_seq_high_and_reserved, self.clock_seq_low]);

        // Ignore first 2 bytes.
        let mut tmp_buffer = [0u8; 8];
        tmp_buffer[2..].copy_from_slice(&self.node_identifier);
        let node = u64::from_be_bytes(tmp_buffer);

        let guid = format!(
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            self.time_low, self.time_mid, self.time_high_and_version, clock_seq, node
        )
        .to_uppercase();
        guid
    }
}