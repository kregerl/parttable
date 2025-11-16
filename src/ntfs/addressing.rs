use std::ops::Add;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Bytes(pub usize);

impl Add<Bytes> for Bytes {
    type Output = Bytes;

    fn add(self, rhs: Bytes) -> Self::Output {
        Bytes(self.0 + rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lba(pub usize);

impl Lba {
    pub fn to_bytes(self, sector_size: usize) -> usize {
        self.0 * sector_size
    }
}
impl Add<Lba> for Lba {
    type Output = Lba;

    fn add(self, rhs: Lba) -> Self::Output {
        Lba(self.0 + rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Vcn(pub usize);

impl Vcn {
    pub fn to_bytes(self, cluster_size: usize) -> usize {
        self.0 * cluster_size
    }
}

impl Add<Vcn> for Vcn {
    type Output = Vcn;

    fn add(self, rhs: Vcn) -> Self::Output {
        Vcn(self.0 + rhs.0)
    }
}
