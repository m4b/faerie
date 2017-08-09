use {error, Artifact, Object, Target, Code, Data, Ctx};

// use std::collections::HashMap;
// use std::fmt;
// use std::io::{Seek, Cursor, BufWriter, Write};
// use std::io::SeekFrom::*;
// use scroll::Lwrite;
// use shawshank;
// use ordermap::OrderMap;

// use goblin::mach::header::{self, Header};
// use goblin::mach::relocation::RelocationInfo;

pub struct Mach {
    ctx: Ctx,
    target: Target,
}

impl Mach {
    pub fn new(name: Option<String>, target: Target) -> Self {
        let ctx = Ctx::from(target.clone());
        let name = name.unwrap_or("goblin".to_owned());
        // let mut offsets = HashMap::new();
        // let mut strings = shawshank::string_arena_set();
        // let mut section_symbols = OrderMap::new();
        // let mut sizeof_strtab = 1;
        Mach {
            ctx,
            target,
        }
    }
}
