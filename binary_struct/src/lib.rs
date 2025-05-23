#[derive(Debug)]
pub enum ParseError {
    TooShort,
}

#[derive(Debug, Copy, Clone)]
pub struct Skip<const N: usize>;

impl<const N: usize> Default for Skip<N> {
    fn default() -> Self {
        Skip
    }
}

pub trait BinaryParse: Sized {
    const SIZE: usize = std::mem::size_of::<Self>();
    // const SIZE: usize;
    fn parse(input: &[u8]) -> Result<Self, ParseError> where Self: Sized;
}

impl<const N: usize> BinaryParse for [u8; N] {
    const SIZE: usize = std::mem::size_of::<Self>();
    // const SIZE: usize = N;
    fn parse(input: &[u8]) -> Result<Self, ParseError> {
        if input.len() < N {
            return Err(ParseError::TooShort);
        }
        Ok(input[..N].try_into().unwrap())
    }
}

impl BinaryParse for u8 {
    // const SIZE: usize = std::mem::size_of::<Self>();
    fn parse(input: &[u8]) -> Result<Self, ParseError> {
        Ok(input[0])
    }
}

impl BinaryParse for u16 {
    // const SIZE: usize = std::mem::size_of::<Self>();
    fn parse(input: &[u8]) -> Result<Self, ParseError> {
        Ok(u16::from_le_bytes(input[..Self::SIZE].try_into().unwrap()))
    }
}

impl BinaryParse for u32 {
    // const SIZE: usize = std::mem::size_of::<Self>();
    fn parse(input: &[u8]) -> Result<Self, ParseError> {
        Ok(u32::from_le_bytes(input[..Self::SIZE].try_into().unwrap()))
    }
}