extern crate goblin;
extern crate indexmap;
extern crate scroll;
extern crate string_interner;
#[macro_use]
extern crate log;
#[macro_use]
extern crate failure;
extern crate target_lexicon;

use goblin::container;

type Ctx = container::Ctx;

mod elf;
mod mach;
mod target;

pub mod artifact;
pub use crate::artifact::{
    decl::{
        CStringDecl, DataDecl, DataImportDecl, DebugSectionDecl, Decl, FunctionDecl,
        FunctionImportDecl,
    },
    Artifact, ArtifactBuilder, ImportKind, Link, Reloc,
};
