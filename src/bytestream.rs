use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom, Cursor},
    path::Path,
    string::FromUtf16Error,
};

use byteorder::{ByteOrder, ReadBytesExt};
pub const SECTOR_SIZE: usize = 512;

// FIXME: Remove `Readable` impls for numbers and replace with `ReadableEndianess`
pub trait Readable {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized;
}

pub trait ReadableEndianness {
    fn read<T>(reader: &mut ByteStream) -> io::Result<Self>
    where
        T: ByteOrder,
        Self: Sized;
}

impl Readable for u8 {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        reader.cursor.read_u8()
    }
}

impl Readable for i8 {
    fn read(reader: &mut ByteStream) -> io::Result<Self>
    where
        Self: Sized,
    {
        reader.cursor.read_i8()
    }
}

impl ReadableEndianness for u16 {
    fn read<T>(reader: &mut ByteStream) -> io::Result<Self>
    where
        T: ByteOrder,
        Self: Sized,
    {
        reader.cursor.read_u16::<T>()
    }
}

impl ReadableEndianness for u32 {
    fn read<T>(reader: &mut ByteStream) -> io::Result<Self>
    where
        T: ByteOrder,
        Self: Sized,
    {
        reader.cursor.read_u32::<T>()
    }
}

impl ReadableEndianness for u64 {
    fn read<T>(reader: &mut ByteStream) -> io::Result<Self>
    where
        T: ByteOrder,
        Self: Sized {
        reader.cursor.read_u64::<T>()
    }
}

pub struct ByteStream {
    reader: BufReader<File>,
    cursor: Cursor<Vec<u8>>,
}

impl ByteStream {
    // Offset in sectors
    pub fn new(path: &Path, size: usize, offset: u64) -> io::Result<Self> {
        let mut reader = BufReader::new(File::open(path)?);
        reader.seek(SeekFrom::Start(offset * SECTOR_SIZE as u64))?;
        let mut buffer = vec![0u8; size];
        reader.read_exact(&mut buffer)?;
        Ok(Self { reader, cursor: Cursor::new(buffer) })
    }

    pub fn from_byte_offset(path: &Path, size: usize, offset: u64) -> io::Result<Self> {
        let mut reader = BufReader::new(File::open(path)?);
        let sector_num = offset / SECTOR_SIZE as u64;
        let byte_offset = offset - (sector_num * SECTOR_SIZE as u64);

        reader.seek(SeekFrom::Start(sector_num * SECTOR_SIZE as u64))?;
        let mut buffer = vec![0u8; size];
        reader.read_exact(&mut buffer)?;
        let mut cursor = Cursor::new(buffer);
        io::copy(&mut cursor.by_ref().take(byte_offset), &mut io::sink())?;
        Ok(Self { reader, cursor })
    }

    pub fn get_byte_offset(&mut self) -> io::Result<u64> {
        Ok(self.reader.seek(SeekFrom::Current(0))? + self.cursor.position())
    }

    pub fn read_raw(&mut self, amount: usize) -> io::Result<Vec<u8>> {
        let mut buffer = vec![0u8; amount];
        self.cursor.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    /// Reads raw bytes from sectors starting at `from` until `from + amount` without advancing the buffered reader's index.
    pub fn read_raw_sectors_from_file(&mut self, from: usize, amount: usize) -> io::Result<Vec<u8>> {
        let start = from * SECTOR_SIZE;
        let amt = amount * SECTOR_SIZE;
        self.read_raw_bytes_from_file(start, amt)
    }

    /// Reads raw bytes starting at `from` until `from + amount` without advancing the buffered reader's index.
    pub fn read_raw_bytes_from_file(&mut self, from: usize, amount: usize) -> io::Result<Vec<u8>> {
        let current = self.reader.seek(SeekFrom::Current(0))?;
        let mut buffer = vec![0u8; amount];
        let _ = self.reader.seek(SeekFrom::Start(from as u64))?;
        self.reader.read_exact(&mut buffer)?;
        let _ = self.reader.seek(SeekFrom::Start(current))?;
        Ok(buffer)
    }

    pub fn peek_le<T>(&mut self) -> io::Result<T>
    where
        T: ReadableEndianness,
    {
        let current_index = self.cursor.seek(SeekFrom::Current(0))?;
        let result = self.read_le::<T>()?;
        let _ = self.cursor.seek(SeekFrom::Start(current_index))?;
        Ok(result)
    }

    pub fn read<T>(&mut self) -> io::Result<T>
    where
        T: Readable,
    {
        T::read(self)
    }

    pub fn read_le<T>(&mut self) -> io::Result<T>
    where
        T: ReadableEndianness,
    {
        T::read::<byteorder::LittleEndian>(self)
    }

    pub fn read_be<T>(&mut self) -> io::Result<T>
    where
        T: ReadableEndianness,
    {
        T::read::<byteorder::BigEndian>(self)
    }

    pub fn read_array<T, const S: usize>(&mut self) -> io::Result<[T; S]>
    where
        T: Readable + Copy,
    {
        let buffer = [T::read(self)?; S];
        Ok(buffer)
    }

    // Reads S bytes from the stream
    pub fn read_byte_array<const S: usize>(&mut self) -> io::Result<[u8; S]> {
        let mut buffer = [0u8; S];
        self.cursor.read_exact(&mut buffer)?;
        Ok(buffer)
    }

    pub fn skip_bytes(&mut self, amount: u64) -> io::Result<()> {
        io::copy(&mut self.cursor.by_ref().take(amount), &mut io::sink())?;
        Ok(())
    }

    pub fn jump_to_sector(&mut self, amount: u64) -> io::Result<()> {
        self.jump_to_byte(amount * SECTOR_SIZE as u64)
    }

    pub fn jump_to_byte(&mut self, amount: u64) -> io::Result<()> {
        self.reader.seek(SeekFrom::Start(amount))?;
        self.refill_buffer()?;
        Ok(())
    }

    fn refill_buffer(&mut self) -> io::Result<()> {
        let mut buffer = vec![0u8; SECTOR_SIZE];
        self.reader.read_exact(&mut buffer)?;
        self.cursor = Cursor::new(buffer);
        Ok(())
    }
}

pub fn interpret_bytes_as_utf16(name_bytes: &[u8]) -> Result<String, FromUtf16Error> {
    let num_bytes = name_bytes.len();
    let mut unicode_symbols: Vec<u16> = Vec::with_capacity(num_bytes / 2);
    for index in (0..num_bytes).step_by(2) {
        // Order of top and bottom here is reversed since the bytes are in little endian
        let first = name_bytes[index];
        let second = name_bytes[index + 1];
        unicode_symbols.push(bytes_to_u16(first, second));
    }
    String::from_utf16(&unicode_symbols)
}

fn bytes_to_u16(first: u8, second: u8) -> u16 {
    #[cfg(target_endian = "little")]
    {
        ((second as u16) << 8) | first as u16
    }
    #[cfg(target_endian = "big")]
    {
        ((first as u16) << 8) | second as u16
    }
}
