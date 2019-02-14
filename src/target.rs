#![allow(non_camel_case_types)]

use crate::container;
use crate::Ctx;
use target_lexicon::{Triple, PointerWidth, Endianness};

pub fn make_ctx(target: &Triple) -> Ctx {
    let container_size = match target.pointer_width() {
        Err(()) |
        Ok(PointerWidth::U16) => return Ctx::default(),
        Ok(PointerWidth::U32) => container::Container::Little,
        Ok(PointerWidth::U64) => container::Container::Big,
    };
    let endianness = match target.endianness() {
        Err(()) => return Ctx::default(),
        Ok(Endianness::Little) => container::Endian::Little,
        Ok(Endianness::Big) => container::Endian::Big,
    };
    Ctx::new(container_size, endianness)
}
