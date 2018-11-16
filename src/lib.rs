extern crate goblin;
extern crate scroll;
extern crate indexmap;
extern crate string_interner;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;
extern crate target_lexicon;

use goblin::container;

type Ctx = container::Ctx;

mod target;
mod elf;
mod mach;

pub mod artifact;
pub use artifact::{Artifact, ArtifactBuilder, Link, ImportKind, Decl, Reloc};
