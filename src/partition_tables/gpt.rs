use binary_struct::prelude::*;
use binary_struct::{BinaryStruct, Skip};

use crate::guid::Guid;
use crate::mapped_disk::{MappedDisk, MappedDiskError, MappedDiskResult, SECTOR_SIZE};

#[derive(BinaryStruct, Debug)]
struct GptHeader {
    #[binary_struct(num_bytes = 8)]
    efi_part: String,
    // Revision 1.0 (00h 00h 01h 00h) for UEFI 2.8
    revision: [u8; 4],
    // Header size in little endian (in bytes, usually 5Ch 00h 00h 00h or 92 bytes)
    header_size: u32,
    crc32: u32,
    // Reserved space of 0's
    _reserved: Skip<4>,
    // Location of this GPT header
    current_lba: u64,
    // Location of the backup GPT header
    backup_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: Guid,
    starting_lba_of_partition_entries: u64,
    number_partition_entries: u32,
    size_single_partition_entry: u32,
    crc32_partition_entries: u32,
}

impl GptHeader {
    fn number_of_sectors(&self) -> u32 {
        (self.number_partition_entries * self.size_single_partition_entry) / SECTOR_SIZE as u32
    }
}

fn validate_header_crc32(
    disk: &MappedDisk,
    header_size: u32,
    crc32: u32,
) -> MappedDiskResult<bool> {
    let mut header_bytes = disk
        .read_bytes_starting_at_sector(1, header_size as usize)?
        .to_vec();

    // CRC32 of header (offset +0 to +0x5b) in little endian, with this field zeroed during calculation
    header_bytes.splice(16..20, vec![0u8; 4]);
    Ok(calculate_crc32(header_bytes) == crc32)
}

// https://lxp32.github.io/docs/a-simple-example-crc32-calculation/
fn calculate_crc32(bytes: Vec<u8>) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for mut byte in bytes {
        for _ in 0..8 {
            let tmp = ((byte as u32) ^ crc) & 1;
            crc >>= 1;
            if tmp != 0 {
                crc = crc ^ 0xEDB88320;
            }
            byte >>= 1;
        }
    }
    !crc
}

#[derive(BinaryStruct, Debug)]
pub struct GptPartitionTableEntry {
    partition_type_guid: Guid,
    unique_partition_guid: Guid,
    starting_lba: u64,
    ending_lba: u64,
    attribute_flags: [u8; 8],
    partition_name: [u8; 72],
}

impl GptPartitionTableEntry {
    fn is_empty(&self) -> bool {
        self.starting_lba == 0
            && self.ending_lba == 0
            && self.attribute_flags.iter().all(|byte| *byte == 0)
            && self.partition_name.iter().all(|byte| *byte == 0)
    }

    pub fn partition_type(&self) -> String {
        self.partition_type_guid.to_string()
    }

    pub fn partition_type_str(&self) -> &'static str {
        lookup_partition_type(&self.partition_type_guid)
    }

    pub fn starting_lba(&self) -> u64 {
        self.starting_lba
    }

    pub fn ending_lba(&self) -> u64 {
        self.ending_lba
    }
    
    pub fn number_of_sectors(&self) -> u64 {
        self.ending_lba - self.starting_lba
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GptParseError {
    #[error("Mapped Disk Error: {0}")]
    MappedDisk(#[from] MappedDiskError),
    #[error("Could not parse GPT header, 'EFI PART' not found")]
    InvalidSignature,
    #[error(
        "CRC32 mismatch in partition table entreis: expected {expected:#010x} got {actual:#010x}"
    )]
    Crc32Mismatch { expected: u32, actual: u32 },
}

fn parse_gpt_header(disk: &MappedDisk) -> Result<GptHeader, GptParseError> {
    let mut header = disk.read::<GptHeader>()?;
    if header.efi_part != "EFI PART" {
        return Err(GptParseError::InvalidSignature);
    }

    if !validate_header_crc32(&disk, header.header_size, header.crc32)? {
        disk.set_cursor_from_lba(header.backup_lba as usize)?;
        header = parse_gpt_header(disk)?;
    }
    Ok(header)
}

fn validate_partition_entries_crc32(
    disk: &MappedDisk,
    header: &GptHeader,
) -> Result<(), GptParseError> {

    let buffer = disk
        .read_sectors_at(
            header.starting_lba_of_partition_entries as usize,
            header.number_of_sectors() as usize,
        )?
        .to_vec();

    let crc32 = calculate_crc32(buffer);
    if crc32 != header.crc32_partition_entries {
        return Err(GptParseError::Crc32Mismatch {
            expected: header.crc32_partition_entries,
            actual: crc32,
        });
    }
    Ok(())
}

pub fn parse_gpt(disk: &MappedDisk) -> Result<Vec<GptPartitionTableEntry>, GptParseError> {
    disk.set_cursor_from_lba(1)?;
    let header = parse_gpt_header(&disk)?;

    validate_partition_entries_crc32(disk, &header)?;

    let mut partition_table = Vec::new();
    'outer: for index in 0..header.number_of_sectors() {
        let sector_lba = header.starting_lba_of_partition_entries + index as u64;
        disk.set_cursor_from_lba(sector_lba as usize)?;

        loop {
            let partition_table_entry = disk.read::<GptPartitionTableEntry>()?;
            if partition_table_entry.is_empty() {
                break 'outer;
            }
            partition_table.push(partition_table_entry);
        }
    }

    Ok(partition_table)
}

fn lookup_partition_type(partition_type: &Guid) -> &'static str {
    match partition_type.to_string().as_str() {
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "EFI System",
        "024DEE41-33E7-11D3-9D69-0008C781F39F" => "MBR partition scheme",
        "D3BFE2DE-3DAF-11DF-BA40-E3A556D89593" => "Intel Fast Flash",
        "21686148-6449-6E6F-744E-656564454649" => "BIOS boot",
        "F4019732-066E-4E12-8273-346C5641494F" => "Sony boot partition",
        "BFBFAFE7-A34F-448A-9A5B-6213EB736C22" => "Lenovo boot partition",
        "9E1A2D38-C612-4316-AA26-8B49521E5A8B" => "PowerPC PReP boot",
        "7412F7D5-A156-4B13-81DC-867174929325" => "ONIE boot",
        "D4E6E2CD-4469-46F3-B5CB-1BFF57AFC149" => "ONIE config",
        "E3C9E316-0B5C-4DB8-817D-F92DF00215AE" => "Microsoft reserved",
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "Microsoft basic data",
        "5808C8AA-7E8F-42E0-85D2-E1E90434CFB3" => "Microsoft LDM metadata",
        "AF9B60A0-1431-4F62-BC68-3311714A69AD" => "Microsoft LDM data",
        "DE94BBA4-06D1-4D40-A16A-BFD50179D6AC" => "Windows recovery environment",
        "37AFFC90-EF7D-4E96-91C3-2D7AE055B174" => "IBM General Parallel Fs",
        "E75CAF8F-F680-4CEE-AFA3-B001E56EFC2D" => "Microsoft Storage Spaces",
        "75894C1E-3AEB-11D3-B7C1-7B03A0000000" => "HP-UX data",
        "E2A1E728-32E3-11D6-A682-7B03A0000000" => "HP-UX service",
        "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F" => "Linux swap",
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => "Linux filesystem",
        "3B8F8425-20E0-4F3B-907F-1A25A76F98E8" => "Linux server data",
        "44479540-F297-41B2-9AF7-D131D5F0458A" => "Linux root (x86)",
        "69DAD710-2CE4-4E3C-B16C-21A1D49ABED3" => "Linux root (ARM)",
        "4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709" => "Linux root (x86-64)",
        "B921B045-1DF0-41C3-AF44-4C6F280D3FAE" => "Linux root (ARM-64)",
        "993D8D3D-F80E-4225-855A-9DAF8ED7EA97" => "Linux root  (IA-64)",
        "8DA63339-0007-60C0-C436-083AC8230908" => "Linux reserved",
        "933AC7E1-2EB4-4F13-B844-0E14E2AEF915" => "Linux home",
        "A19D880F-05FC-4D3B-A006-743F0F84911E" => "Linux RAID",
        "BC13C2FF-59E6-4262-A352-B275FD6F7172" => "Linux extended boot",
        "E6D6D379-F507-44C2-A23C-238F2A3DF928" => "Linux LVM",
        "516E7CB4-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD data",
        "83BD6B9D-7F41-11DC-BE0B-001560B84F0F" => "FreeBSD boot",
        "516E7CB5-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD swap",
        "516E7CB6-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD UFS",
        "516E7CBA-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD ZFS",
        "516E7CB8-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD Vinum",
        "48465300-0000-11AA-AA11-00306543ECAC" => "Apple HFS/HFS+",
        "55465300-0000-11AA-AA11-00306543ECAC" => "Apple UFS",
        "52414944-0000-11AA-AA11-00306543ECAC" => "Apple RAID",
        "52414944-5F4F-11AA-AA11-00306543ECAC" => "Apple RAID offline",
        "426F6F74-0000-11AA-AA11-00306543ECAC" => "Apple boot",
        "4C616265-6C00-11AA-AA11-00306543ECAC" => "Apple label",
        "5265636F-7665-11AA-AA11-00306543ECAC" => "Apple TV recovery",
        "53746F72-6167-11AA-AA11-00306543ECAC" => "Apple Core storage",
        "6A82CB45-1DD2-11B2-99A6-080020736631" => "Solaris boot",
        "6A85CF4D-1DD2-11B2-99A6-080020736631" => "Solaris root",
        "6A898CC3-1DD2-11B2-99A6-080020736631" => "Solaris /usr & Apple ZFS",
        "6A87C46F-1DD2-11B2-99A6-080020736631" => "Solaris swap",
        "6A8B642B-1DD2-11B2-99A6-080020736631" => "Solaris backup",
        "6A8EF2E9-1DD2-11B2-99A6-080020736631" => "Solaris /var",
        "6A90BA39-1DD2-11B2-99A6-080020736631" => "Solaris /home",
        "6A9283A5-1DD2-11B2-99A6-080020736631" => "Solaris alternate sector",
        "6A945A3B-1DD2-11B2-99A6-080020736631" => "Solaris reserved 1",
        "6A9630D1-1DD2-11B2-99A6-080020736631" => "Solaris reserved 2",
        "6A980767-1DD2-11B2-99A6-080020736631" => "Solaris reserved 3",
        "6A96237F-1DD2-11B2-99A6-080020736631" => "Solaris reserved 4",
        "6A8D2AC7-1DD2-11B2-99A6-080020736631" => "Solaris reserved 5",
        "49F48D32-B10E-11DC-B99B-0019D1879648" => "NetBSD swap",
        "49F48D5A-B10E-11DC-B99B-0019D1879648" => "NetBSD FFS",
        "49F48D82-B10E-11DC-B99B-0019D1879648" => "NetBSD LFS",
        "2DB519C4-B10E-11DC-B99B-0019D1879648" => "NetBSD concatenated",
        "2DB519EC-B10E-11DC-B99B-0019D1879648" => "NetBSD encrypted",
        "49F48DAA-B10E-11DC-B99B-0019D1879648" => "NetBSD RAID",
        "FE3A2A5D-4F32-41A7-B725-ACCC3285A309" => "ChromeOS kernel",
        "3CB8E202-3B7E-47DD-8A3C-7FF2A13CFCEC" => "ChromeOS root fs",
        "2E0A753D-9E48-43B0-8337-B15192CB1B5E" => "ChromeOS reserved",
        "85D5E45A-237C-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD data",
        "85D5E45E-237C-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD boot",
        "85D5E45B-237C-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD swap",
        "0394EF8B-237E-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD UFS",
        "85D5E45D-237C-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD ZFS",
        "85D5E45C-237C-11E1-B4B3-E89A8F7FC3A7" => "MidnightBSD Vinum",
        "45B0969E-9B03-4F30-B4C6-B4B80CEFF106" => "Ceph Journal",
        "45B0969E-9B03-4F30-B4C6-5EC00CEFF106" => "Ceph Encrypted Journal",
        "4FBD7E29-9D25-41B8-AFD0-062C0CEFF05D" => "Ceph OSD",
        "4FBD7E29-9D25-41B8-AFD0-5EC00CEFF05D" => "Ceph crypt OSD",
        "89C57F98-2FE5-4DC0-89C1-F3AD0CEFF2BE" => "Ceph disk in creation",
        "89C57F98-2FE5-4DC0-89C1-5EC00CEFF2BE" => "Ceph crypt disk in creation",
        "AA31E02A-400F-11DB-9590-000C2911D1B8" => "VMware VMFS",
        "9D275380-40AD-11DB-BF97-000C2911D1B8" => "VMware Diagnostic",
        "381CFCCC-7288-11E0-92EE-000C2911D0B2" => "VMware Virtual SAN",
        "77719A0C-A4A0-11E3-A47E-000C29745A24" => "VMware Virsto",
        "9198EFFC-31C0-11DB-8F78-000C2911D1B8" => "VMware Reserved",
        "824CC7A0-36A8-11E3-890A-952519AD3F61" => "OpenBSD data",
        "CEF5A9AD-73BC-4601-89F3-CDEEEEE321A1" => "QNX6 file system",
        "C91818F9-8025-47AF-89D2-F030D7000C2C" => "Plan 9 partition",
        "5B193300-FC78-40CD-8002-E86C45580B47" => "HiFive Unleashed FSBL",
        "2E54B353-1271-4842-806F-E436D6AF6985" => "HiFive Unleashed BBL",
        _ => "Unknown Partition Type",
    }
}
