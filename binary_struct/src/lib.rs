use std::{array::TryFromSliceError, fmt};

#[cfg(feature = "derive")]
pub use binary_struct_derive::BinaryStruct;

pub mod prelude;

macro_rules! impl_binary_parse {
    ($t:ty, $from_types:expr) => {
        impl BinarySize for $t {
            const SIZE: usize = ::core::mem::size_of::<Self>();
        }

        impl BinaryParse for $t {
            fn parse(input: &[u8]) -> Result<Self, ParseError> {
                if input.len() < Self::SIZE {
                    return Err(ParseError::TooShort);
                }
                Ok($from_types(input[..Self::SIZE].try_into()?))
            }
        }
    };

    (u8) => {
        impl BinarySize for u8 {
            const SIZE: usize = ::core::mem::size_of::<Self>();
        }

        impl BinaryParse for u8 {
            fn parse(input: &[u8]) -> Result<Self, ParseError> {
                if input.len() < Self::SIZE {
                    return Err(ParseError::TooShort);
                }
                Ok(input[0])
            }
        }
    };
    (i8) => {
        impl BinarySize for i8 {
            const SIZE: usize = ::core::mem::size_of::<Self>();
        }

        impl BinaryParse for i8 {
            fn parse(input: &[u8]) -> Result<Self, ParseError> {
                if input.len() < Self::SIZE {
                    return Err(ParseError::TooShort);
                }
                Ok(input[0] as i8)
            }
        }
    }
}
/// Trait for defining how types are interpreted from raw binary data.  
/// When implemented this will need to parse `Self` out of the slice of bytes.  
pub trait BinaryParse {
    fn parse(input: &[u8]) -> Result<Self, ParseError> where Self: Sized;
}

/// Trait for defining the size of a type.  
/// This is required since this will be used when reading a slice of bytes to pass to [`BinaryParse`].  
/// The size of a struct that derives BinaryParse cannot be `std::mem::size_of::<T>()` since the struct's size
/// does not take [`Skip<N>`] into account since its a zero sized type.
pub trait BinarySize {
    const SIZE: usize;
}

pub trait BinaryType: BinaryParse + BinarySize {}
impl<T: BinaryParse + BinarySize> BinaryType for T {}


#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Input too short")]
    TooShort,
    #[error("{0}")]
    TryFromSliceError(#[from] TryFromSliceError)
}

#[derive(Copy, Clone)]
pub struct Skip<const N: usize>;

impl<const N: usize> fmt::Debug for Skip<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Skip<{}>", N)
    }
}

impl<const N: usize> Default for Skip<N> {
    fn default() -> Self {
        Skip
    }
}

impl<const N: usize> BinarySize for [u8; N] {
    const SIZE: usize = N;
}

impl<const N: usize> BinaryParse for [u8; N] {
    fn parse(input: &[u8]) -> Result<Self, ParseError> {
        if input.len() < N {
            return Err(ParseError::TooShort);
        }
        Ok(input[..N].try_into().unwrap())
    }
}

impl_binary_parse!(i8);
impl_binary_parse!(u8);
impl_binary_parse!(u16, u16::from_le_bytes);
impl_binary_parse!(u32, u32::from_le_bytes);
impl_binary_parse!(u64, u64::from_le_bytes);