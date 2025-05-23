use crate::{
    bytestream::{interpret_bytes_as_utf16, ByteStream, Readable, SECTOR_SIZE},
    mbr::{MbrPartitionTableEntryNode, BOOT_SIGNATURE},
    Timestomp,
};
use byteorder::{LittleEndian, WriteBytesExt};
use chrono::{offset, DateTime, Local};
use core::time;
use csv::Writer;
use prettytable::{row, Table};
use std::{
    collections::HashMap,
    fmt::Display,
    fs::{File, OpenOptions},
    io::{self, Seek},
    path::Path,
    string::FromUtf8Error,
    time::{Duration, UNIX_EPOCH},
};

#[derive(Debug)]
struct NtfsPartitionBootRecord {
    jump_instruction: [u8; 3],
    oem_id: [u8; 8],
    // BPB
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    device_type: u8,
    number_of_sectors_in_volume: u64,
    mft_lcn: u64, // Logical cluster number where the MFT starts.
    backup_mft_lcn: u64,
    // - If this value, when read in two’s complement, is positive,
    //   i.e. if its value goes from 00h to 7Fh (0000 0000 a 0111 1111),
    //   it actually designates the number of clusters per register
    // - If this value, when read in two’s complement, is negative,
    //   i.e. if its value goes from 80h to FFh (1000 0000 a 1111 1111), the
    //   size in bytes of each register will be equal to  2 to the power of the byte absolute value.
    mft_size: i8,
    number_of_clusters_per_index_buffer: u8,
    serial_number: [u8; 8],
    error_bytes: Vec<u8>,
}

impl Readable for NtfsPartitionBootRecord {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        // 13 Bytes of error marking space (0x0e - 0x14, 0x16 - 0x17, 0x20 - 0x23)
        let mut error_bytes = Vec::with_capacity(13);
        // 22 Bytes of unused space (0x18 - 0x1F, 0x24 - 0x27, 0x41 - 0x43, 0x45 - 0x47, 0x50 - 0x53)
        let mut unused_space = Vec::with_capacity(22);

        let jump_instruction = reader.read_byte_array::<3>()?;
        // Interpreted as a string
        let oem_id = reader.read_byte_array::<8>()?;
        let bytes_per_sector = reader.read_le::<u16>()?;
        let sectors_per_cluster = reader.read::<u8>()?;
        error_bytes.append(&mut reader.read_byte_array::<7>()?.to_vec());
        let device_type = reader.read::<u8>()?;
        error_bytes.append(&mut reader.read_byte_array::<2>()?.to_vec());
        unused_space.append(&mut reader.read_byte_array::<8>()?.to_vec());
        error_bytes.append(&mut reader.read_byte_array::<4>()?.to_vec());
        unused_space.append(&mut reader.read_byte_array::<4>()?.to_vec());
        let number_of_sectors_in_volume = reader.read_le::<u64>()?;
        let mft_lcn = reader.read_le::<u64>()?;
        let backup_mft_lcn = reader.read_le::<u64>()?;
        let mft_size = reader.read::<i8>()?;
        unused_space.append(&mut reader.read_byte_array::<3>()?.to_vec());
        let number_of_clusters_per_index_buffer = reader.read::<u8>()?;
        unused_space.append(&mut reader.read_byte_array::<3>()?.to_vec());
        let serial_number = reader.read_byte_array::<8>()?;
        unused_space.append(&mut reader.read_byte_array::<4>()?.to_vec());

        Ok(Self {
            jump_instruction,
            oem_id,
            bytes_per_sector,
            sectors_per_cluster,
            device_type,
            number_of_sectors_in_volume,
            mft_lcn,
            backup_mft_lcn,
            mft_size,
            number_of_clusters_per_index_buffer,
            serial_number,
            error_bytes,
        })
    }
}

impl NtfsPartitionBootRecord {
    pub fn oem_id_str(&self) -> Result<String, FromUtf8Error> {
        String::from_utf8(self.oem_id.to_vec()).and_then(|s| Ok(s.trim().into()))
    }
}

pub fn parse_pbr(path: &Path, starting_lba: u64) -> io::Result<Vec<MftFileRecord>> {
    let mut stream = ByteStream::new(path, SECTOR_SIZE, starting_lba)?;
    // stream.jump_to_sector(starting_lba)?;
    let partition_boot_record = stream.read::<NtfsPartitionBootRecord>()?;
    match partition_boot_record.oem_id_str().as_deref() {
        Ok("NTFS") => {
            // 510(Sector size - signature) - 84 (PBR btyes read) = 426 Boot code
            let _ = stream.read_byte_array::<426>()?;
            assert_eq!(
                stream.read_byte_array::<2>()?,
                BOOT_SIGNATURE,
                "End of sector was not reached"
            );

            let mft_lba = starting_lba
                + (partition_boot_record.mft_lcn
                    * partition_boot_record.sectors_per_cluster as u64);
            // - If this value, when read in two’s complement, is positive,
            //   i.e. if its value goes from 00h to 7Fh (0000 0000 a 0111 1111),
            //   it actually designates the number of clusters per register
            // - If this value, when read in two’s complement, is negative,
            //   i.e. if its value goes from 80h to FFh (1000 0000 a 1111 1111), the
            //   size in bytes of each register will be equal to  2 to the power of the byte absolute value.
            let mft_size = if partition_boot_record.mft_size < 0 {
                2u32.pow(partition_boot_record.mft_size.abs() as u32)
            } else {
                (partition_boot_record.mft_size as usize * 8 * SECTOR_SIZE) as u32
            };
            parse_mft(path, mft_lba, mft_size)
        }
        Err(e) => panic!("Error parsing OEM ID: {}", e),
        _ => panic!("Cannot parse $MFT of a non-NTFS partition"),
    }
}

#[derive(Debug)]
struct MftFileDescriptor {
    signature: [u8; 4],
    offest_of_update_seq: u16,
    size_of_update_seq: u16,
    log_file_seq_nr: u64,
    use_count: u8,
    deletion_count: u8,
    hard_link_count: u16,
    offset_first_attribute: u16,
    // 0x00 == Register free, 0x01 == Register in use, 0x02 == Register is a directory
    flags: u16,
    file_size_on_disk: u32,
    space_allocated: u32,
    base_register: u64,
    next_attribute_id: u16,
    update_sequence_number: u16,
    update_sequence: u32,
}

impl Readable for MftFileDescriptor {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            signature: reader.read_byte_array::<4>()?,
            offest_of_update_seq: reader.read_le::<u16>()?,
            size_of_update_seq: reader.read_le::<u16>()?,
            log_file_seq_nr: reader.read_le::<u64>()?,
            use_count: reader.read::<u8>()?,
            deletion_count: reader.read::<u8>()?,
            hard_link_count: reader.read_le::<u16>()?,
            offset_first_attribute: reader.read_le::<u16>()?,
            flags: reader.read_le::<u16>()?,
            file_size_on_disk: reader.read_le::<u32>()?,
            space_allocated: reader.read_le::<u32>()?,
            base_register: reader.read_le::<u64>()?,
            next_attribute_id: reader.read_le::<u16>()?,
            update_sequence_number: reader.read_le::<u16>()?,
            update_sequence: reader.read_le::<u32>()?,
        })
    }
}

/// Struct for holding the common values between all attribute headers.
/// Flags
/// 0x0001 == Compressed
/// 0x4000 == Encrypted
/// 0x8000 == Sparse
#[derive(Debug, Clone)]
struct CommonAttributeHeader {
    attribute_type: u32,
    length: u32,
    non_resident_flag: u8,
    name_length: u8,
    name_offset: u16,
    flags: [u8; 2],
    attribute_id: u16,
}

impl Readable for CommonAttributeHeader {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            attribute_type: reader.read_le::<u32>()?,
            length: reader.read_le::<u32>()?,
            non_resident_flag: reader.read::<u8>()?,
            name_length: reader.read::<u8>()?,
            name_offset: reader.read_le::<u16>()?,
            flags: reader.read_byte_array::<2>()?,
            attribute_id: reader.read_le::<u16>()?,
        })
    }
}

/// Data relating to resident attributes only.
#[derive(Debug, Clone)]
struct ResidentAttributeHeader {
    attribute_length: u32,
    attribute_offset: u16,
    indexed_flag: u8,
}

impl Readable for ResidentAttributeHeader {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            attribute_length: reader.read_le::<u32>()?,
            attribute_offset: reader.read_le::<u16>()?,
            indexed_flag: reader.read::<u8>()?,
        })
    }
}

/// Data specific to non resident attribute headers
#[derive(Debug, Clone)]
struct NonResidentAttributeHeader {
    starting_vcn: u64,
    ending_vcn: u64,
    data_runs_offset: u16,
    compression_unit_size: u16,
    file_allocation_size: u64,
    file_real_size: u64,
    initial_stream_size: u64,
}

impl Readable for NonResidentAttributeHeader {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        let starting_vcn = reader.read_le::<u64>()?;
        let ending_vcn = reader.read_le::<u64>()?;
        let data_runs_offset = reader.read_le::<u16>()?;
        let compression_unit_size = reader.read_le::<u16>()?;
        // Discard 4 bytes of 0 padding
        let _ = reader.read_byte_array::<4>()?;
        let allocated_size_of_attribute = reader.read_le::<u64>()?;
        let real_size_of_attribute = reader.read_le::<u64>()?;
        let initialized_data_size = reader.read_le::<u64>()?;
        Ok(Self {
            starting_vcn,
            ending_vcn,
            data_runs_offset,
            compression_unit_size,
            file_allocation_size: allocated_size_of_attribute,
            file_real_size: real_size_of_attribute,
            initial_stream_size: initialized_data_size,
        })
    }
}

// The MFT Must have one of these attribute headers.
#[derive(Debug, Clone)]
enum AttributeHeader {
    ResidentNoName {
        common_header: CommonAttributeHeader,
        resident_header: ResidentAttributeHeader,
    },
    ResidentNamed {
        common_header: CommonAttributeHeader,
        resident_header: ResidentAttributeHeader,
        attribute_name: String,
    },
    NonResidentNoName {
        common_header: CommonAttributeHeader,
        non_resident_header: NonResidentAttributeHeader,
    },
    NonResidentNamed {
        common_header: CommonAttributeHeader,
        non_resident_header: NonResidentAttributeHeader,
        attribute_name: String,
    },
}

impl Readable for AttributeHeader {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        let common_attribute_header = reader.read::<CommonAttributeHeader>()?;
        // If the non resident flag is off
        Ok(if common_attribute_header.non_resident_flag == 0 {
            let resident_attribute_header = reader.read::<ResidentAttributeHeader>()?;
            // Discard one byte for padding for resident attributes
            let _ = reader.read::<u8>()?;

            // if attributes has a name
            if common_attribute_header.name_length > 0 {
                // Attribute name is 2*N since it is stored in unicode(2 bytes)
                let attribute_name_bytes =
                    reader.read_raw(common_attribute_header.name_length as usize * 2)?;
                let name = interpret_bytes_as_utf16(&attribute_name_bytes)
                    .expect("Invalid utf16 bytes in attribute header.");
                AttributeHeader::ResidentNamed {
                    common_header: common_attribute_header,
                    resident_header: resident_attribute_header,
                    attribute_name: name,
                }
            } else {
                AttributeHeader::ResidentNoName {
                    common_header: common_attribute_header,
                    resident_header: resident_attribute_header,
                }
            }
        } else {
            // Read the non resident attribute header
            let non_resident_attribute_header = reader.read::<NonResidentAttributeHeader>()?;
            if common_attribute_header.name_length > 0 {
                // If is is named, read the name
                let attribute_name_bytes =
                    reader.read_raw(common_attribute_header.name_length as usize * 2)?;
                let name = interpret_bytes_as_utf16(&attribute_name_bytes).unwrap_or("".into());
                AttributeHeader::NonResidentNamed {
                    common_header: common_attribute_header,
                    non_resident_header: non_resident_attribute_header,
                    attribute_name: name,
                }
            } else {
                // Otherwise return the nameless non resident header
                AttributeHeader::NonResidentNoName {
                    common_header: common_attribute_header,
                    non_resident_header: non_resident_attribute_header,
                }
            }
        })
    }
}

impl AttributeHeader {
    pub fn attribute_type(&self) -> u32 {
        self.common_header().attribute_type
    }

    pub fn attribute_length(&self) -> u32 {
        self.common_header().length
    }

    pub fn file_allocation_size(&self) -> Option<u64> {
        match self {
            AttributeHeader::ResidentNoName { .. } => None,
            AttributeHeader::ResidentNamed { .. } => None,
            AttributeHeader::NonResidentNoName {
                non_resident_header,
                ..
            } => Some(non_resident_header.file_allocation_size),
            AttributeHeader::NonResidentNamed {
                non_resident_header,
                ..
            } => Some(non_resident_header.file_allocation_size),
        }
    }

    pub fn datarun_offset(&self) -> u16 {
        match self {
            AttributeHeader::NonResidentNoName {
                non_resident_header,
                ..
            } => non_resident_header.data_runs_offset,
            AttributeHeader::NonResidentNamed {
                non_resident_header,
                ..
            } => non_resident_header.data_runs_offset,
            _ => 0,
        }
    }

    fn common_header(&self) -> &CommonAttributeHeader {
        match self {
            AttributeHeader::ResidentNoName { common_header, .. } => common_header,
            AttributeHeader::ResidentNamed { common_header, .. } => common_header,
            AttributeHeader::NonResidentNoName { common_header, .. } => common_header,
            AttributeHeader::NonResidentNamed { common_header, .. } => common_header,
        }
    }
}

#[derive(Debug, Clone)]
struct PermissionParseError;

impl Display for PermissionParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid NTFS permissions integer")
    }
}

#[derive(Debug)]
#[repr(u32)]
enum NtfsPermissions {
    ReadOnly = 0x0001,
    Hidden = 0x0002,
    System = 0x0004,
    Archive = 0x0020,
    Device = 0x0040,
    Normal = 0x0080,
    Temporary = 0x0100,
    SparseFile = 0x0200,
    ReparseFile = 0x0400,
    Compressed = 0x0800,
    Offline = 0x1000,
    NotContentIndexed = 0x2000,
    Encrypted = 0x4000,
}

impl TryFrom<u32> for NtfsPermissions {
    type Error = PermissionParseError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0x0001 => Ok(NtfsPermissions::ReadOnly),
            0x0002 => Ok(NtfsPermissions::Hidden),
            0x0004 => Ok(NtfsPermissions::System),
            0x0020 => Ok(NtfsPermissions::Archive),
            0x0040 => Ok(NtfsPermissions::Device),
            0x0080 => Ok(NtfsPermissions::Normal),
            0x0100 => Ok(NtfsPermissions::Temporary),
            0x0200 => Ok(NtfsPermissions::SparseFile),
            0x0400 => Ok(NtfsPermissions::ReparseFile),
            0x0800 => Ok(NtfsPermissions::Compressed),
            0x1000 => Ok(NtfsPermissions::Offline),
            0x2000 => Ok(NtfsPermissions::NotContentIndexed),
            0x4000 => Ok(NtfsPermissions::Encrypted),
            _ => Err(PermissionParseError),
        }
    }
}

#[derive(Debug)]
struct NtfsDatetime {
    datetime: DateTime<Local>,
}

impl NtfsDatetime {
    pub fn ole2(&self) -> u64 {
        (self.datetime.timestamp() as u64 * 10000000) + 116444736000000000
    }
    pub fn ole2_le_bytes(&self) -> [u8; 8] {
        self.ole2().to_le_bytes()
    }
}

impl From<u64> for NtfsDatetime {
    fn from(unix_epoch: u64) -> Self {
        let datetime = DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(unix_epoch));
        Self { datetime }
    }
}

impl Readable for NtfsDatetime {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        let ole2_timestamp = reader.read_le::<u64>()?;
        let timestamp_unix_epoch = (ole2_timestamp - 116444736000000000) / 10000000;
        let datetime =
            DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(timestamp_unix_epoch));
        Ok(Self { datetime })
    }
}

#[derive(Debug)]
struct StandardInformation {
    datetime_file_creation: NtfsDatetime,
    datetime_file_modification: NtfsDatetime,
    datetime_mft_modification: NtfsDatetime,
    datetime_file_reading: NtfsDatetime,
    file_permission_flags: u32,
    maximum_number_versions: u32,
    version_number: u64,
}

impl Readable for StandardInformation {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            datetime_file_creation: reader.read::<NtfsDatetime>()?,
            datetime_file_modification: reader.read::<NtfsDatetime>()?,
            datetime_mft_modification: reader.read::<NtfsDatetime>()?,
            datetime_file_reading: reader.read::<NtfsDatetime>()?,
            file_permission_flags: reader.read_le::<u32>()?,
            maximum_number_versions: reader.read_le::<u32>()?,
            version_number: reader.read_le::<u64>()?,
        })
    }
}

#[derive(Debug)]
struct FileName {
    reference_to_parent_dir: u64,
    datetime_file_creation: NtfsDatetime,
    datetime_file_modification: NtfsDatetime,
    datetime_mft_modification: NtfsDatetime,
    datetime_file_reading: NtfsDatetime,
    file_size_allocated_on_disk: u64,
    real_file_size: u64,
    file_permission_flags: u32,
    extended_attributes_and_reparse: u32,
    name_size: u8,
    namespace: u8,
    name: String,
    // 6 Bytes of padding
}

impl Readable for FileName {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {   
        let reference_to_parent_dir = reader.read_le::<u64>()?;
        let datetime_file_creation = reader.read::<NtfsDatetime>()?;
        let datetime_file_modification = reader.read::<NtfsDatetime>()?;
        let datetime_mft_modification = reader.read::<NtfsDatetime>()?;
        let datetime_file_reading = reader.read::<NtfsDatetime>()?;
        let file_size_allocated_on_disk = reader.read_le::<u64>()?;
        let real_file_size = reader.read_le::<u64>()?;
        let file_permission_flags = reader.read_le::<u32>()?;
        let extended_attributes_and_reparse = reader.read_le::<u32>()?;
        let name_size = reader.read::<u8>()?;
        let namespace = reader.read::<u8>()?;
        let name_bytes = reader.read_raw(name_size as usize * 2)?;
        let name = interpret_bytes_as_utf16(&name_bytes)
            .expect("Invalid utf16 bytes in attribute header.");
        // 6 Bytes of padding
        let _ = reader.read_byte_array::<6>()?;
        Ok(Self {
            reference_to_parent_dir,
            datetime_file_creation,
            datetime_file_modification,
            datetime_mft_modification,
            datetime_file_reading,
            file_size_allocated_on_disk,
            real_file_size,
            file_permission_flags,
            extended_attributes_and_reparse,
            name_size,
            namespace,
            name,
        })
    }
}

#[derive(Debug)]
struct DataRun {
    length: u64,
    offset: i64,
}

impl Readable for DataRun {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        // Assuming the data run will be at most 8 bytes
        let datarun = reader.read_byte_array::<8>()?;
        let mut length: u64 = 0;
        let mut offset: i64 = 0;

        let high_nibble = (datarun[0] & 0b11110000) >> 4;
        let low_nibble = datarun[0] & 0b00001111;

        for i in 0..low_nibble as usize {
            length |= (datarun[1 + i] as u64) << (i * 8);
        }

        for i in 0..high_nibble as usize {
            offset |= (datarun[1 + low_nibble as usize + i] as i64) << (i * 8);
        }

        if offset & (1 << (high_nibble * 8 - 1)) > 0 {
            for i in 0..high_nibble as usize {
                offset |= (0xFF as i64) << (i * 8);
            }
        }

        Ok(Self { length, offset })
    }
}

#[derive(Debug)]
enum MftAttribute {
    StandardInformation(StandardInformation),
    FileName(FileName),
    Data(DataRun),
}

#[derive(Debug)]
pub struct MftFileRecord {
    file_descriptor: MftFileDescriptor,
    attributes: Vec<(u64, AttributeHeader, MftAttribute)>,
}

impl MftFileRecord {
    pub fn has_file_name_attribute(&self, file_name: &str) -> bool {
        self.attributes
            .iter()
            .filter_map(|(size, header, attribute)| {
                if header.attribute_type() == 0x30 {
                    Some((size, header, attribute))
                } else {
                    None
                }
            })
            .find(|(_, _, attribute)| match attribute {
                MftAttribute::FileName(file_name_attr) => file_name_attr.name == file_name,
                _ => false,
            })
            .is_some()
    }
}

fn parse_mft_file_record(
    path: &Path,
    starting_offset: u64,
    mft_record_size: usize,
    ignore_data_attribute: bool,
) -> io::Result<Option<MftFileRecord>> {
    let mut stream = ByteStream::from_byte_offset(path, mft_record_size, starting_offset)?;
    let mft_file_descriptor = stream.read::<MftFileDescriptor>()?;
    let mut attributes: Vec<(u64, AttributeHeader, MftAttribute)> = Vec::new();
    match &mft_file_descriptor.signature {
        b"FILE" => {
            let attribute_start_offset =
                starting_offset + mft_file_descriptor.offset_first_attribute as u64;
            let mut attribute_offset: u64 = 0;

            let mut duplicate_attribute_map: Vec<u32> = Vec::new();
            while stream.peek_le::<u32>()? != u32::MAX {
                let offset = attribute_start_offset + attribute_offset;
                stream = ByteStream::from_byte_offset(path, mft_record_size, offset)?;
                let attribute_header = stream.read::<AttributeHeader>()?;
                let length = attribute_header.attribute_length();
                // FIXME: account for update sequences: https://stackoverflow.com/questions/55126151/ntfs-mft-datarun
                // Currently ignoring $DATA attributes that are not the $MFT itself.

                // FIXME: Currently ignoring duplicate attributes.
                // For file records with hard links, there can be more than one $FILE_NAME attribute 
                if duplicate_attribute_map.iter().find(|&attr_type| *attr_type == attribute_header.attribute_type()).is_some() {
                    break;
                }

                match attribute_header.attribute_type() {
                    0x10 => {
                        let start_stdinfo = stream.get_byte_offset()?;
                        let standard_information = stream.read::<StandardInformation>()?;
                        attributes.push((
                            start_stdinfo,
                            attribute_header,
                            MftAttribute::StandardInformation(standard_information),
                        ));
                        duplicate_attribute_map.push(0x10);
                    }
                    0x30 => {
                        let start_filename = stream.get_byte_offset()?;
                        let file_name = stream.read::<FileName>()?;
                        attributes.push((
                            start_filename,
                            attribute_header,
                            MftAttribute::FileName(file_name),
                        ));
                        duplicate_attribute_map.push(0x30);
                    }
                    0x80 => {
                        if ignore_data_attribute {
                            break;
                        }
                        let start_data = stream.get_byte_offset()?;
                        let datarun = stream.read::<DataRun>()?;
                        attributes.push((
                            start_data,
                            attribute_header,
                            MftAttribute::Data(datarun),
                        ));
                    }
                    _ => {
                        break;
                    }
                }
                attribute_offset += length as u64;
            }
        }
        b"BAAD" => {
            todo!("BAAD")
        }
        _ => {
            // println!("Unknown signature: {:#?}",stream.get_reader().stream_position());
            return Ok(None);
        }
    }
    Ok(Some(MftFileRecord {
        file_descriptor: mft_file_descriptor,
        attributes,
    }))
}

// https://sabercomlogica.com/en/ntfs-resident-and-no-named-attributes/
fn parse_mft(path: &Path, mft_lba: u64, mft_record_size: u32) -> io::Result<Vec<MftFileRecord>> {
    let starting_offset = mft_lba * SECTOR_SIZE as u64;
    let mut offset = starting_offset;

    // The record in the MFT that describes the MFT
    let mft = parse_mft_file_record(path, starting_offset, mft_record_size as usize, false)?
        .expect("Expected MFT file record to exist.");
    offset += 1024;
    let mft_data_attribute = mft
        .attributes
        .iter()
        .find(|(_, _, attribute)| match attribute {
            MftAttribute::Data(_) => true,
            _ => false,
        })
        .map(|(_, header, _)| header.clone());

    let mut records: Vec<MftFileRecord> = Vec::new();
    records.push(mft);
    match mft_data_attribute {
        Some(record) => {
            let allocation_size = record
                .file_allocation_size()
                .expect("No file allocation size.");
            while offset < starting_offset + allocation_size {
                match parse_mft_file_record(path, offset, mft_record_size as usize, true)? {
                    Some(record) => {
                        records.push(record);
                    }
                    None => {}
                }
                offset += mft_record_size as u64;
            }
        }
        None => {
            eprintln!("Expected MFT file record to have a $DATA attribute.")
        }
    }
    Ok(records)
}

// There are more than 8 possible timestamps in MFT Records 
// https://dfir.ru/2021/01/10/standard_information-vs-file_name/
pub fn mft_to_csv(records: Vec<MftFileRecord>, file_name: &str) -> Result<(), csv::Error> {
    let mut writer = Writer::from_path(Path::new(file_name))?;
    let header = [
        "File Name",
        "File Size",
        "File Size on Disk",
        "$FN Modified",
        "$FN MFT Modified",
        "$FN Created",
        "$FN Read",
        "$SI Modified",
        "$SI MFT Modified",
        "$SI Created",
        "$SI Read",
    ];
    writer.write_record(header)?;

    for mut record in records {
        record
            .attributes
            .sort_by(|(_, header1, _), (_, header2, _)| {
                header1.attribute_type().cmp(&header2.attribute_type())
            });
        record.attributes.reverse();
        let mut tmp: [String; 11] = [
            "No Name".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
            "Unknown".into(),
        ];
        for (_attribute_offset, _, attribute) in record.attributes {
            match attribute {
                MftAttribute::StandardInformation(stdinfo) => {
                    tmp[7] = stdinfo.datetime_file_modification.datetime.to_string();
                    tmp[8] = stdinfo.datetime_mft_modification.datetime.to_string();
                    tmp[9] = stdinfo.datetime_file_creation.datetime.to_string();
                    tmp[10] = stdinfo.datetime_file_reading.datetime.to_string();
                },
                MftAttribute::FileName(filename) => {
                    tmp[0] = filename.name;
                    tmp[1] = format!("{}", filename.real_file_size);
                    tmp[2] = format!("{}", filename.file_size_allocated_on_disk);
                    tmp[3] = filename.datetime_file_modification.datetime.to_string();
                    tmp[4] = filename.datetime_mft_modification.datetime.to_string();
                    tmp[5] = filename.datetime_file_creation.datetime.to_string();
                    tmp[6] = filename.datetime_file_reading.datetime.to_string();
                },
                MftAttribute::Data(_) => {},
            }
        }
        writer.write_record(tmp)?;
    }

    writer.flush()?;
    Ok(())
}

pub fn display_mft(records: Vec<MftFileRecord>) {
    println!("# of Records: {}", records.len());
    for mut record in records {
        record
            .attributes
            .sort_by(|(_, header1, _), (_, header2, _)| {
                header1.attribute_type().cmp(&header2.attribute_type())
            });
        record.attributes.reverse();
        for (attribute_offset, _, attribute) in record.attributes {
            match attribute {
                MftAttribute::StandardInformation(stdinfo) => {
                    let mut table = Table::new();
                    table.add_row(row![
                        "$SI Byte Offset",
                        "$SI Modified",
                        "$SI MFT Modified",
                        "$SI Created",
                        "$SI Read",
                    ]);
                    table.add_row(row![
                        attribute_offset,
                        stdinfo.datetime_file_modification.datetime,
                        stdinfo.datetime_mft_modification.datetime,
                        stdinfo.datetime_file_creation.datetime,
                        stdinfo.datetime_file_reading.datetime,
                    ]);
                    table.printstd();
                }
                MftAttribute::FileName(file_name) => {
                    let mut table = Table::new();
                    table.add_row(row![
                        "File Name",
                        // "Byte offset",
                        "$FN Modified",
                        "$FN MFT Modified",
                        "$FN Created",
                        "$FN Read",
                        // "File Size",
                        // "File Size on Disk",
                    ]);
                    table.add_row(row![
                        file_name.name,
                        // attribute_offset,
                        file_name.datetime_file_modification.datetime,
                        file_name.datetime_mft_modification.datetime,
                        file_name.datetime_file_creation.datetime,
                        file_name.datetime_file_reading.datetime,
                        // file_name.real_file_size,
                        // file_name.file_size_allocated_on_disk,
                    ]);
                    table.printstd();
                }
                MftAttribute::Data(_) => {}
            }
        }
        println!();
    }
}

pub fn timestomp_mft(path: &Path, records: Vec<MftFileRecord>, timestomp: Timestomp) {
    let Timestomp::Timestomp {
        file_name,
        timestamp,
    } = timestomp;

    let records_with_file_name: Vec<MftFileRecord> = records
        .into_iter()
        .filter(|attrib| attrib.has_file_name_attribute(&file_name))
        .collect();
    if records_with_file_name.len() > 0 {
        let record = &records_with_file_name[0];
        let fn_attrib = record
            .attributes
            .iter()
            .find(|(_, header, _)| header.attribute_type() == 0x30)
            .unwrap();
        let stdinfo_attrib = record
            .attributes
            .iter()
            .find(|(_, header, _)| header.attribute_type() == 0x10)
            .unwrap();
        println!(
            "Timestomp $FN `{}` at {} with {}",
            file_name, fn_attrib.0, timestamp
        );
        println!(
            "Timestomp $SI `{}` at {} with {}",
            file_name, stdinfo_attrib.0, timestamp
        );
        let mut file = OpenOptions::new().write(true).open(path).unwrap();
        for i in 0..4 {
            file.seek(io::SeekFrom::Start(stdinfo_attrib.0 + (i * 8)))
                .unwrap();
            file.write_uint::<LittleEndian>(NtfsDatetime::from(timestamp).ole2(), 8)
                .unwrap();

            file.seek(io::SeekFrom::Start(fn_attrib.0 + 8 + (i * 8)))
                .unwrap();
            file.write_uint::<LittleEndian>(NtfsDatetime::from(timestamp).ole2(), 8)
                .unwrap();
        }
    } else {
        println!("No MFT record with name `{}`", file_name);
    }
}

#[test]
fn datarun_test() {
    // let datarun: [u8; 8] = [0x21, 0x18, 0x34, 0x56, 0x00, 0x00, 0x00, 0x00];
    // let datarun: [u8; 8] = [0x31, 0x01, 0x41, 0x00, 0x01, 0x00, 0x00, 0x00];
    let datarun: [u8; 8] = [128, 0x0, 0x0, 0x0, 80, 0x00, 0x00, 0x00];
    //356864000
    //356929536
    let mut length: u64 = 0;
    let mut offset: i64 = 0;

    let high_nibble = (datarun[0] & 0b11110000) >> 4;
    let low_nibble = datarun[0] & 0b00001111;

    for i in 0..low_nibble as usize {
        length |= (datarun[1 + i] as u64) << (i * 8);
    }

    for i in 0..high_nibble as usize {
        offset |= (datarun[1 + low_nibble as usize + i] as i64) << (i * 8);
    }

    println!("Here: {:#04x}", offset & (1 << (high_nibble * 8 - 1)));
    if offset & (1 << (high_nibble * 8 - 1)) > 0 {
        for i in 0..high_nibble as usize {
            offset |= (0xFF as i64) << (i * 8);
        }
    }

    println!("Length: {:#04x}", length);
    println!("Offset: {:#04x}", offset);
}

#[test]
fn test_stomp() {
    let bytes = [0x00, 0x48, 0xbc, 0xf4, 0xe0, 0x1d, 0xd9, 0x01];
    let num = u64::from_le_bytes(bytes);
    let timestamp_unix_epoch = (num - 116444736000000000) / 10000000;
    let datetime = DateTime::<Local>::from(UNIX_EPOCH + Duration::from_secs(timestamp_unix_epoch));
    println!("DateTime: {:#?}", datetime);
}


#[test]
fn test_erroring_file_record() {
    let record = parse_mft_file_record(Path::new("\\\\.\\PhysicalDrive0"), 20142478336, 1024, true).unwrap();
    println!("Record: {:#?}", record);
}
