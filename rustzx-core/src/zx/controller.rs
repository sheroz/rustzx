//! Contains ZX Spectrum System contrller (like ula or so) of emulator
use crate::{
    host::{Host, HostContext},
    settings::RustzxSettings,
    utils::{screen::bitmap_line_addr, split_word, Clocks},
    z80::Z80Bus,
    zx::{
        constants::{ADDR_LD_BREAK, CANVAS_HEIGHT, CLOCKS_PER_COL},
        events::EmulationEvents,
        joy::kempston::KempstonJoy,
        keys::ZXKey,
        machine::ZXMachine,
        memory::{Page, RamType, RomType, ZXMemory, PAGE_SIZE},
        tape::{Tap, TapeImpl, ZXTape},
        video::{colors::ZXColor, screen::ZXScreen},
    },
};

#[cfg(feature = "embedded-roms")]
use crate::zx::roms;
#[cfg(feature = "sound")]
use crate::zx::sound::mixer::ZXMixer;
#[cfg(feature = "precise-border")]
use crate::zx::video::border::ZXBorder;

/// ZX System controller
pub(crate) struct ZXController<H: Host> {
    // parts of ZX Spectum.
    pub machine: ZXMachine,
    pub memory: ZXMemory,
    pub screen: ZXScreen<H::FrameBuffer>,
    pub tape: ZXTape,
    #[cfg(feature = "precise-border")]
    pub border: ZXBorder<H::FrameBuffer>,
    pub kempston: Option<KempstonJoy>,
    #[cfg(feature = "sound")]
    pub mixer: ZXMixer,
    pub keyboard: [u8; 8],
    // current border color
    pub border_color: ZXColor,
    // clocls count from frame start
    frame_clocks: Clocks,
    // frames count, which passed during emulation invokation
    passed_frames: usize,
    events: EmulationEvents,
    // audio in
    mic: bool,
    // audio out
    ear: bool,
    paging_enabled: bool,
    screen_bank: u8,
}

impl<H: Host> ZXController<H> {
    /// Returns new ZXController from settings
    pub fn new(settings: &RustzxSettings, host_context: H::Context) -> Self {
        let (memory, paging, screen_bank);
        match settings.machine {
            ZXMachine::Sinclair48K => {
                memory = ZXMemory::new(RomType::K16, RamType::K48);
                paging = false;
                screen_bank = 0;
            }
            ZXMachine::Sinclair128K => {
                memory = ZXMemory::new(RomType::K32, RamType::K128);
                paging = true;
                screen_bank = 5;
            }
        };
        let kempston = if settings.enable_kempston {
            Some(KempstonJoy::default())
        } else {
            None
        };

        let screen = ZXScreen::new(settings.machine, host_context.frame_buffer_context());
        #[cfg(feature = "precise-border")]
        let border = ZXBorder::new(settings.machine, host_context.frame_buffer_context());

        #[cfg(feature = "sound")]
        let mixer = Self::create_mixer(settings);

        let out = ZXController {
            machine: settings.machine,
            memory,
            screen,
            #[cfg(feature = "precise-border")]
            border,
            kempston,
            #[cfg(feature = "sound")]
            mixer,
            keyboard: [0xFF; 8],
            border_color: ZXColor::Black,
            frame_clocks: Clocks(0),
            passed_frames: 0,
            tape: Tap::default().into(),
            events: Default::default(),
            mic: false,
            ear: false,
            paging_enabled: paging,
            screen_bank,
        };

        #[cfg(feature = "embedded-roms")]
        if settings.load_default_rom {
            let mut out = out;
            out.load_default_rom();
            return out;
        }

        out
    }

    #[cfg(feature = "sound")]
    fn create_mixer(settings: &RustzxSettings) -> ZXMixer {
        let mut mixer = ZXMixer::new(
            settings.beeper_enabled,
            #[cfg(feature = "ay")]
            settings.ay_enabled,
            settings.sound_sample_rate,
        );
        #[cfg(feature = "ay")]
        mixer.ay.mode(settings.ay_mode);
        mixer.volume(settings.sound_volume as f64 / 200.0);
        mixer
    }

    /// returns current frame emulation pos in percents
    fn frame_pos(&self) -> f64 {
        let val = self.frame_clocks.count() as f64 / self.machine.specs().clocks_frame as f64;
        if val > 1.0 {
            1.0
        } else {
            val
        }
    }

    /// loads builted-in ROM
    #[cfg(feature = "embedded-roms")]
    fn load_default_rom(&mut self) {
        match self.machine {
            ZXMachine::Sinclair48K => {
                let page = self.memory.rom_page_data_mut(0);
                page.copy_from_slice(roms::ROM_48K);
            }
            ZXMachine::Sinclair128K => {
                let page = self.memory.rom_page_data_mut(0);
                page.copy_from_slice(roms::ROM_128K_0);
                let page = self.memory.rom_page_data_mut(1);
                page.copy_from_slice(roms::ROM_128K_1);
            }
        }
    }

    /// Changes key state in controller
    pub fn send_key(&mut self, key: ZXKey, pressed: bool) {
        let row_id = key.row_id();
        self.keyboard[row_id] &= !key.mask();
        if !pressed {
            self.keyboard[row_id] |= key.mask();
        }
    }

    /// Returns current bus floating value
    fn floating_bus_value(&self) -> u8 {
        let specs = self.machine.specs();
        let clocks = self.frame_clocks;
        if clocks.count() < specs.clocks_first_pixel + 2 {
            return 0xFF;
        }
        let clocks = clocks.count() - (specs.clocks_first_pixel + 2);
        let row = clocks / specs.clocks_line;
        let clocks = clocks % specs.clocks_line;
        let col = (clocks / 8) * 2 + (clocks % 8) / 2;
        if row < CANVAS_HEIGHT
            && clocks < specs.clocks_screen_row - CLOCKS_PER_COL
            && ((clocks & 0x04) == 0)
        {
            if clocks % 2 == 0 {
                return self.memory.read(bitmap_line_addr(row) + col as u16);
            } else {
                let byte = (row / 8) * 32 + col;
                return self.memory.read(0x5800 + byte as u16);
            };
        }
        0xFF
    }

    /// make contention
    fn do_contention(&mut self) {
        let contention = self.machine.contention_clocks(self.frame_clocks);
        self.wait_internal(contention);
    }

    /// make contention + wait some clocks
    fn do_contention_and_wait(&mut self, wait_time: Clocks) {
        let contention = self.machine.contention_clocks(self.frame_clocks);
        self.wait_internal(contention + wait_time);
    }

    // check addr contention
    fn addr_is_contended(&self, addr: u16) -> bool {
        if let Page::Ram(bank) = self.memory.get_page(addr) {
            self.machine.bank_is_contended(bank as usize)
        } else {
            false
        }
    }

    /// Returns early IO contention clocks
    fn io_contention_first(&mut self, port: u16) {
        if self.addr_is_contended(port) {
            self.do_contention();
        };
        self.wait_internal(Clocks(1));
    }

    /// Returns late IO contention clocks
    fn io_contention_last(&mut self, port: u16) {
        if self.machine.port_is_contended(port) {
            self.do_contention_and_wait(Clocks(2));
        } else if self.addr_is_contended(port) {
            self.do_contention_and_wait(Clocks(1));
            self.do_contention_and_wait(Clocks(1));
            self.do_contention();
        } else {
            self.wait_internal(Clocks(2));
        }
    }

    /// Starts a new frame
    fn new_frame(&mut self) {
        self.frame_clocks -= self.machine.specs().clocks_frame;
        self.screen.new_frame();
        #[cfg(feature = "precise-border")]
        self.border.new_frame();
        #[cfg(feature = "sound")]
        self.mixer.new_frame();
    }

    /// Clears all detected
    pub fn clear_events(&mut self) {
        self.events.clear();
    }

    /// Returns last events
    pub fn events(&self) -> EmulationEvents {
        self.events
    }

    /// Returns true if all frame clocks has been passed
    pub fn frames_count(&self) -> usize {
        self.passed_frames
    }

    pub fn reset_frame_counter(&mut self) {
        self.passed_frames = 0;
    }

    /// Returns current clocks from frame start
    pub fn clocks(&self) -> Clocks {
        self.frame_clocks
    }

    fn write_7ffd(&mut self, val: u8) {
        if !self.paging_enabled {
            return;
        }
        // remap top 16K of the ram
        self.memory.remap(3, Page::Ram(val & 0x07));
        // third block is not pageable
        // second block is screen buffer, not pageable. but we need to change active buffer
        let new_screen_bank = if val & 0x08 == 0 { 5 } else { 7 };
        self.screen.switch_bank(new_screen_bank as usize);
        self.screen_bank = new_screen_bank;
        // remap ROM
        self.memory.remap(0, Page::Rom((val >> 4) & 0x01));
        // check paging allow bit
        if val & 0x20 != 0 {
            self.paging_enabled = false;
        }
    }

    #[cfg(all(feature = "sound", feature = "ay"))]
    fn read_ay_port(&mut self) -> u8 {
        self.mixer.ay.read()
    }

    #[cfg(not(all(feature = "sound", feature = "ay")))]
    fn read_ay_port(&mut self) -> u8 {
        self.floating_bus_value()
    }

    #[cfg(all(feature = "sound", feature = "ay"))]
    fn write_ay_port(&mut self, value: u8) {
        self.mixer.ay.write(value);
    }

    #[cfg(not(all(feature = "sound", feature = "ay")))]
    fn write_ay_port(&mut self, _: u8) {}

    #[cfg(all(feature = "sound", feature = "ay"))]
    fn select_ay_reg(&mut self, value: u8) {
        self.mixer.ay.select_reg(value)
    }

    #[cfg(not(all(feature = "sound", feature = "ay")))]
    fn select_ay_reg(&mut self, _: u8) {}

    pub(crate) fn set_border_color(
        &mut self,
        #[allow(unused_variables)] clocks: Clocks,
        color: ZXColor,
    ) {
        self.border_color = color;
        #[cfg(feature = "precise-border")]
        self.border.set_border(clocks, color);
    }
}

impl<H: Host> Z80Bus for ZXController<H> {
    /// we need to check different breakpoints like tape
    /// loading detection breakpoint
    fn pc_callback(&mut self, addr: u16) {
        // check mapped memory page at 0x0000 .. 0x3FFF
        let check_fast_load = match self.machine {
            ZXMachine::Sinclair48K if self.memory.get_bank_type(0) == Page::Rom(0) => true,
            ZXMachine::Sinclair128K if self.memory.get_bank_type(0) == Page::Rom(1) => true,
            _ => false,
        };
        if check_fast_load {
            // Tape LOAD/VERIFY
            if addr == ADDR_LD_BREAK {
                // Add event (Fast tape loading request) it must be executed
                // by emulator immediately
                self.events |= EmulationEvents::TAPE_FAST_LOAD_TRIGGER_DETECTED;
            }
        }
    }

    /// read data without taking onto account contention
    fn read_internal(&mut self, addr: u16) -> u8 {
        self.memory.read(addr)
    }

    /// write data without taking onto account contention
    fn write_internal(&mut self, addr: u16, data: u8) {
        self.memory.write(addr, data);
        // if ram then compare bank to screen bank
        if let Page::Ram(bank) = self.memory.get_page(addr) {
            self.screen
                .update(addr % PAGE_SIZE as u16, bank as usize, data);
        }
    }

    /// Cahnges internal state on clocks count change (emualtion processing)
    fn wait_internal(&mut self, clk: Clocks) {
        self.frame_clocks += clk;
        self.tape.process_clocks(clk);
        let mic = self.tape.current_bit();
        self.mic = mic;
        #[cfg(feature = "sound")]
        {
            let pos = self.frame_pos();
            self.mixer.beeper.change_bit(self.mic | self.ear);
            self.mixer.process(pos);
        }
        self.screen.process_clocks(self.frame_clocks);
        if self.frame_clocks.count() >= self.machine.specs().clocks_frame {
            self.new_frame();
            self.passed_frames += 1;
        }
    }

    // wait with memory request pin active
    fn wait_mreq(&mut self, addr: u16, clk: Clocks) {
        match self.machine {
            ZXMachine::Sinclair48K | ZXMachine::Sinclair128K => {
                // contention in low 16k RAM
                if self.addr_is_contended(addr) {
                    self.do_contention();
                }
            }
        }
        self.wait_internal(clk);
    }

    /// wait without memory request pin active
    fn wait_no_mreq(&mut self, addr: u16, clk: Clocks) {
        // only for 48 K!
        self.wait_mreq(addr, clk);
    }

    /// read io from hardware
    fn read_io(&mut self, port: u16) -> u8 {
        // all contentions check
        self.io_contention_first(port);
        self.io_contention_last(port);
        // find out what we need to do
        let (h, _) = split_word(port);
        let output = if port & 0x0001 == 0 {
            // ULA port
            let mut tmp: u8 = 0xFF;
            for n in 0..8 {
                // if bit of row reset
                if ((h >> n) & 0x01) == 0 {
                    tmp &= self.keyboard[n];
                }
            }
            // invert bit 6 if mic_hw active;
            if self.mic {
                tmp ^= 0x40;
            }
            // 5 and 7 unused
            tmp
        } else if port & 0xC002 == 0xC000 {
            self.read_ay_port()
        } else if self.kempston.is_some() && (port & 0x0020 == 0) {
            if let Some(ref joy) = self.kempston {
                joy.read()
            } else {
                unreachable!()
            }
        } else {
            self.floating_bus_value()
        };
        // add one clock after operation
        self.wait_internal(Clocks(1));
        output
    }

    /// write value to hardware port
    fn write_io(&mut self, port: u16, data: u8) {
        // first contention
        self.io_contention_first(port);
        // find active port
        if port & 0xC002 == 0xC000 {
            self.select_ay_reg(data);
        } else if port & 0xC002 == 0x8000 {
            self.write_ay_port(data);
        } else if port & 0x0001 == 0 {
            self.set_border_color(self.frame_clocks, ZXColor::from_bits(data & 0x07));
            self.mic = data & 0x08 != 0;
            self.ear = data & 0x10 != 0;
            #[cfg(feature = "sound")]
            self.mixer.beeper.change_bit(self.mic | self.ear);
        } else if (port & 0x8002 == 0) && (self.machine == ZXMachine::Sinclair128K) {
            self.write_7ffd(data);
        }
        // last contention after byte write
        self.io_contention_last(port);
        // add one clock after operation
        self.wait_internal(Clocks(1));
    }

    /// value, requested during `INT0` interrupt
    fn read_interrupt(&mut self) -> u8 {
        0xFF
    }

    /// checks system maskable interrupt pin state
    fn int_active(&self) -> bool {
        self.frame_clocks.count() % self.machine.specs().clocks_frame
            < self.machine.specs().interrupt_length
    }

    /// checks non-maskable interrupt pin state
    fn nmi_active(&self) -> bool {
        false
    }

    /// CPU calls it when RETI instruction was processed
    fn reti(&mut self) {}

    /// CPU calls when was being halted
    fn halt(&mut self, _: bool) {}
}
