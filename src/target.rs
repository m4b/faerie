#![allow(non_camel_case_types)]

use container;
use Ctx;

#[derive(Debug, Copy, Clone)]
pub enum Target {
    X86_64,
    X86,
    ARM64,
    ARMv7,
    Unknown,
}

impl From<Target> for Ctx {
    fn from(target: Target) -> Self {
        use self::Target::*;
        match target {
            X86_64 => Ctx::new(container::Container::Big,   container::Endian::Little),
            X86 => Ctx::new(container::Container::Little,   container::Endian::Little),
            ARM64 => Ctx::new(container::Container::Big,    container::Endian::Little),
            ARMv7 => Ctx::new(container::Container::Little, container::Endian::Little),
            Unknown => Ctx::default(),
        }
    }
}
