extern crate goblin;
extern crate scroll;
extern crate shawshank;
extern crate ordermap;
#[macro_use]
extern crate log;

pub use goblin::container::{self, Ctx};
pub use goblin::error as error;

pub type Code = Vec<u8>;
pub type Data = Vec<u8>;

mod target;
pub use target::Target;

pub mod elf;
pub use elf::Elf;

pub mod artifact;
pub use artifact::{Object, Artifact};

#[cfg(test)]
mod tests {

}
