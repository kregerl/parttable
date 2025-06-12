use std::{cell::Cell, string::FromUtf16Error};

use binary_struct::{prelude::*, BinaryStruct, BinaryType, Skip};
use bitmask_enum::bitmask;
use eframe::glow::Buffer;

use crate::mapped_disk::{BufferedMappedDisk, MappedDisk, MappedDiskError, MappedDiskResult};

use super::pbr::{validate_pbr, NtfsPartitionBootRecord};

macro_rules! impl_binary_bitmask_parse {
    // General implementation for all unsigned and signed integer types.
    // The enum type and inner bitmask type must be known
    ($real_type:ty, $inner_type:ty, $from_types:expr) => {
        impl BinaryParse for $real_type {
            fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError>
            where
                Self: Sized,
            {
                let flags = $from_types(input[..Self::SIZE].try_into()?);
                Ok(Self::from(flags))
            }
        }

        impl BinarySize for $real_type {
            const SIZE: usize = ::core::mem::size_of::<$inner_type>();
        }
    };

    // Used for bitmasks of u8s only
    ($real_type:ty, $inner_type:ty) => {
        impl BinarySize for $real_type {
            const SIZE: usize = ::core::mem::size_of::<Self>();
        }

        impl BinaryParse for $real_type {
            fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError> {
                Ok(Self::from(input[0]))
            }
        }
    };
}

#[derive(BinaryStruct, Debug)]
struct MftFileRecord {
    #[binary_struct(num_bytes = 4)]
    signature: String,
    offest_of_update_seq: u16,
    // Size of the update sequence in WORDS
    size_of_update_seq: u16,
    log_file_seq_number: u64,
    use_count: u8,
    deletion_count: u8,
    hard_link_count: u16,
    offset_first_attribute: u16,
    flags: MftFileRecordFlags,
    file_size_on_disk: u32,
    space_allocated: u32,
    base_register: u64,
    next_attribute_id: u16,
}

#[bitmask(u16)]
pub enum MftFileRecordFlags {
    RecordInUse = 0x01,
    RecordIsDir = 0x02,
    RecordIsExtension = 0x04,
    SpecialIndexPresent = 0x08,
}
impl_binary_bitmask_parse!(MftFileRecordFlags, u16, u16::from_le_bytes);

#[derive(BinaryStruct, Debug)]
pub struct CommonAttributeHeader {
    attribute_type: u32,
    length: u32,
    non_resident_flag: u8,
    name_length: u8,
    name_offset: u16,
    flags: AttributeHeaderFlags,
    attribute_id: u16,
}

impl CommonAttributeHeader {
    pub fn is_resident(&self) -> bool {
        self.non_resident_flag == 0
    }
}

#[bitmask(u16)]
pub enum AttributeHeaderFlags {
    Compressed = 0x0001,
    Encrypted = 0x4000,
    Sparse = 0x8000,
}
impl_binary_bitmask_parse!(AttributeHeaderFlags, u16, u16::from_le_bytes);

#[derive(BinaryStruct, Debug)]
pub struct ResidentAttributeHeader {
    attribute_length: u32,
    attritube_offset: u16,
    indexed_flag: u8,
    _padding: Skip<1>,
}

#[derive(BinaryStruct, Debug)]
pub struct NonResidentAttributeHeader {
    starting_vcn: u64,
    ending_vcn: u64,
    data_runs_offset: u16,
    compression_unit_size: u16,
    _padding: Skip<4>,
    file_allocation_size: u64,
    file_real_size: u64,
    initial_stream_size: u64,
}

#[derive(Debug)]
pub enum AttributeHeader {
    ResidentUnnamed {
        common: CommonAttributeHeader,
        resident: ResidentAttributeHeader,
    },
    ResidentNamed {
        common: CommonAttributeHeader,
        resident: ResidentAttributeHeader,
        name: String,
    },
    NonResidentUnnamed {
        common: CommonAttributeHeader,
        non_resident: NonResidentAttributeHeader,
    },
    NonResidentNamed {
        common: CommonAttributeHeader,
        non_resident: NonResidentAttributeHeader,
        name: String,
    },
}

impl AttributeHeader {
    pub fn attribute_type(&self) -> u32 {
        match self {
            AttributeHeader::ResidentUnnamed { common, .. } => common.attribute_type,
            AttributeHeader::ResidentNamed { common, .. } => common.attribute_type,
            AttributeHeader::NonResidentUnnamed { common, .. } => common.attribute_type,
            AttributeHeader::NonResidentNamed { common, .. } => common.attribute_type,
        }
    }

    pub fn length(&self) -> u32 {
        match self {
            AttributeHeader::ResidentUnnamed { common, .. } => common.length,
            AttributeHeader::ResidentNamed { common, .. } => common.length,
            AttributeHeader::NonResidentUnnamed { common, .. } => common.length,
            AttributeHeader::NonResidentNamed { common, .. } => common.length,
        }
    }

    pub fn name_offset(&self) -> u16 {
        match self {
            AttributeHeader::ResidentUnnamed { common, .. } => common.name_offset,
            AttributeHeader::ResidentNamed { common, .. } => common.name_offset,
            AttributeHeader::NonResidentUnnamed { common, .. } => common.name_offset,
            AttributeHeader::NonResidentNamed { common, .. } => common.name_offset,
        }
    }

    /// Returns the attribute length of a resident attribute
    /// Returns 0 for non-resident attributes
    pub fn resident_attribute_value_length(&self) -> u32 {
        match self {
            AttributeHeader::ResidentUnnamed { resident, .. } => resident.attribute_length,
            AttributeHeader::ResidentNamed { resident, .. } => resident.attribute_length,
            _ => 0,
        }
    }

    pub fn resident_attribute_offset(&self) -> Option<u16> {
        match self {
            AttributeHeader::ResidentUnnamed { resident, .. } => Some(resident.attritube_offset),
            AttributeHeader::ResidentNamed { resident, .. } => Some(resident.attritube_offset),
            _ => None,
        }
    }

    pub fn total_attribute_length(&self) -> usize {
        (match self {
            AttributeHeader::ResidentUnnamed { common, .. } => common.length,
            AttributeHeader::ResidentNamed { common, .. } => common.length,
            AttributeHeader::NonResidentUnnamed { common, .. } => common.length,
            AttributeHeader::NonResidentNamed { common, .. } => common.length,
        }) as usize
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            AttributeHeader::ResidentNamed { name, .. } => Some(name),
            AttributeHeader::NonResidentNamed { name, .. } => Some(name),
            _ => None,
        }
    }
}

fn parse_attribute_header(disk: &BufferedMappedDisk) -> MappedDiskResult<AttributeHeader> {
    let attribute_header = disk.read::<CommonAttributeHeader>()?;
    if attribute_header.is_resident() {
        let resident_attribute_header = disk.read::<ResidentAttributeHeader>()?;
        if attribute_header.name_length > 0 {
            let name = parse_attribute_name(&disk, &attribute_header)?;
            Ok(AttributeHeader::ResidentNamed {
                common: attribute_header,
                resident: resident_attribute_header,
                name,
            })
        } else {
            Ok(AttributeHeader::ResidentUnnamed {
                common: attribute_header,
                resident: resident_attribute_header,
            })
        }
    } else {
        let non_resident_attribute_header = disk.read::<NonResidentAttributeHeader>()?;
        if attribute_header.name_length > 0 {
            let name = parse_attribute_name(&disk, &attribute_header)?;
            Ok(AttributeHeader::NonResidentNamed {
                common: attribute_header,
                non_resident: non_resident_attribute_header,
                name,
            })
        } else {
            Ok(AttributeHeader::NonResidentUnnamed {
                common: attribute_header,
                non_resident: non_resident_attribute_header,
            })
        }
    }
}

#[derive(Debug)]
pub enum Attribute<'a> {
    StandardInfomation(StandardInformation),
    FileName(FileName),
    Data(Vec<DataRun>),
    BitMap(Vec<DataRun>),
    SecurityDescriptor(SecurityDescriptor),
    VolumeName(String),
    VolumeInformation(VolumeInformation),
    // IndexRoot
    IndexAllocation(Vec<DataRun>),
    EAInformation(ExtendedAttributeInformation),
    ExtendedAttributes(Vec<ExtendedAttribute<'a>>),
}

#[derive(BinaryStruct, Debug)]
pub struct StandardInformation {
    date_time_file_creation: u64,
    date_time_file_modification: u64,
    date_time_mft_modification: u64,
    date_time_file_reading: u64,
    // 0x0001	Read-Only
    // 0x0002	Hidden
    // 0x0004	System
    // 0x0020	Archive
    // 0x0040	Device (reserved)
    // 0x0080	Normal
    // 0x0100	Temporary
    // 0x0200	Sparse File
    // 0x0400	Reparse Point
    // 0x0800	Compressed
    // 0x1000	Offline
    // 0x2000	Not Content Indexed
    // 0x4000	Encrypted
    file_permissions: u32,
    max_number_of_versions: u32,
    version_number: u64,
}

// #[derive(BinaryStruct, Debug)]
// pub struct NtfsV3StandardInformation {
//     standard_information: StandardInformation,
//     owner_id: u32,
//     security_descriptor_id: u32,
//     quota_changed: u64,
//     update_sequence_number: u64,
// }

#[derive(BinaryStruct, Debug)]
struct FileName {
    reference_to_parent_dir: u64,
    datetime_file_creation: u64,
    datetime_file_modification: u64,
    datetime_mft_modification: u64,
    datetime_file_reading: u64,
    file_size_allocated_on_disk: u64,
    real_file_size: u64,
    flags: FileNameFlags,
    extended_attributes_and_reparse: u32,
    name_size: u8,
    namespace: u8,
    #[binary_struct(num_bytes_from = "name_size", encoding = "utf16")]
    name: String,
}

#[bitmask(u32)]
pub enum FileNameFlags {
    ReadOnly = 0x0001,
    Hidden = 0x0002,
    System = 0x0004,
    Archive = 0x0020,
    Device = 0x0040,
    Normal = 0x0080,
    Temporary = 0x0100,
    SparseFile = 0x0200,
    ReparsePoint = 0x0400,
    Compressed = 0x0800,
    Offline = 0x1000,
    NotContentIndexed = 0x2000,
    Encrypted = 0x4000,
    // copy from corresponding bit in MFT record
    Directory = 0x10000000,
    // copy from corresponding bit in MFT record
    IndexView = 0x20000000,
}
impl_binary_bitmask_parse!(FileNameFlags, u32, u32::from_le_bytes);

#[derive(Debug)]
struct DataRun {
    length: u64,
    offset: i64,
}

fn parse_dataruns(disk: &BufferedMappedDisk) -> MappedDiskResult<Vec<DataRun>> {
    let mut dataruns = Vec::new();
    while disk.peek::<u8>()? != 0 {
        let datarun_header_byte = disk.read::<u8>()?;
        // Length is a VCN
        // i.e byte offset = length * sectors/cluster * bytes/sector
        let mut length = 0u64;
        // Offset is defined as number of clusters
        // i.e byte offset = offset * sectors/cluster * bytes/sector
        let mut offset = 0i64;

        let high_nibble = datarun_header_byte >> 4;
        let low_nibble = datarun_header_byte & 0b00001111;

        for i in 0..low_nibble as usize {
            length |= (disk.read::<u8>()? as u64) << (i * 8);
        }

        for i in 0..high_nibble as usize {
            offset |= (disk.read::<u8>()? as i64) << (i * 8);
        }

        // Sign-extend the offset since it can be negative
        // Sign-extending means copying the sign bit of the unextended value
        // to all bits on the left side of the larger-size value.
        if high_nibble > 0 && (offset & (1 << (high_nibble * 8 - 1))) != 0 {
            let mask = !0 << (high_nibble * 8);
            offset |= mask;
        }

        dataruns.push(DataRun { length, offset });
    }

    Ok(dataruns)
}

#[bitmask(u16)]
enum ControlFlags {
    OwnerDefaulted = 0x0001,
    GroupDefaulted = 0x0002,
    DACLPresent = 0x0004,
    DACLDefaulted = 0x0008,
    SACLPresent = 0x0010,
    SACLDefaulted = 0x0020,
    DACLAutoInheritReq = 0x0100,
    SACLAutoInheritReq = 0x0200,
    DACLAutoInherited = 0x0400,
    SACLAutoInherited = 0x0800,
    DACLProtected = 0x1000,
    SACLProtected = 0x2000,
    RMControlValid = 0x4000,
    SelfRelative = 0x8000,
}
impl_binary_bitmask_parse!(ControlFlags, u16, u16::from_le_bytes);

#[bitmask(u8)]
enum AceFlags {
    ObjectInheritsACE = 0x01,
    ContainerInheritsACE = 0x02,
    DoNotPropagateInheritedACE = 0x04,
    InheritOnlyACE = 0x08,
    AuditOnSuccess = 0x40,
    AuditOnFailure = 0x80,
}

impl_binary_bitmask_parse!(AceFlags, u8);

// impl BinaryParse for AceFlags {
//     fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError>
//     where
//         Self: Sized,
//     {
//         Ok(Self::from(input[0]))
//     }
// }

// impl BinarySize for AceFlags {
//     const SIZE: usize = std::mem::size_of::<u8>();
// }

#[derive(BinaryStruct, Debug)]
struct SecurityDescriptorHeader {
    revision: u8,
    _padding: Skip<1>,
    control_flags: ControlFlags,
    // The 4 offsets below are relative to the start of this header
    user_sid_offset: u32,
    group_sid_offset: u32,
    sacl_offset: u32,
    dacl_offset: u32,
}

#[derive(BinaryStruct, Debug)]
struct AccessControlList {
    revision: u8,
    _padding: Skip<1>,
    acl_size: u16,
    ace_count: u16,
    _padding2: Skip<2>,
    #[binary_struct(ignore = true)]
    access_control_entries: Vec<AccessControlEntryWithSID>,
}

#[derive(Debug)]
struct AccessControlEntryWithSID {
    entry: AccessControlEntry,
    sid: SecurityIdentifier,
}

#[derive(BinaryStruct, Debug)]
struct AccessControlEntry {
    // Can be one of 3 types
    // - 0x00 Access Allowed
    // - 0x01 Access Denied
    // - 0x02 System Audit
    typ: u8,
    flags: AceFlags,
    size: u16,
    access_mask: [u8; 4],
}

#[derive(Debug)]
struct SecurityIdentifier(String);

impl BinaryParse for SecurityIdentifier {
    fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError>
    where
        Self: Sized,
    {
        let revision = input[0];
        let sub_authority_count = input[1];
        // 6 Bytes read as big endian
        // Need to pad the first two bytes with 0 so it can be read as a u64.
        let nt_authority = {
            let mut tmp = Vec::from([0u8, 0u8]);
            tmp.extend(&input[2..8]);
            u64::from_be_bytes(tmp[0..8].try_into()?)
        };
        // Read sub authorities into a vec, each sub authority is 4 bytes
        let mut sub_authorities = Vec::new();
        for i in 0..sub_authority_count {
            let current_iteration_offset = i as usize * 4;
            sub_authorities.push(u32::from_le_bytes(
                input[(8 + current_iteration_offset)..(12 + current_iteration_offset)]
                    .try_into()?,
            ))
        }
        // Format bytes into the Windows SID format
        let mut id = format!("S-{0}-{1}", revision, nt_authority);
        for sub_authority in sub_authorities {
            id.push('-');
            id.push_str(sub_authority.to_string().as_str());
        }

        Ok(Self(id))
    }
}

impl SecurityIdentifier {
    pub fn calculate_size(sub_authority_count: u8) -> usize {
        // revision(1 byte) + sub authority count(1 byte) + identifier authority(6 bytes) + sub authorities (4 bytes each)
        1 + 1 + 6 + (sub_authority_count as usize * 4)
    }
}

pub fn interpret_bytes_as_utf16(name_bytes: &[u8]) -> Result<String, FromUtf16Error> {
    let num_bytes = name_bytes.len();
    let mut unicode_symbols: Vec<u16> = Vec::with_capacity(num_bytes / 2);
    for index in (0..num_bytes).step_by(2) {
        // Order of top and bottom here is reversed since the bytes are in little endian
        let first = name_bytes[index];
        let second = name_bytes[index + 1];
        unicode_symbols.push(((second as u16) << 8) | first as u16);
    }
    String::from_utf16(&unicode_symbols)
}

fn parse_attribute_name(
    disk: &BufferedMappedDisk,
    attribute_header: &CommonAttributeHeader,
) -> MappedDiskResult<String> {
    let name_bytes = disk.read_bytes(attribute_header.name_length as usize * 2usize)?;
    let name =
        interpret_bytes_as_utf16(&name_bytes).expect("Invalid utf16 bytes in attribute header.");
    Ok(name)
}

fn parse_sid(
    disk: &BufferedMappedDisk,
    ace_opt: Option<&AccessControlEntry>,
) -> MappedDiskResult<SecurityIdentifier> {
    match ace_opt {
        Some(ace) => {
            disk.read_with_size::<SecurityIdentifier>(ace.size as usize - AccessControlEntry::SIZE)
        }
        None => {
            let first_two_bytes_sid = disk.peek::<[u8; 2]>()?;
            let sub_authority_count = first_two_bytes_sid[1];
            disk.read_with_size::<SecurityIdentifier>(SecurityIdentifier::calculate_size(
                sub_authority_count,
            ))
        }
    }
}

fn parse_access_control_list(disk: &BufferedMappedDisk) -> MappedDiskResult<AccessControlList> {
    let mut acl = disk.read::<AccessControlList>()?;
    for _ in 0..acl.ace_count {
        let ace = disk.read::<AccessControlEntry>()?;
        let sid = parse_sid(disk, Some(&ace))?;
        acl.access_control_entries
            .push(AccessControlEntryWithSID { entry: ace, sid });
    }

    Ok(acl)
}

#[derive(Debug)]
struct SecurityDescriptor {
    header: SecurityDescriptorHeader,
    user_sid: SecurityIdentifier,
    group_sid: SecurityIdentifier,
    dacl: Option<AccessControlList>,
    sacl: Option<AccessControlList>,
}

#[derive(BinaryStruct, Debug)]
struct VolumeInformation {
    _padding: Skip<8>,
    major_version: u8,
    minor_version: u8,
    flags: VolumeInformationFlags,
    _padding2: Skip<4>,
}

#[bitmask(u16)]
enum VolumeInformationFlags {
    Dirty = 0x0001,
    ResizeLogFile = 0x0002,
    UpgradeOnMount = 0x0004,
    MountedOnNT4 = 0x0008,
    DeleteUSNUnderway = 0x0010,
    RepairObjectIds = 0x0020,
    ModifiedByChkdsk = 0x8000,
}
impl_binary_bitmask_parse!(VolumeInformationFlags, u16, u16::from_le_bytes);

#[derive(BinaryStruct, Debug)]
struct IndexRootHeader {
    attribute_type: u32,
    collation_rule: u32,
    index_block_size: u32,
    clusters_per_index_block: u8,
    _padding: Skip<3>,
}

#[derive(BinaryStruct, Debug)]
struct IndexNodeHeader {
    entry_offset: u32,
    total_entry_size: u32,
    allocated_entry_size: u32,
    // has sub-nodes
    non_leaf_node_flag: u8,
    _padding: Skip<3>,
}

#[derive(BinaryStruct, Debug)]
struct IndexEntryHeader {
    file_reference: u64,
    index_entry_length: u16,
    stream_length: u16,
    flags: IndexEntryFlags,
    _padding: Skip<3>,
}

#[bitmask(u8)]
enum IndexEntryFlags {
    SubNode = 0x01,
    LastIndexEntry = 0x02,
}
impl_binary_bitmask_parse!(IndexEntryFlags, u8);

#[derive(BinaryStruct, Debug)]
struct SdhIndexKey {
    hash_key: u32,
    security_id_key: u32,
}

#[derive(BinaryStruct, Debug)]
struct SdhIndexValue {
    hash_key: u32,
    security_id_key: u32,
    // Offest to the security descriptor in the $SDS
    security_descriptor_offset: u64,
    // Size of the security descriptor in the $SDS
    security_descriptor_size: u32,
    #[binary_struct(num_bytes = 4)]
    padding: String,
}

#[derive(BinaryStruct, Debug)]
struct SiiIndexKey {
    security_id: u32,
    // offset: u64,
}

#[derive(BinaryStruct, Debug)]
struct SiiIndexValue {
    hash: u32,
    security_id: u32,
    // Offest to the security descriptor in the $SDS
    security_descriptor_offset: u64,
    // Size of the security descriptor in the $SDS
    security_descriptor_size: u32,
}

#[derive(BinaryStruct, Debug)]
struct QuotaIndexValue {
    version: u32,
    flags: QuotaFlags,
    bytes_used: u64,
    change_time: u64,
    warning_limit: u64,
    hard_limit: u64,
    exceeded_time: u64,
}

#[bitmask(u32)]
enum QuotaFlags {
    DefaultLimits = 0x0001,
    LimitReached = 0x0002,
    IdDeleted = 0x0004,
    TrackingEnabled = 0x0010,
    EnforcementEnabled = 0x0020,
    TrackingRequested = 0x0040,
    LogThreshold = 0x0080,
    LogLimit = 0x0100,
    OutOfDate = 0x0200,
    Corrupt = 0x0400,
    PendingDeletes = 0x0800,
}
impl_binary_bitmask_parse!(QuotaFlags, u32, u32::from_le_bytes);

#[derive(BinaryStruct, Debug)]
struct ExtendedAttributeInformation {
    extended_attributes_packed_size: u16,
    extended_attributes_num: u16,
    extended_attributes_unpacked_size: u32,
}

#[derive(BinaryStruct, Debug)]
struct ExtendedAttributeHeader {
    next_ea_offset: u32,
    flags: ExtendedAttributeFlags,
    name_length: u8,
    value_length: u16,
}

#[bitmask(u8)]
pub enum ExtendedAttributeFlags {
    NeedEA = 0x80,
}
impl_binary_bitmask_parse!(ExtendedAttributeFlags, u8);

#[derive(Debug)]
struct ExtendedAttribute<'a> {
    name: String,
    value: &'a [u8],
}

pub fn parse_attribute<'a>(
    disk: &'a BufferedMappedDisk,
) -> MappedDiskResult<Option<Attribute<'a>>> {
    let offset_of_attribute_header = disk.current_offset();
    let attribute_header = parse_attribute_header(disk)?;
    // Sometimes resident attributes don't take up the full attribute length when they are named
    // In the example below, the name offset tells us where the name is located,
    // which is 2*name_length bytes long. If we read the 24 bytes from the common and resident
    // header and also the name, then we have read 28 bytes. Since the attribute is located at a 32 byte
    // offset, we need to jump there.
    // ResidentNamed {
    //     common: CommonAttributeHeader {
    //         attribute_type: 144,
    //         length: 120,
    //         non_resident_flag: 0,
    //         name_length: 2,
    //         name_offset: 24,
    //         flags: [
    //             0,
    //             0,
    //         ],
    //         attribute_id: 3,
    //     },
    //     resident: ResidentAttributeHeader {
    //         attribute_length: 88,
    //         attritube_offset: 32,
    //         indexed_flag: 0,
    //         _padding: Skip,
    //     },
    //     name: "$O",
    // }
    if let Some(attribute_offset) = attribute_header.resident_attribute_offset() {
        disk.set_cursor(offset_of_attribute_header + attribute_offset as usize)?;
    }
    println!("attribute_header: {:#?}", attribute_header);
    match attribute_header.attribute_type() {
        0x10 => {
            let std_info = disk.read::<StandardInformation>()?;
            println!(
                "Standard info: {:#?} at {}",
                std_info,
                disk.current_offset()
            );
            // In NTFS 3.0+ the standard information attribute has 24 bytes of extra fields
            // - Owner Identifier (4 bytes)
            // - Security Descriptor Identifier (4 bytes)
            // - Quota Changed (8 bytes)
            // - Update Sequence Number (8 bytes)
            // These bytes need to be skipped so the cursor is aligned with the next attribute.
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::StandardInfomation(std_info)))
        }
        0x20 => {
            // $ATTRIBUTE_LIST
            todo!("$ATTRIBUTE_LIST")
        }
        0x30 => {
            let file_name = disk.read_with_size::<FileName>(
                attribute_header.resident_attribute_value_length() as usize,
            )?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::FileName(file_name)))
        }
        0x40 => {
            // $OBJECT_ID
            todo!("$OBJECT_ID")
        }
        0x50 => {
            // $SECURITY_DESCRIPTOR
            let starting_offset_of_security_descriptor = disk.current_offset();
            let security_descriptor_header = disk.read::<SecurityDescriptorHeader>()?;

            let dacl = if security_descriptor_header
                .control_flags
                .contains(ControlFlags::DACLPresent)
            {
                disk.set_cursor(
                    starting_offset_of_security_descriptor
                        + security_descriptor_header.dacl_offset as usize,
                )?;
                Some(parse_access_control_list(disk)?)
            } else {
                None
            };

            let sacl = if security_descriptor_header
                .control_flags
                .contains(ControlFlags::SACLPresent)
            {
                disk.set_cursor(
                    starting_offset_of_security_descriptor
                        + security_descriptor_header.sacl_offset as usize,
                )?;
                Some(parse_access_control_list(disk)?)
            } else {
                None
            };
            // Jump to where the user SID is
            // Offset is relative to start of SecurityDescriptorHeader
            disk.set_cursor(
                starting_offset_of_security_descriptor
                    + security_descriptor_header.user_sid_offset as usize,
            )?;
            let user_sid = parse_sid(disk, None)?;

            // Jump to where the group SID is
            // Offset is relative to start of SecurityDescriptorHeader
            disk.set_cursor(
                starting_offset_of_security_descriptor
                    + security_descriptor_header.group_sid_offset as usize,
            )?;
            let group_sid = parse_sid(disk, None)?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;

            Ok(Some(Attribute::SecurityDescriptor(SecurityDescriptor {
                header: security_descriptor_header,
                user_sid,
                group_sid,
                dacl,
                sacl,
            })))
        }
        0x60 => {
            // $VOLUME_NAME
            let name_length = attribute_header.resident_attribute_value_length();
            let volume_name = if name_length != 0 {
                let name_bytes = disk.read_bytes(name_length as usize)?;
                // FIXME: Do not unwrap here
                interpret_bytes_as_utf16(name_bytes).unwrap()
            } else {
                String::new()
            };
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::VolumeName(volume_name)))
        }
        0x70 => {
            // $VOLUME_INFORMATION
            let volume_information = disk.read::<VolumeInformation>()?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::VolumeInformation(volume_information)))
        }
        0x80 => {
            // $DATA
            let dataruns = parse_dataruns(disk)?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::Data(dataruns)))
        }
        0x90 => {
            // $INDEX_ROOT
            println!("Offest: {}", disk.current_offset());
            let index_root_header = disk.read::<IndexRootHeader>()?;
            println!("index_root_header: {:#?}", index_root_header);
            let index_header = disk.read::<IndexNodeHeader>()?;
            println!("index_header: {:#?}", index_header);
            let mut offset = disk.current_offset();
            loop {
                println!("Offest: {}", disk.current_offset());
                let index_entry_header = disk.read::<IndexEntryHeader>()?;
                // TODO: Sub nodes
                println!("index_entry_header: {:#?}", index_entry_header);
                if index_entry_header
                    .flags
                    .contains(IndexEntryFlags::LastIndexEntry)
                {
                    break;
                }
                if attribute_header.name().is_none() {
                    panic!("Attribute name is none");
                }

                let attribute_name = attribute_header.name().unwrap();
                match attribute_name {
                    "$SDH" => {
                        println!("disk.current_offset(): {}", disk.current_offset());
                        let sdh_key = disk.read::<SdhIndexKey>()?;
                        let sdh_value = disk.read::<SdhIndexValue>()?;
                        println!("sdh_key: {:#?}", sdh_key);
                        println!("sdh_value: {:#?}", sdh_value);
                        // todo!("SdhIndexEntry: {}", disk.current_offset());
                    }
                    "$SII" => {
                        // todo!("SiiIndexEntry: {}", disk.current_offset());
                        let sii_key = disk.read::<SiiIndexKey>()?;
                        let sii_value = disk.read::<SiiIndexValue>()?;
                        println!("sii_key: {:#?}", sii_key);
                        println!("sii_value: {:#?}", sii_value);
                        // todo!("SiiIndexEntry: {}", disk.current_offset());
                    }
                    "$I30" => {
                        let file_name_attribute = disk.read_with_size::<FileName>(
                            index_entry_header.index_entry_length as usize,
                        )?;
                        println!("file_name_attribute: {:#?}", file_name_attribute);
                    }
                    "$O" => {
                        let sid = parse_sid(disk, None)?;
                        let owner_id = disk.read::<u32>()?;
                        println!("Sid: {:#?}", sid);
                        println!("owner_id: {:#?}", owner_id);
                    }
                    "$Q" => {
                        let quota_owner_id = disk.read::<u32>()?;
                        println!("owner_id: {:#?}", quota_owner_id);
                        let quota_value = disk.read::<QuotaIndexValue>()?;
                        println!("q_value: {:#?}", quota_value);
                        if !quota_value.flags.contains(QuotaFlags::DefaultLimits) {
                            let sid = parse_sid(disk, None)?;
                            println!("sid: {:#?}", sid);
                        }
                    }
                    _ => {
                        todo!(
                            "Unknown or unsupported $INDEX_ROOT name: {} @ {}",
                            attribute_name,
                            disk.current_offset()
                        );
                    }
                }

                offset += index_entry_header.index_entry_length as usize;
                disk.set_cursor(offset)?;
            }
            println!("Offest: {}", disk.current_offset());
            Ok(None)
            // todo!("$INDEX_ROOT")
        }
        0xA0 => {
            // $INDEX_ALLOCATION
            // FIXME: Need to perform "fixups" with update sequqnces when actually reading the non-resident
            // data
            let dataruns = parse_dataruns(disk)?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::IndexAllocation(dataruns)))
        }
        0xB0 => {
            // $BITMAP
            let dataruns = parse_dataruns(disk)?;
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::BitMap(dataruns)))
        }
        0xC0 => {
            // $REPARSE_POINT
            todo!("$REPARSE_POINT")
        }
        0xD0 => {
            // $EA_INFORMATION
            let ea_information = disk.read::<ExtendedAttributeInformation>()?;
            println!("ea_info: {:#?}", ea_information);
            Ok(Some(Attribute::EAInformation(ea_information)))
        }
        0xE0 => {
            // $EA
            let mut extended_attributes: Vec<ExtendedAttribute> = Vec::new();
            let mut offset = disk.current_offset();
            loop {
                // If the next ea offset is 0, exit the loop
                if disk.peek::<u32>()? == 0 {
                    break;
                }
                let extended_attribute = disk.read::<ExtendedAttributeHeader>()?;
                let name = String::from_utf8(
                    disk.read_bytes(extended_attribute.name_length as usize)?
                        .to_vec(),
                )
                .unwrap();
                let value = disk.read_bytes(extended_attribute.value_length as usize)?;

                extended_attributes.push(ExtendedAttribute { name, value });

                offset += extended_attribute.next_ea_offset as usize;
                disk.set_cursor(offset)?;
            }
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::ExtendedAttributes(extended_attributes)))
            // todo!("$EA: {}", disk.current_offset())
        }
        0x100 => {
            // $LOGGED_UTILITY_STREAM
            todo!("$LOGGED_UTILITY_STREAM")
        }
        _ => Ok(None),
    }
}

fn parse_file_attributes(
    disk: &BufferedMappedDisk,
    starting_byte_offset: usize,
) -> MappedDiskResult<()> {
    disk.set_cursor(starting_byte_offset)?;
    while disk.peek::<u32>()? != u32::MAX {
        // println!("starting offset: {}", disk.current_offset());
        let attribute = parse_attribute(&disk)?;
        if let Some(attr) = attribute {
            println!("Attr: {:#?} at {}", attr, disk.current_offset());
        }
        // println!("current offset: {:#?}", disk.current_offset());
    }
    Ok(())
}

pub struct NtfsReader<'a> {
    disk: &'a MappedDisk,
    sector_size: usize,
    sectors_per_cluster: usize,
    starting_lba_of_filesystem: usize,
}

impl<'a> NtfsReader<'a> {
    pub fn new(
        disk: &'a MappedDisk,
        pbr: &NtfsPartitionBootRecord,
        starting_lba_of_filesystem: usize,
    ) -> Self {
        Self {
            disk,
            sector_size: pbr.sector_size() as usize,
            sectors_per_cluster: pbr.sectors_per_cluster() as usize,
            starting_lba_of_filesystem: starting_lba_of_filesystem,
        }
    }

    // pub fn starting_offset_of_filesystem(&self) -> usize {
    //     self.byte_offset_from_lba(self.starting_lba_of_filesystem)
    // }

    pub fn byte_offset_from_lba(&self, lba: usize) -> usize {
        lba * self.sector_size
    }

    // pub fn set_cursor_from_lba(&self, lba: usize) -> MappedDiskResult<()> {
    //     self.disk.set_cursor(self.byte_offset_from_lba(lba))
    // }

    pub fn byte_offset_from_lcn(&self, lcn: usize) -> usize {
        lcn * self.sector_size * self.sectors_per_cluster
    }
}

pub fn patch_update_sequence(
    disk: &mut BufferedMappedDisk,
    sector_size: usize,
    update_sequence_number: u16,
    update_sequence_array: &[u8],
) -> MappedDiskResult<()> {
    let number_of_sectors = disk.buffer_size / sector_size;
    for sector_index in 0..number_of_sectors {
        let offset_to_update_sequence = sector_size * sector_index;
        let buffer_update_sequence_number = u16::from_le_bytes(
            disk.buffer[((sector_size - 2) + offset_to_update_sequence)
                ..(sector_size + offset_to_update_sequence)]
                .try_into()
                .unwrap(),
        );
        if buffer_update_sequence_number == update_sequence_number {
            let update_sequence_array_offset = sector_index * 2;
            let update_sequence = &update_sequence_array
                [update_sequence_array_offset..update_sequence_array_offset + 2];
            for (index, byte) in update_sequence.iter().enumerate() {
                disk.buffer[(510 + offset_to_update_sequence) + index] = *byte;
            }
        }
    }
    Ok(())
}

pub fn parse_mft(
    fs_reader: &mut NtfsReader,
    partition_boot_record: &NtfsPartitionBootRecord,
) -> MappedDiskResult<()> {
    let starting_lba = validate_pbr(
        partition_boot_record,
        fs_reader.starting_lba_of_filesystem as u64,
    )
    .unwrap();
    println!("starting_lba: {}", starting_lba);
    let mut current_offset = fs_reader.byte_offset_from_lba(starting_lba);
    println!(
        "Total size: {}",
        fs_reader.byte_offset_from_lba(partition_boot_record.number_of_sectors_in_volume())
    );
    while current_offset + partition_boot_record.mft_size()
        <= fs_reader.byte_offset_from_lba(partition_boot_record.number_of_sectors_in_volume())
    {
        println!(
            "Current: {}",
            current_offset + partition_boot_record.mft_size()
        );
        fs_reader.disk.set_cursor(current_offset)?;
        let file_descriptor = fs_reader.disk.read::<MftFileRecord>()?;

        if file_descriptor.signature == "FILE" {
            println!("MftFileDescriptor: {:#?}", file_descriptor);
            fs_reader
                .disk
                .set_cursor(current_offset + file_descriptor.offest_of_update_seq as usize)?;
            let update_sequence_number = fs_reader.disk.read::<u16>()?;
            let update_sequence_array = fs_reader
                .disk
                .read_bytes((file_descriptor.size_of_update_seq as usize - 1) * 2)?;

            let mut buffered_mapped_disk =
                BufferedMappedDisk::new(fs_reader.disk, partition_boot_record.mft_size());
            buffered_mapped_disk.fill_buffer_at(current_offset)?;
            patch_update_sequence(
                &mut buffered_mapped_disk,
                partition_boot_record.sector_size() as usize,
                update_sequence_number,
                &update_sequence_array,
            )?;

            parse_file_attributes(
                &buffered_mapped_disk,
                current_offset + file_descriptor.offset_first_attribute as usize,
            )?;
        } else {
            // TODO: Skip
        }
        current_offset += partition_boot_record.mft_size();
    }

    Ok(())
}
