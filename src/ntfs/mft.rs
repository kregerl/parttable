use std::{ops::Index, string::FromUtf16Error};

use binary_struct::{prelude::*, BinaryStruct, Skip};
use bitmask_enum::bitmask;
use eframe::epaint::tessellator::{path, PathType};

use crate::{
    guid::Guid,
    mapped_disk::{BufferedMappedDisk, MappedDisk, MappedDiskResult},
};

use super::pbr::{validate_pbr, NtfsPartitionBootRecord};

macro_rules! impl_binary_bitmask_parse {
    // General implementation for all unsigned and signed integer types.
    // The enum type and inner bitmask type must be known
    ($enum_type:ty, $inner_type:ty, $from_types:expr) => {
        impl BinaryParse for $enum_type {
            fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError>
            where
                Self: Sized,
            {
                let flags = $from_types(input[..Self::SIZE].try_into().unwrap());
                Ok(Self::from(flags))
            }
        }

        impl BinarySize for $enum_type {
            const SIZE: usize = ::core::mem::size_of::<$inner_type>();
        }
    };

    // Used for bitmasks of u8s only
    ($enum_type:ty, $inner_type:ty) => {
        impl BinarySize for $enum_type {
            const SIZE: usize = ::core::mem::size_of::<Self>();
        }

        impl BinaryParse for $enum_type {
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
    let attribute_header = disk.read::<CommonAttributeHeader>().unwrap();
    if attribute_header.is_resident() {
        let resident_attribute_header = disk.read::<ResidentAttributeHeader>().unwrap();
        if attribute_header.name_length > 0 {
            let name = parse_attribute_name(&disk, &attribute_header).unwrap();
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
        let non_resident_attribute_header = disk.read::<NonResidentAttributeHeader>().unwrap();
        if attribute_header.name_length > 0 {
            let name = parse_attribute_name(&disk, &attribute_header).unwrap();
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
pub enum Attribute {
    StandardInfomation(StandardInformation),
    FileName(FileName),
    ObjectId(ObjectIds),
    Data(Vec<DataRun>),
    BitMap(Vec<DataRun>),
    SecurityDescriptor(SecurityDescriptor),
    VolumeName(String),
    VolumeInformation(VolumeInformation),
    IndexRoot(Vec<IndexEntry>),
    IndexAllocation((Option<String>, Vec<DataRun>)),
    ReparsePoint(ReparsePoint),
    EAInformation(ExtendedAttributeInformation),
    ExtendedAttributes(Vec<ExtendedAttribute>),
}

#[derive(BinaryStruct, Debug)]
pub struct StandardInformation {
    date_time_file_creation: u64,
    date_time_file_modification: u64,
    date_time_mft_modification: u64,
    date_time_file_reading: u64,
    file_permissions: DosFilePermissionFlags,
    max_number_of_versions: u32,
    version_number: u64,
}

/// Packed u64 where the lower 48 bits contain the MFT record index and the high 16 are the sequence number
#[derive(Debug)]
struct FileReferenceNumber {
    record_index: u64,
    sequence_number: u16,
}

impl BinaryParse for FileReferenceNumber {
    fn parse(input: &[u8]) -> Result<Self, binary_struct::ParseError>
    where
        Self: Sized,
    {
        let packed_frn = u64::from_le_bytes(input[..Self::SIZE].try_into().unwrap());
        let record_index = packed_frn & 0x0000_FFFF_FFFF_FFFF;
        let sequence_number = (packed_frn >> 48) as u16;

        Ok(Self {
            record_index,
            sequence_number,
        })
    }
}

impl BinarySize for FileReferenceNumber {
    const SIZE: usize = std::mem::size_of::<u64>();
}

#[derive(BinaryStruct, Debug)]
struct FileName {
    reference_to_parent_dir: FileReferenceNumber,
    datetime_file_creation: u64,
    datetime_file_modification: u64,
    datetime_mft_modification: u64,
    datetime_file_reading: u64,
    file_size_allocated_on_disk: u64,
    real_file_size: u64,
    flags: DosFilePermissionFlags,
    extended_attributes_and_reparse: u32,
    name_size: u8,
    namespace: u8,
    #[binary_struct(num_bytes_from = "name_size", encoding = "utf16")]
    name: String,
}

#[bitmask(u32)]
pub enum DosFilePermissionFlags {
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
impl_binary_bitmask_parse!(DosFilePermissionFlags, u32, u32::from_le_bytes);

#[derive(BinaryStruct, Debug)]
struct ObjectIds {
    object_id: Guid,
    birth_volume_id: Guid,
    birth_object_id: Guid,
    domain_id: Guid,
}

#[derive(Debug)]
struct DataRun {
    length: u64,
    offset: i64,
}

fn parse_dataruns(disk: &BufferedMappedDisk) -> MappedDiskResult<Vec<DataRun>> {
    let mut dataruns = Vec::new();
    while disk.peek::<u8>().unwrap() != 0 {
        let datarun_header_byte = disk.read::<u8>().unwrap();
        // Length is a VCN
        // i.e byte offset = length * sectors/cluster * bytes/sector
        let mut length = 0u64;
        // Offset is defined as number of clusters
        // i.e byte offset = offset * sectors/cluster * bytes/sector
        let mut offset = 0i64;

        let high_nibble = datarun_header_byte >> 4;
        let low_nibble = datarun_header_byte & 0b00001111;

        for i in 0..low_nibble as usize {
            length |= (disk.read::<u8>().unwrap() as u64) << (i * 8);
        }

        for i in 0..high_nibble as usize {
            offset |= (disk.read::<u8>().unwrap() as i64) << (i * 8);
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
            u64::from_be_bytes(tmp[0..8].try_into().unwrap())
        };
        // Read sub authorities into a vec, each sub authority is 4 bytes
        let mut sub_authorities = Vec::new();
        for i in 0..sub_authority_count {
            let current_iteration_offset = i as usize * 4;
            sub_authorities.push(u32::from_le_bytes(
                input[(8 + current_iteration_offset)..(12 + current_iteration_offset)]
                    .try_into()
                    .unwrap(),
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
        // revision(1 byte) + sub authority count(1 byte) + identifier authority(6 bytes) + (N * sub authorities (4 bytes each))
        1 + 1 + 6 + (sub_authority_count as usize * 4)
    }
}

fn parse_attribute_name(
    disk: &BufferedMappedDisk,
    attribute_header: &CommonAttributeHeader,
) -> MappedDiskResult<String> {
    disk.read_string_utf16(attribute_header.name_length as usize * 2usize)
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
            let first_two_bytes_sid = disk.peek::<[u8; 2]>().unwrap();
            let sub_authority_count = first_two_bytes_sid[1];
            disk.read_with_size::<SecurityIdentifier>(SecurityIdentifier::calculate_size(
                sub_authority_count,
            ))
        }
    }
}

fn parse_access_control_list(disk: &BufferedMappedDisk) -> MappedDiskResult<AccessControlList> {
    let mut acl = disk.read::<AccessControlList>().unwrap();
    for _ in 0..acl.ace_count {
        let ace = disk.read::<AccessControlEntry>().unwrap();
        let sid = parse_sid(disk, Some(&ace)).unwrap();
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
    file_reference: FileReferenceNumber,
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

#[derive(Debug)]
pub enum IndexEntry {
    SubNode {
        attribute_name: String,
        vcn: u64,
    },
    Sdh {
        key: SdhIndexKey,
        value: SdhIndexValue,
    },
    Sii {
        key: SiiIndexKey,
        value: SiiIndexValue,
    },
    FileName(FileName),
    QuotaO {
        sid: SecurityIdentifier,
        owner_id: u32,
    },
    ObjIdO {
        reference_number: FileReferenceNumber,
        ids: ObjectIds,
    },
    Q {
        owner_id: u32,
        value: QuotaIndexValue,
        sid: Option<SecurityIdentifier>,
    },
    R {
        reparse_flags: u32,
        reference_number: FileReferenceNumber,
    },
}

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
    #[binary_struct(num_bytes = 4, encoding = "utf16")]
    padding: String,
}

#[derive(BinaryStruct, Debug)]
struct SiiIndexKey {
    security_id: u32,
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
struct ExtendedAttribute {
    name: String,
    value: Vec<u8>,
}

#[derive(Debug)]
pub struct ParsingContext {
    pub record_number: usize,
}

#[derive(BinaryStruct, Debug)]
struct ReparsePointHeader {
    reparse_tag: ReparseTag,
    reparse_data_length: u16,
    _padding: Skip<2>,
}

#[bitmask(u32)]
pub enum ReparseTag {
    MountPoint = 0x3,
    SymbolicLink = 0xc,
    Isalias = 0x20000000,
    IsHighLatency = 0x40000000,
    IsMicrosoft = 0x80000000,
}
impl_binary_bitmask_parse!(ReparseTag, u32, u32::from_le_bytes);

#[derive(BinaryStruct, Debug)]
struct ReparseDataHeader {
    substitute_name_offset: u16,
    substitute_name_length: u16,
    print_name_offset: u16,
    print_name_length: u16,
}

#[bitmask(u32)]
pub enum SymbolicLinkFlags {
    Absolute = 0x0,
    Relative = 0x1,
}
impl_binary_bitmask_parse!(SymbolicLinkFlags, u32, u32::from_le_bytes);

#[derive(Debug)]
struct ReparsePoint {
    tag: ReparseTag,
    path_type: SymbolicLinkFlags,
    substitute_name: String,
    path_name: String,
}

pub fn parse_attribute<'a>(
    disk: &'a BufferedMappedDisk,
    context: &ParsingContext,
) -> MappedDiskResult<Option<Attribute>> {
    let offset_of_attribute_header = disk.current_offset();
    let attribute_header = parse_attribute_header(disk).unwrap();
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
        disk.set_cursor(offset_of_attribute_header + attribute_offset as usize)
            .unwrap();
    }
    println!("attribute_header: {:#?}", attribute_header);
    match attribute_header.attribute_type() {
        0x10 => {
            let std_info = disk.read::<StandardInformation>().unwrap();
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
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::StandardInfomation(std_info)))
        }
        0x20 => {
            // $ATTRIBUTE_LIST
            todo!("$ATTRIBUTE_LIST")
        }
        0x30 => {
            let file_name = disk
                .read_with_size::<FileName>(
                    attribute_header.resident_attribute_value_length() as usize
                )
                .unwrap();
            println!("File name: {:#?}", file_name);
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::FileName(file_name)))
        }
        0x40 => {
            // $OBJECT_ID
            let object_ids = disk.read::<ObjectIds>().unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::ObjectId(object_ids)))
        }
        0x50 => {
            // $SECURITY_DESCRIPTOR
            let starting_offset_of_security_descriptor = disk.current_offset();
            let security_descriptor_header = disk.read::<SecurityDescriptorHeader>().unwrap();

            let dacl = if security_descriptor_header
                .control_flags
                .contains(ControlFlags::DACLPresent)
            {
                disk.set_cursor(
                    starting_offset_of_security_descriptor
                        + security_descriptor_header.dacl_offset as usize,
                )
                .unwrap();
                Some(parse_access_control_list(disk).unwrap())
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
                )
                .unwrap();
                Some(parse_access_control_list(disk).unwrap())
            } else {
                None
            };
            // Jump to where the user SID is
            // Offset is relative to start of SecurityDescriptorHeader
            disk.set_cursor(
                starting_offset_of_security_descriptor
                    + security_descriptor_header.user_sid_offset as usize,
            )
            .unwrap();
            let user_sid = parse_sid(disk, None).unwrap();

            // Jump to where the group SID is
            // Offset is relative to start of SecurityDescriptorHeader
            disk.set_cursor(
                starting_offset_of_security_descriptor
                    + security_descriptor_header.group_sid_offset as usize,
            )
            .unwrap();
            let group_sid = parse_sid(disk, None).unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();

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
                disk.read_string_utf16(name_length as usize)?
            } else {
                String::new()
            };
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::VolumeName(volume_name)))
        }
        0x70 => {
            // $VOLUME_INFORMATION
            let volume_information = disk.read::<VolumeInformation>().unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::VolumeInformation(volume_information)))
        }
        0x80 => {
            // $DATA
            let dataruns = parse_dataruns(disk).unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::Data(dataruns)))
        }
        0x90 => {
            // $INDEX_ROOT
            println!("Offest: {}", disk.current_offset());
            let index_root_header = disk.read::<IndexRootHeader>().unwrap();
            println!("index_root_header: {:#?}", index_root_header);
            let index_header = disk.read::<IndexNodeHeader>().unwrap();
            println!("index_header: {:#?}", index_header);
            let mut offset = disk.current_offset();
            let mut index_entries: Vec<IndexEntry> = Vec::new();
            loop {
                if attribute_header.name().is_none() {
                    panic!("Attribute name is none");
                }

                let attribute_name = attribute_header.name().unwrap();

                println!("Offest: {}", disk.current_offset());
                let index_entry_header = disk.read::<IndexEntryHeader>().unwrap();
                println!("index_entry_header: {:#?}", index_entry_header);
                if index_entry_header.flags.contains(IndexEntryFlags::SubNode) {
                    // TODO: Need attribute name for to match against the index allocation name to get the
                    // datarun
                    println!("SubNode offset: {}", disk.current_offset());
                    let subnode_vcn = disk.read::<u64>().unwrap();
                    index_entries.push(IndexEntry::SubNode {
                        attribute_name: attribute_name.to_owned(),
                        vcn: subnode_vcn,
                    });
                }

                if index_entry_header
                    .flags
                    .contains(IndexEntryFlags::LastIndexEntry)
                {
                    break;
                }

                let index_entry = match attribute_name {
                    "$SDH" => {
                        let key = disk.read::<SdhIndexKey>().unwrap();
                        let value = disk.read::<SdhIndexValue>().unwrap();
                        let entry = IndexEntry::Sdh { key, value };
                        println!("Entry: {:#?}", entry);
                        entry
                    }
                    "$SII" => {
                        let key = disk.read::<SiiIndexKey>().unwrap();
                        let value = disk.read::<SiiIndexValue>().unwrap();
                        let entry = IndexEntry::Sii { key, value };
                        println!("Entry: {:#?}", entry);
                        entry
                    }
                    "$I30" => {
                        let file_name_attribute = disk
                            .read_with_size::<FileName>(
                                index_entry_header.index_entry_length as usize,
                            )
                            .unwrap();
                        let entry = IndexEntry::FileName(file_name_attribute);
                        println!("Entry: {:#?}", entry);
                        entry
                    }
                    "$O" => {
                        // If this record is $ObjId
                        let entry = if context.record_number == 25 {
                            let object_id = disk.read::<Guid>()?;
                            let reference_number = disk.read::<FileReferenceNumber>()?;
                            let birth_volume_id = disk.read::<Guid>()?;
                            let birth_object_id = disk.read::<Guid>()?;
                            let domain_id = disk.read::<Guid>()?;
                            IndexEntry::ObjIdO {
                                reference_number,
                                ids: ObjectIds {
                                    object_id,
                                    birth_volume_id,
                                    birth_object_id,
                                    domain_id,
                                },
                            }
                        } else {
                            let sid = parse_sid(disk, None).unwrap();
                            let owner_id = disk.read::<u32>().unwrap();
                            IndexEntry::QuotaO { sid, owner_id }
                        };

                        println!("Entry: {:#?}", entry);
                        entry
                    }
                    "$Q" => {
                        let owner_id = disk.read::<u32>().unwrap();
                        let value = disk.read::<QuotaIndexValue>().unwrap();
                        let sid = if !value.flags.contains(QuotaFlags::DefaultLimits) {
                            Some(parse_sid(disk, None).unwrap())
                        } else {
                            None
                        };
                        let entry = IndexEntry::Q {
                            owner_id,
                            value,
                            sid,
                        };
                        println!("Entry: {:#?}", entry);
                        entry
                    }
                    "$R" => {
                        let reparse_flags = disk.read::<u32>()?;
                        let reference_number = disk.read::<FileReferenceNumber>()?;
                        let _ = disk.read::<[u8; 4]>()?;
                        IndexEntry::R {
                            reparse_flags,
                            reference_number,
                        }
                    }
                    _ => {
                        todo!(
                            "Unknown or unsupported $INDEX_ROOT name: {} @ {}",
                            attribute_name,
                            disk.current_offset()
                        );
                    }
                };
                index_entries.push(index_entry);

                offset += index_entry_header.index_entry_length as usize;
                disk.set_cursor(offset).unwrap();
            }
            println!("Offest: {}", disk.current_offset());
            Ok(Some(Attribute::IndexRoot(index_entries)))
        }
        0xA0 => {
            // $INDEX_ALLOCATION
            // FIXME: Need to perform "fixups" with update sequqnces when actually reading the non-resident
            // data
            let name = attribute_header.name();
            let dataruns = parse_dataruns(disk).unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            println!("Index allocation dataruns: {:#?}", dataruns);
            Ok(Some(Attribute::IndexAllocation((
                name.map(|s| s.to_string()),
                dataruns,
            ))))
        }
        0xB0 => {
            // $BITMAP
            let dataruns = parse_dataruns(disk).unwrap();
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::BitMap(dataruns)))
        }
        0xC0 => {
            // $REPARSE_POINT
            let reparse_point_header = disk.read::<ReparsePointHeader>()?;
            let tag = reparse_point_header.reparse_tag;
            let reparse_point = if tag.contains(ReparseTag::IsMicrosoft) {
                let is_symlink = tag.contains(ReparseTag::SymbolicLink);

                if tag.intersects(ReparseTag::SymbolicLink.or(ReparseTag::MountPoint)) {
                    let reparse_data_header = disk.read::<ReparseDataHeader>()?;

                    let path_type = if is_symlink {
                        disk.read::<SymbolicLinkFlags>()?
                    } else {
                        SymbolicLinkFlags::Absolute
                    };
                    let start_offset = disk.current_offset();
                    disk.set_cursor(
                        start_offset + reparse_data_header.substitute_name_offset as usize,
                    )?;
                    let substitute_name = disk.read_string_utf16(reparse_data_header.substitute_name_length as usize)?;

                    disk.set_cursor(start_offset + reparse_data_header.print_name_offset as usize)?;
                    let path_name = disk.read_string_utf16(reparse_data_header.print_name_length as usize)?;
                    ReparsePoint {
                        tag,
                        path_type,
                        substitute_name,
                        path_name,
                    }
                } else {
                    todo!("$REPARSE_POINT: {}", disk.current_offset())
                }
            } else {
                todo!("$REPARSE_POINT non-microsoft: {}", disk.current_offset())
            };
            disk.set_cursor(
                offset_of_attribute_header + attribute_header.total_attribute_length(),
            )?;
            Ok(Some(Attribute::ReparsePoint(reparse_point)))
        }
        0xD0 => {
            // $EA_INFORMATION
            let ea_information = disk.read::<ExtendedAttributeInformation>().unwrap();
            println!("ea_info: {:#?}", ea_information);
            Ok(Some(Attribute::EAInformation(ea_information)))
        }
        0xE0 => {
            // $EA
            let mut extended_attributes: Vec<ExtendedAttribute> = Vec::new();
            let mut offset = disk.current_offset();
            loop {
                // If the next ea offset is 0, exit the loop
                if disk.peek::<u32>().unwrap() == 0 {
                    break;
                }
                let extended_attribute = disk.read::<ExtendedAttributeHeader>().unwrap();
                let name = String::from_utf8(
                    disk.read_bytes(extended_attribute.name_length as usize)
                        .unwrap()
                        .to_vec(),
                )
                .unwrap();
                let value = disk
                    .read_bytes(extended_attribute.value_length as usize)
                    .unwrap();

                extended_attributes.push(ExtendedAttribute {
                    name,
                    value: value.to_owned(),
                });

                offset += extended_attribute.next_ea_offset as usize;
                disk.set_cursor(offset).unwrap();
            }
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(Some(Attribute::ExtendedAttributes(extended_attributes)))
        }
        0x100 => {
            // $LOGGED_UTILITY_STREAM
            // todo!("$LOGGED_UTILITY_STREAM: {}", disk.current_offset())
            disk.set_cursor(offset_of_attribute_header + attribute_header.total_attribute_length())
                .unwrap();
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn parse_file_attributes(
    disk: &BufferedMappedDisk,
    starting_byte_offset: usize,
    context: ParsingContext,
) -> MappedDiskResult<Vec<Attribute>> {
    disk.set_cursor(starting_byte_offset).unwrap();
    let mut attributes: Vec<Attribute> = Vec::new();
    while disk.peek::<u32>().unwrap() != u32::MAX {
        let attribute = parse_attribute(&disk, &context).unwrap();
        if let Some(attr) = attribute {
            attributes.push(attr);
        }
    }
    Ok(attributes)
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
        fs_reader.disk.set_cursor(current_offset).unwrap();
        let file_descriptor = fs_reader.disk.read::<MftFileRecord>().unwrap();

        if file_descriptor.signature == "FILE" {
            let record_number = (current_offset - fs_reader.byte_offset_from_lba(starting_lba))
                / partition_boot_record.mft_size();
            println!("Entry number: {}", record_number);
            println!("MftFileDescriptor: {:#?}", file_descriptor);
            fs_reader
                .disk
                .set_cursor(current_offset + file_descriptor.offest_of_update_seq as usize)
                .unwrap();
            let update_sequence_number = fs_reader.disk.read::<u16>().unwrap();
            let update_sequence_array = fs_reader
                .disk
                .read_bytes((file_descriptor.size_of_update_seq as usize - 1) * 2)
                .unwrap();

            let mut buffered_mapped_disk =
                BufferedMappedDisk::new(fs_reader.disk, partition_boot_record.mft_size());
            buffered_mapped_disk.fill_buffer_at(current_offset).unwrap();
            patch_update_sequence(
                &mut buffered_mapped_disk,
                partition_boot_record.sector_size() as usize,
                update_sequence_number,
                &update_sequence_array,
            )
            .unwrap();

            let attributes = parse_file_attributes(
                &buffered_mapped_disk,
                current_offset + file_descriptor.offset_first_attribute as usize,
                ParsingContext { record_number },
            )
            .unwrap();
            println!("Attributes: {:#?}", attributes)
        } else {
            // TODO: Skip
        }
        current_offset += partition_boot_record.mft_size();
    }

    Ok(())
}
