use super::*;
use z80::*;

/// Rotate operations (RLC, RRC, RL, RR, SLA, SRA, SLL, SRL)
/// returns result (can be useful with DDCB/FDCB instructions)
pub fn execute_rot(cpu: &mut Z80, bus: &mut Z80Bus, rot_code: U3, operand: RotOperand8) -> u8 {
    // get byte which will be rotated
    let mut data = match operand {
        RotOperand8::Indirect(addr) => bus.read(addr),
        RotOperand8::Reg(reg) => cpu.regs.get_reg_8(reg),
    };
    let (sign, zero, f5, f3, half_carry, pv, sub, carry);
    match rot_code {
        // RLC
        U3::N0 => {
            // get msb
            carry = (data & 0x80) != 0;
            // shift left and clear lowerest bit
            data = (data << 1) & 0xFE;
            // set lsb if msb was set
            if carry {
                data |= 0x01;
            };
        }
        // RRC
        U3::N1 => {
            // get lsb
            carry = (data & 0x01) != 0;
            // shift left and clear highest bit
            data = (data >> 1) & 0x7F;
            // set lsb if msb was set
            if carry {
                data |= 0x80;
            };
        }
        // RL
        U3::N2 => {
            // get msb
            carry = (data & 0x80) != 0;
            // shift left and clear lowerest bit
            data = (data << 1) & 0xFE;
            // set lsb if msb was set
            if cpu.regs.get_flag(Flag::Carry) {
                data |= 0x01;
            };
        }
        // RR
        U3::N3 => {
            // get lsb
            carry = (data & 0x01) != 0;
            // shift left and clear highest bit
            data = (data >> 1) & 0x7F;
            // set lsb if msb was set
            if cpu.regs.get_flag(Flag::Carry) {
                data |= 0x80;
            };
        }
        // SLA
        U3::N4 => {
            // get msb
            carry = (data & 0x80) != 0;
            // shift left and clear lowerest bit
            data = (data << 1) & 0xFE;
        }
        // SRA
        U3::N5 => {
            // get lsb
            carry = (data & 0x01) != 0;
            // shift left and leave highest bit unchanged
            data = ((data >> 1) & 0x7F) | (data & 0x80);
        }
        // SLL
        U3::N6 => {
            // get msb
            carry = (data & 0x80) != 0;
            // shift left and set lowerest bit
            data = (data << 1) | 0x01;
        }
        // SRL
        U3::N7 => {
            // get lsb
            carry = (data & 0x01) != 0;
            // shift left and leave highest bit unchanged
            data = (data >> 1) & 0x7F;
        }
    };
    zero = data == 0;
    sign = (data & 0x80) != 0;
    half_carry = true;
    pv = tables::PARITY_BIT[data as usize] != 0;
    sub = false;
    f3 = data & 0b1000 != 0;
    f5 = data & 0b100000 != 0;
    // write result
    match operand {
        RotOperand8::Indirect(addr) => {
            bus.write(addr, data);
        }
        RotOperand8::Reg(reg) => {
            cpu.regs.set_reg_8(reg, data);
        }
    };
    cpu.regs.set_flag(Flag::Carry, carry);
    cpu.regs.set_flag(Flag::Sub, sub);
    cpu.regs.set_flag(Flag::ParityOveflow, pv);
    cpu.regs.set_flag(Flag::F3, f3);
    cpu.regs.set_flag(Flag::HalfCarry, half_carry);
    cpu.regs.set_flag(Flag::F5, f5);
    cpu.regs.set_flag(Flag::Zero, zero);
    cpu.regs.set_flag(Flag::Sign, sign);
    data
}
