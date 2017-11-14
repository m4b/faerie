extern crate goblin;
extern crate scroll;
extern crate shawshank;
extern crate ordermap;
extern crate string_interner;
#[macro_use]
extern crate log;

use goblin::container;
pub use goblin::error as error;

type Ctx = container::Ctx;
pub type Code = Vec<u8>;
pub type Data = Vec<u8>;

mod target;
pub use target::Target;

pub mod elf;
pub use elf::Elf;

pub mod mach;
pub use mach::Mach;

pub mod artifact;
pub use artifact::{Object, Artifact, ArtifactBuilder, Link, ImportKind};

#[cfg(test)]
mod tests {

}
