const PAGE_SIZE: usize = 16 * 1024;
const SIZE_16K: usize = PAGE_SIZE;
const SIZE_32K: usize = PAGE_SIZE * 2;
const SIZE_48K: usize = PAGE_SIZE * 3;
const SIZE_64K: usize = PAGE_SIZE * 4;
const SIZE_128K: usize = PAGE_SIZE * 8;
const MEM_BLOCKS: usize = 4;

/// Rom type enum
pub enum RomType {
    K16,
    K32,
    K64,
}

/// Ram type enum
pub enum RamType {
    K16,
    K48,
    K128,
}

// Page info and type
#[derive(Clone, Copy)]
pub enum Page {
    Ram(u8),
    Rom(u8),
}

// Memory struct
pub struct ZXMemory {
    rom: Vec<u8>,
    ram: Vec<u8>,
    // 4 x 16K blocks  map
    map: [Page; 4],
}

impl ZXMemory {
    /// Returns new Memory with coresponding rom and ram types
    pub fn new(rom_type: RomType, ram_type: RamType) -> ZXMemory {
        let ram_size;
        let mem_map;
        match ram_type {
            RamType::K16 => {
                ram_size = SIZE_16K;
                mem_map = [Page::Rom(0), Page::Ram(0), Page::Ram(0), Page::Ram(0)];
            }
            RamType::K48 => {
                ram_size = SIZE_48K;
                mem_map = [Page::Rom(0), Page::Ram(0), Page::Ram(1), Page::Ram(2)];
            }
            RamType::K128 => {
                ram_size = SIZE_128K;
                mem_map = [Page::Rom(0), Page::Ram(5), Page::Ram(2), Page::Ram(0)];
            }
        };
        let rom_size = match rom_type {
            RomType::K16 => SIZE_16K,
            RomType::K32 => SIZE_32K,
            RomType::K64 => SIZE_64K,
        };
        ZXMemory {
            rom: vec![0; rom_size],
            ram: vec![0; ram_size],
            map: mem_map,
        }
    }

    /// Returns value form memory
    pub fn read(&self, addr: u16) -> u8 {
        let page = self.map[(addr as usize) / PAGE_SIZE];
        let addr_rel = addr as usize % PAGE_SIZE;
        match page {
            Page::Rom(page) => self.rom[(page as usize) * PAGE_SIZE + addr_rel],
            Page::Ram(page) => self.ram[(page as usize) * PAGE_SIZE + addr_rel],
        }
    }

    /// Writes value to memory
    pub fn write(&mut self, addr: u16, value: u8) {
        let page = self.map[(addr as usize) / PAGE_SIZE];
        let addr_rel = addr as usize % PAGE_SIZE;
        match page {
            Page::Ram(page) => {
                self.ram[(page as usize) * PAGE_SIZE + addr_rel] = value;
            }
            _ => {}
        };
    }

    /// Changes memory map
    pub fn remap(&mut self, block: usize, page: Page) -> Result<(), ()> {
        if block < MEM_BLOCKS {
            match page {
                Page::Ram(page) if (page as usize + 1) * PAGE_SIZE > self.ram.len() => Err(()),
                Page::Rom(page) if (page as usize + 1) * PAGE_SIZE > self.rom.len() => Err(()),
                _ => {
                    self.map[block] = page;
                    Ok(())
                }
            }
        } else {
            Err(())
        }
    }

    /// Loads ROM from array slice to memory
    /// TODO: make "upload_page" fuction, allow to load not only rom's
    pub fn load_rom(&mut self, page: u8, data: &[u8]) -> Result<(), ()> {
        if (page as usize + 1) * PAGE_SIZE > self.rom.len() {
            Err(())
        } else {
            let shift = page as usize * PAGE_SIZE;
            let mut slice = &mut self.rom[shift..shift + PAGE_SIZE];
            slice[..data.len()].clone_from_slice(data);
            Ok(())
        }
    }

    /// Dumps current address space
    pub fn dump(&self) -> Vec<u8> {
        let mut out = self.rom.clone();
        let mut ram = self.ram.clone();
        out.append(&mut ram);
        out
    }
}
