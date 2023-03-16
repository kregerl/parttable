use std::{io::{Read, self}, path::Path};

use crate::bytestream::{Readable, ByteStream};

#[derive(Debug, Copy, Clone)]
struct DriverDescriptorEntry {
    start_lba: u32,
    size_in_sectors: u16,
    sys_type: u16,
}

impl Readable for DriverDescriptorEntry {
    fn read(reader: &mut crate::bytestream::ByteStream) -> std::io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            start_lba: reader.read::<u32>()?,
            size_in_sectors: reader.read::<u16>()?,
            sys_type: reader.read::<u16>()?,
        })
    }
}

#[derive(Debug)]
struct DriverDescriptorMap {
    // 2 bytes
    signature: String,
    block_size: u16,
    block_count: u32,
    device_type: u16,
    device_id: u16,
    driver_data: u32,
    driver_descriptor_count: u16,
    driver_descriptor_map: [DriverDescriptorEntry; 8],
}

impl Readable for DriverDescriptorMap {
    fn read(reader: &mut crate::bytestream::ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            signature: String::from_utf8(reader.read_byte_array::<2>()?.to_vec()).unwrap(),
            block_size: reader.read::<u16>()?,
            block_count: reader.read::<u32>()?,
            device_type: reader.read::<u16>()?,
            device_id: reader.read::<u16>()?,
            driver_data: reader.read::<u32>()?,
            driver_descriptor_count: reader.read::<u16>()?,
            driver_descriptor_map: reader.read_array::<DriverDescriptorEntry, 8>()?,
        })
    }
}

pub fn is_apm_disk(path: &str) -> io::Result<bool> {
    let mut stream = ByteStream::new(&Path::new(path))?;
    let driver_descriptor_map = stream.read::<DriverDescriptorMap>()?;
    Ok(driver_descriptor_map.signature == "ER")
}

#[derive(Debug)]
pub struct ApmPartitionTable {
    signature: String,
    number_of_partitions: u32,
    starting_lba: u32,
    size_in_sectors: u32,
    partition_name: String,
    partition_type: String,
    starting_lba_of_data: u32,
    size_in_sectors_of_date: u32,
    partition_status: u32,
    starting_lba_boot_code: u32,
    size_boot_code: u32,
    address_boot_loader: u32,
    boot_entry_point: u32,
    checksum: u32,
    processor_type: [u8; 16],
}

impl Readable for ApmPartitionTable {
    fn read(reader: &mut crate::bytestream::ByteStream) -> std::io::Result<Self>
    where
        Self: Sized,
    {
        let signature = String::from_utf8(reader.read_byte_array::<2>()?.to_vec()).unwrap();
        // reserved 2
        let _ = reader.read_byte_array::<2>()?;
        let number_of_partitions = reader.read::<u32>()?;
        let starting_lba = reader.read::<u32>()?;
        let size_in_sectors = reader.read::<u32>()?;
        let partition_name = String::from_utf8(reader.read_byte_array::<32>()?.to_vec()).unwrap();
        let partition_type = String::from_utf8(reader.read_byte_array::<32>()?.to_vec()).unwrap();
        let starting_lba_of_data = reader.read::<u32>()?;
        let size_in_sectors_of_date = reader.read::<u32>()?;
        let partition_status = reader.read::<u32>()?;
        let starting_lba_boot_code = reader.read::<u32>()?;
        let size_boot_code = reader.read::<u32>()?;
        let address_boot_loader = reader.read::<u32>()?;
        // reserved 4
        let _ = reader.read_byte_array::<4>()?;
        let boot_entry_point = reader.read::<u32>()?;
        // reserved 4
        let _ = reader.read_byte_array::<4>()?;
        let checksum = reader.read::<u32>()?;
        let processor_type = reader.read_byte_array::<16>()?;
        Ok(Self {
            signature,
            number_of_partitions,
            starting_lba,
            size_in_sectors,
            partition_name,
            partition_type,
            starting_lba_of_data,
            size_in_sectors_of_date,
            partition_status,
            starting_lba_boot_code,
            size_boot_code,
            address_boot_loader,
            boot_entry_point,
            checksum,
            processor_type,
        })
    }
}

impl ApmPartitionTable {
    pub fn is_valid_apm_partition_table_entry(&self) -> bool {
        !self.processor_type.is_empty()
    }
}

#[derive(Debug)]
#[repr(u32)]
enum ApmPartitionStatus {
    Valid = 0x00000001,
    Allocated = 0x00000002,
    InUse = 0x00000004,
    ContainsBootInfo = 0x00000008,
    Readable = 0x00000010,
    Writable = 0x00000020,
    PositionIndependent = 0x00000040,
    ChainCompatibleDrive = 0x00000100,
    RealDriver = 0x00000200,
    ChainDriver = 0x00000400,
    AutomaticallyMount = 0x40000000,
    StartupPartition = 0x80000000,
}

pub fn parse_apm(path: &str) -> io::Result<Vec<ApmPartitionTable>> {
    let mut stream = ByteStream::new(&Path::new(path))?;
    stream.jump_to_sector(1)?;
    let mut partition_tables = Vec::new();
    loop {
        let partition_table = stream.read::<ApmPartitionTable>()?;
        if !partition_table.is_valid_apm_partition_table_entry() {
            break;
        }
        partition_tables.push(partition_table);
    }

    Ok(partition_tables)
}