use crate::{util, Elev};
use memmap2::Mmap;

pub(crate) enum SampleStore {
    Tombstone(usize),
    InMem(Box<[Elev]>),
    MemMap(Mmap),
}

impl SampleStore {
    pub(crate) fn get_linear_unchecked(&self, index: usize) -> Elev {
        match self {
            Self::Tombstone(size) => {
                assert!(
                    index < *size,
                    "index {index} exceeds tombstone's virtual size {size}"
                );
                0
            }
            Self::InMem(samples) => samples[index],
            Self::MemMap(raw) => {
                let start = index * size_of::<Elev>();
                let end = start + size_of::<Elev>();
                let bytes = &mut &raw.as_ref()[start..end];
                util::parse_sample(bytes)
            }
        }
    }

    /// Returns the lowest elevation sample in this data.
    pub(crate) fn min(&self) -> Elev {
        match self {
            Self::Tombstone(_) => 0,
            Self::InMem(samples) => samples.iter().min().copied().unwrap(),
            Self::MemMap(raw) => (*raw)
                .chunks_exact(2)
                .map(util::parse_sample)
                .min()
                .unwrap(),
        }
    }

    /// Returns the highest elevation sample in this data.
    pub(crate) fn max(&self) -> Elev {
        match self {
            Self::Tombstone(_) => 0,
            Self::InMem(samples) => samples.iter().max().copied().unwrap(),
            Self::MemMap(raw) => (*raw)
                .chunks_exact(2)
                .map(util::parse_sample)
                .max()
                .unwrap(),
        }
    }
}
