use super::{MemAccessSize, Memory};
use std::io;
use std::io::Read;

pub fn read_to_memory(
    reader: impl Read,
    memory: &mut impl Memory,
    start_addr: u32,
) -> io::Result<()> {
    let mut write_addr = start_addr;
    for b in reader.bytes() {
        let b = b?;
        if !memory.write_mem(write_addr, MemAccessSize::Byte, b as u32) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Could not write byte at address 0x{:08x}", write_addr),
            ));
        }
        write_addr += 1;
    }

    Ok(())
}

pub struct VecMemory {
    pub mem: Vec<u32>,
}

impl VecMemory {
    pub fn new(init_mem: Vec<u32>) -> VecMemory {
        VecMemory { mem: init_mem }
    }
}

impl Memory for VecMemory {
    fn read_mem(&mut self, addr: u32, size: MemAccessSize) -> Option<u32> {
        let (shift, mask) = match size {
            MemAccessSize::Byte => (addr & 0x3, 0xff),
            MemAccessSize::HalfWord => (addr & 0x2, 0xffff),
            MemAccessSize::Word => (0, 0xffffffff),
        };

        if (addr & 0x3) != shift {
            panic!("Memory read must be aligned");
        }

        let word_addr = addr >> 2;

        let read_data = self.mem.get(word_addr as usize).copied()?;

        Some((read_data >> (shift * 8)) & mask)
    }

    fn write_mem(&mut self, addr: u32, size: MemAccessSize, store_data: u32) -> bool {
        let (shift, mask) = match size {
            MemAccessSize::Byte => (addr & 0x3, 0xff),
            MemAccessSize::HalfWord => (addr & 0x2, 0xffff),
            MemAccessSize::Word => (0, 0xffffffff),
        };

        if (addr & 0x3) != shift {
            panic!("Memory write must be aligned");
        }

        let write_mask = !(mask << (shift * 8));

        let word_addr = (addr >> 2) as usize;

        if let Some(update_data) = self.mem.get(word_addr) {
            let new = (update_data & write_mask) | ((store_data & mask) << (shift * 8));
            self.mem[word_addr] = new;
            true
        } else {
            false
        }
    }
}

struct MemoryRegion {
    base: u32,
    size: u32,
    memory: Box<dyn Memory>,
}

pub struct MemorySpace {
    memory_regions: Vec<MemoryRegion>,
}

#[derive(Debug, PartialEq)]
pub enum MemorySpaceError {
    RegionOverlap,
    Unaligned,
}

impl MemorySpace {
    pub fn new() -> Self {
        MemorySpace {
            memory_regions: Vec::new(),
        }
    }

    fn region_overlaps_existing(&self, base: u32, size: u32) -> bool {
        for memory_region in self.memory_regions.iter() {
            if base + size <= memory_region.base {
                continue;
            }

            if memory_region.base + memory_region.size <= base {
                continue;
            }

            return true;
        }

        false
    }

    fn get_memory_region_by_addr(&mut self, addr: u32) -> Option<&mut MemoryRegion> {
        for memory_region in self.memory_regions.iter_mut() {
            if (addr >= memory_region.base) && (addr < (memory_region.base + memory_region.size)) {
                return Some(memory_region);
            }
        }

        None
    }

    pub fn add_memory(
        &mut self,
        base: u32,
        size: u32,
        memory: Box<dyn Memory>,
    ) -> Result<usize, MemorySpaceError> {
        if ((base & 0x3) != 0) || ((size & 0x3) != 0) {
            return Err(MemorySpaceError::Unaligned);
        }

        if self.region_overlaps_existing(base, size) {
            return Err(MemorySpaceError::RegionOverlap);
        }

        let new_mem_index = self.memory_regions.len();
        self.memory_regions
            .push(MemoryRegion { base, size, memory });

        Ok(new_mem_index)
    }

    pub fn get_memory_ref<T: Memory>(&self, index: usize) -> Option<&T> {
        self.memory_regions.get(index)?.memory.downcast_ref::<T>()
    }

    pub fn get_memory_mut<T: Memory>(&mut self, index: usize) -> Option<&mut T> {
        self.memory_regions
            .get_mut(index)?
            .memory
            .downcast_mut::<T>()
    }
}

impl Memory for MemorySpace {
    fn read_mem(&mut self, addr: u32, size: MemAccessSize) -> Option<u32> {
        let memory_region = self.get_memory_region_by_addr(addr)?;

        memory_region
            .memory
            .read_mem(addr - memory_region.base, size)
    }

    fn write_mem(&mut self, addr: u32, size: MemAccessSize, store_data: u32) -> bool {
        if let Some(memory_region) = self.get_memory_region_by_addr(addr) {
            memory_region
                .memory
                .write_mem(addr - memory_region.base, size, store_data)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_vec_memory() {
        let mut test_mem = VecMemory::new(vec![0xdeadbeef, 0xbaadf00d]);

        assert_eq!(test_mem.read_mem(0x0, MemAccessSize::Byte), Some(0xef));

        assert_eq!(test_mem.read_mem(0x5, MemAccessSize::Byte), Some(0xf0));

        assert_eq!(
            test_mem.read_mem(0x6, MemAccessSize::HalfWord),
            Some(0xbaad)
        );

        assert_eq!(
            test_mem.read_mem(0x4, MemAccessSize::Word),
            Some(0xbaadf00d)
        );

        assert_eq!(test_mem.write_mem(0x7, MemAccessSize::Byte, 0xff), true);

        assert_eq!(
            test_mem.write_mem(0x2, MemAccessSize::HalfWord, 0xaaaaface),
            true
        );

        assert_eq!(
            test_mem.write_mem(0x1, MemAccessSize::Byte, 0x1234abcd),
            true
        );

        assert_eq!(
            test_mem.read_mem(0x0, MemAccessSize::Word),
            Some(0xfacecdef)
        );

        assert_eq!(
            test_mem.read_mem(0x4, MemAccessSize::Word),
            Some(0xffadf00d)
        );

        assert_eq!(test_mem.read_mem(0x8, MemAccessSize::Word), None);

        assert_eq!(test_mem.write_mem(0x8, MemAccessSize::Word, 0x0), false);
    }

    struct TestMemory;

    impl Memory for TestMemory {
        fn read_mem(&mut self, addr: u32, size: MemAccessSize) -> Option<u32> {
            Some(0x1234abcd)
        }

        fn write_mem(&mut self, addr: u32, size: MemAccessSize, store_data: u32) -> bool {
            true
        }
    }

    #[test]
    fn test_memory_space() {
        let mem_1_vec = vec![0x11111111, 0x22222222];
        let mem_2_vec = vec![0x33333333, 0x44444444, 0x55555555];

        let mut test_mem_space = MemorySpace::new();

        assert_eq!(
            test_mem_space.add_memory(0x100, 8, Box::new(VecMemory::new(mem_1_vec))),
            Ok(0)
        );

        assert_eq!(
            test_mem_space.add_memory(0x200, 12, Box::new(VecMemory::new(mem_2_vec))),
            Ok(1)
        );

        assert_eq!(
            test_mem_space.add_memory(0x300, 0x100, Box::new(TestMemory {})),
            Ok(2)
        );

        assert_eq!(
            test_mem_space.add_memory(0x280, 0x100, Box::new(TestMemory {})),
            Err(MemorySpaceError::RegionOverlap)
        );

        assert_eq!(
            test_mem_space.add_memory(0x280, 0x80, Box::new(TestMemory {})),
            Ok(3)
        );

        assert_eq!(
            test_mem_space.add_memory(0x403, 0x100, Box::new(TestMemory {})),
            Err(MemorySpaceError::Unaligned)
        );

        assert_eq!(
            test_mem_space.add_memory(0x400, 0x103, Box::new(TestMemory {})),
            Err(MemorySpaceError::Unaligned)
        );

        assert!(test_mem_space.get_memory_ref::<VecMemory>(0).is_some());
        assert!(test_mem_space.get_memory_ref::<TestMemory>(0).is_none());
        assert!(test_mem_space.get_memory_mut::<TestMemory>(2).is_some());
        assert!(test_mem_space.get_memory_mut::<TestMemory>(1).is_none());

        assert_eq!(
            test_mem_space.read_mem(0x100, MemAccessSize::Word),
            Some(0x11111111)
        );

        assert_eq!(
            test_mem_space.read_mem(0x204, MemAccessSize::Word),
            Some(0x44444444)
        );

        assert_eq!(
            test_mem_space.write_mem(0x208, MemAccessSize::Word, 0xffffffff),
            true
        );

        assert_eq!(
            test_mem_space.write_mem(0x20c, MemAccessSize::Word, 0xffffffff),
            false
        );

        assert_eq!(test_mem_space.read_mem(0x108, MemAccessSize::Word), None);

        for i in 0..0x40 {
            assert_eq!(
                test_mem_space.read_mem(i * 4 + 0x300, MemAccessSize::Word),
                Some(0x1234abcd)
            );
        }

        assert_eq!(test_mem_space.read_mem(0x400, MemAccessSize::Word), None);

        assert_eq!(
            test_mem_space.get_memory_ref::<VecMemory>(1).unwrap().mem[2],
            0xffffffff
        );

        test_mem_space.get_memory_mut::<VecMemory>(0).unwrap().mem[0] = 0xdeadbeef;

        assert_eq!(
            test_mem_space.read_mem(0x100, MemAccessSize::Word),
            Some(0xdeadbeef)
        );
    }

    #[test]
    fn test_read_to_memory() {
        let test_bytes: Vec<u8> = (5..21).collect();
        let mut test_memory = VecMemory::new(vec![0; 4]);

        assert!(read_to_memory(test_bytes.as_slice(), &mut test_memory, 0).is_ok());

        for a in 0..16 {
            assert_eq!(test_memory.read_mem(a, MemAccessSize::Byte), Some(a + 5));
        }

        let test_bytes: Vec<u8> = (10..15).collect();

        assert!(read_to_memory(test_bytes.as_slice(), &mut test_memory, 5).is_ok());

        for a in 5..10 {
            assert_eq!(test_memory.read_mem(a, MemAccessSize::Byte), Some(a + 5));
        }

        assert_eq!(
            format!(
                "{:?}",
                read_to_memory(test_bytes.as_slice(), &mut test_memory, 13)
                    .unwrap_err()
                    .get_ref()
                    .unwrap()
            ),
            "\"Could not write byte at address 0x00000010\""
        );
    }
}
