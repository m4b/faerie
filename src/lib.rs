#![deny(missing_docs)]
//! Faerie is a crate for creating object files.

extern crate goblin;
extern crate indexmap;
extern crate scroll;
extern crate string_interner;
#[macro_use]
extern crate log;
extern crate target_lexicon;

use goblin::container;

type Ctx = container::Ctx;

mod elf;
mod mach;
mod target;

pub mod artifact;
pub use crate::artifact::{
    decl::{
        DataDecl, DataImportDecl, DataType, Decl, FunctionDecl, FunctionImportDecl, Scope,
        SectionDecl, SectionKind, Visibility,
    },
    Artifact, ArtifactBuilder, ArtifactError, Data, ImportKind, Link, Reloc,
};
