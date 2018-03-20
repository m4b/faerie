extern crate goblin;
extern crate scroll;
extern crate indexmap;
extern crate string_interner;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;

use goblin::container;

type Ctx = container::Ctx;

mod target;
pub use target::Target;

pub mod elf;
pub use elf::Elf;

pub mod mach;
pub use mach::Mach;

pub mod artifact;
pub use artifact::{Object, Artifact, ArtifactBuilder, Link, ImportKind, Decl, RelocOverride};
