//! # bad64
//!
//! bad64 is a set of Rust bindings to the Binja Arm64 Disassembler.
//!
//! For more information about the disassembler, please see the
//! [upstream](https://github.com/Vector35/arch-arm64/tree/dev/disassembler)
//! repo.
//!
//! There are two main entry points:
//! 1. [`decode`] for decoding a single instruction.
//! ```
//! use bad64::{decode, Operation};
//! // nop - "\x1f\x20\x03\xd5"
//! let decoded = decode(0xd503201f, 0x1000).unwrap();
//!
//! assert_eq!(decoded.address(), 0x1000);
//! assert_eq!(decoded.num_operands(), 0);
//! assert_eq!(decoded.operation(), Operation::NOP);
//! assert_eq!(decoded.mnem(), "nop");
//! ```
//!
//! 2. [`disassemble`] for disassembling a byte sequence.
//! ```
//! use bad64::{disassemble, Operation, Operand, Reg, Imm};
//!
//! // 1000: str   x0, [sp, #-16]! ; "\xe0\x0f\x1f\xf8"
//! // 1004: ldr   x0, [sp], #16   ; "\xe0\x07\x41\xf8"
//! let mut decoded_iter = disassemble(b"\xe0\x0f\x1f\xf8\xe0\x07\x41\xf8", 0x1000);
//!
//! let push = decoded_iter.next().unwrap().unwrap();
//!
//! // check out the push
//! assert_eq!(push.address(), 0x1000);
//! assert_eq!(push.num_operands(), 2);
//! assert_eq!(push.operation(), Operation::STR);
//! assert_eq!(push.operand(0), Some(Operand::Reg { reg: Reg::X0, shift: None }));
//! assert_eq!(push.operand(1), Some(Operand::MemPreIdx { reg: Reg::SP, offset: 16 }));
//! assert_eq!(push.operand(2), None);
//!
//! let pop = decoded_iter.next().unwrap().unwrap();
//!
//! // check out the pop
//! assert_eq!(pop.address(), 0x1004);
//! assert_eq!(pop.num_operands(), 2);
//! assert_eq!(pop.operation(), Operation::LDR);
//! assert_eq!(
//!     pop.operand(0),
//!     Some(Operand::Reg { reg: Reg::X0, shift: None }));
//! assert_eq!(
//!     pop.operand(1),
//!     Some(Operand::MemPostIdxImm { reg: Reg::SP, imm: Imm { neg: false, val: 16 }}));
//! assert_eq!(pop.operand(2), None);
//!
//! // make sure there's nothing left
//! assert_eq!(decoded_iter.next(), None);
//! ```

#![no_std]

#[macro_use]
extern crate num_derive;

#[macro_use]
extern crate static_assertions;

use core::convert::{TryFrom, TryInto};
use core::iter;
use core::mem::MaybeUninit;

use cstr_core::CStr;
use num_traits::FromPrimitive;

use bad64_sys::*;

mod operand;
mod operation;
mod reg;
mod shift;
mod sysreg;

pub use operand::{Imm, Operand};
pub use operation::Operation;
pub use reg::Reg;
pub use shift::Shift;
pub use sysreg::SysReg;

/// A decoded instruction
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Instruction {
    address: u64,
    _inner: bad64_sys::Instruction
}

impl Instruction {
    /// Returns the instruction mnemonic
    ///
    /// # Example
    /// ```
    /// use bad64::decode;
    /// // nop - "\x1f\x20\x03\xd4"
    /// let decoded = decode(0xd503201f, 0x1000).unwrap();
    /// assert_eq!(decoded.mnem(), "nop");
    // ```
    pub fn mnem(&self) -> &'static str {
        unsafe { CStr::from_ptr(operation_to_str(self._inner.operation)) }
            .to_str()
            .unwrap()
    }

    /// Returns the instruction address
    ///
    /// # Example
    /// ```
    /// use bad64::decode;
    /// // nop - "\x1f\x20\x03\xd4"
    /// let decoded = decode(0xd503201f, 0x1000).unwrap();
    /// assert_eq!(decoded.address(), 0x1000);
    /// ```
    pub fn address(&self) -> u64 {
        self.address
    }

    /// Returns the instruction operation
    ///
    /// # Example
    /// ```
    /// use bad64::{decode, Operation};
    /// // nop - "\x1f\x20\x03\xd4"
    /// let decoded = decode(0xd503201f, 0x1000).unwrap();
    /// assert_eq!(decoded.operation(), Operation::NOP);
    // ```
    pub fn operation(&self) -> Operation {
        assert!(self._inner.operation != 0);

        Operation::from_u32(self._inner.operation as u32).unwrap()
    }

    /// Returns an instruction operand
    ///
    /// # Arguments
    ///
    /// * `n` - return's the nth operand
    ///
    /// # Example
    /// ```
    /// use bad64::{decode, Imm, Operation, Operand, Reg};
    /// // add x0, x1, #0x41  - "\x20\x04\x01\x91"
    /// let decoded = decode(0x91010420, 0x1000).unwrap();
    ///
    /// assert_eq!(decoded.operation(), Operation::ADD);
    /// assert_eq!(decoded.num_operands(), 3);
    /// assert_eq!(decoded.operand(0), Some(Operand::Reg { reg: Reg::X0, shift: None }));
    /// assert_eq!(decoded.operand(1), Some(Operand::Reg { reg: Reg::X1, shift: None }));
    /// assert_eq!(decoded.operand(2), Some(Operand::Imm64 { imm: Imm { neg: false, val: 0x41 }, shift: None }));
    /// assert_eq!(decoded.operand(3), None);
    // ```
    pub fn operand(&self, n: usize) -> Option<Operand> {
        if n >= MAX_OPERANDS as usize {
            return None;
        }

        Operand::try_from(&self._inner.operands[n]).ok()
    }

    /// Returns the operand count
    ///
    /// # Example
    /// ```
    /// use bad64::{decode, Operation};
    /// // eor x0, x1, x2  - "\x20\x00\x02\xca"
    /// let decoded = decode(0xca020020, 0x1000).unwrap();
    ///
    /// assert_eq!(decoded.num_operands(), 3);
    /// ```
    pub fn num_operands(&self) -> usize {
        for n in 0..MAX_OPERANDS as usize {
            if self.operand(n).is_none() {
                return n;
            }
        }

        MAX_OPERANDS as usize
    }

    /// Returns an iterator over the operands
    ///
    /// # Example
    /// ```
    /// use bad64::{decode, Operand, Reg};
    ///
    /// // eor x0, x1, x2  - "\x20\x00\x02\xca"
    /// let decoded = decode(0xca020020, 0x1000).unwrap();
    ///
    /// let mut op_iter = decoded.operands();
    ///
    /// assert_eq!(op_iter.next(), Some(Operand::Reg { reg: Reg::X0, shift: None }));
    /// assert_eq!(op_iter.next(), Some(Operand::Reg { reg: Reg::X1, shift: None }));
    /// assert_eq!(op_iter.next(), Some(Operand::Reg { reg: Reg::X2, shift: None }));
    /// assert_eq!(op_iter.next(), None);
    /// ```
    pub fn operands(&self) -> impl Iterator<Item = Operand> + '_ {
        let mut n = 0;
        iter::from_fn(move || {
            let ii = n;

            n += 1;

            self.operand(ii)
        })
    }
}

/// Decoding errors types
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq, FromPrimitive)]
#[repr(i32)]
pub enum DecodeError {
    Reserved = DECODE_STATUS_RESERVED,
    Unmatched = DECODE_STATUS_UNMATCHED,
    Unallocated = DECODE_STATUS_UNALLOCATED,
    Undefined = DECODE_STATUS_UNDEFINED,
    EndOfInstruction = DECODE_STATUS_END_OF_INSTRUCTION,
    Lost = DECODE_STATUS_LOST,
    Unreachable = DECODE_STATUS_UNREACHABLE,
    Short = -8, // bad64 introduced error code, for cases where a we dont have 4 bytes to decode
}

/// Decode a single instruction
///
/// # Arguments
///
/// * `ins` - A little endian u32 of code to be decoded
/// * `address` - Location of code in memory
///
/// # Examples
/// ```
/// use bad64::{decode, Operation};
///
/// // NOTE: little endian
/// let decoded = decode(0xd503201f, 0x1000).unwrap();
///
/// assert_eq!(decoded.num_operands(), 0);
/// assert_eq!(decoded.operands().next(), None);
/// assert_eq!(decoded.operation(), Operation::NOP);
/// assert_eq!(decoded.mnem(), "nop");
/// assert_eq!(decoded.address(), 0x1000);
/// ```
pub fn decode(ins: u32, address: u64) -> Result<Instruction, DecodeError> {
    let mut decoded = MaybeUninit::zeroed();

    let r = unsafe { aarch64_decompose(ins, decoded.as_mut_ptr(), address) };

    match r {
        0 => Ok(Instruction { address, _inner: unsafe { decoded.assume_init() }}),
        _ => Err(DecodeError::from_i32(r).unwrap()),
    }
}

/// Disassemble byte slice
///
/// # Arguments
///
/// * `code` - u8 slice to zero or more instructions
/// * `address` - Location of code in memory
///
/// # Examples
/// ```
/// use bad64::{disassemble, Operation};
///
/// let mut decoded_iter = disassemble(b"\x1f\x20\x03\xd5", 0x1000);
///
/// let decoded = decoded_iter.next().unwrap().unwrap();
///
/// assert_eq!(decoded.address(), 0x1000);
/// assert_eq!(decoded.num_operands(), 0);
/// assert_eq!(decoded.operation(), Operation::NOP);
/// assert_eq!(decoded.mnem(), "nop");
///
/// assert_eq!(decoded_iter.next(), None);
/// ```
pub fn disassemble(
    code: &[u8],
    address: u64,
) -> impl Iterator<Item = Result<Instruction, (u64, DecodeError)>> + '_ {
    (address..)
        .step_by(4)
        .zip(code.chunks(4))
        .map(|(addr, bytes)| match bytes.try_into() {
            Ok(v) => {
                let vv = u32::from_le_bytes(v);

                decode(vv, addr).map_err(|e| (addr, e))
            }
            Err(_) => Err((addr, DecodeError::Short)),
        })
}
