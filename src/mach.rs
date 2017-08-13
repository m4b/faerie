#![allow(unused_variables)]
#![allow(dead_code)]

use {error, Artifact, Target, Object, Code, Data, Ctx};

//use ordermap::OrderMap;
use std::collections::HashMap;
use std::io::{Seek, Cursor, BufWriter, Write};
use std::io::SeekFrom::*;
use scroll::IOwrite;

use goblin::mach::cputype;
use goblin::mach::segment;
use goblin::mach::header::{Header, MH_OBJECT, MH_SUBSECTIONS_VIA_SYMBOLS};

type Section = segment::Section;
type Segment<'a> = segment::Segment<'a>;
// use goblin::mach::relocation::RelocationInfo;

struct CpuType(cputype::CpuType);

impl From<Target> for CpuType {
    fn from(target: Target) -> CpuType {
        use self::Target::*;
        use mach::cputype::*;
        CpuType(match target {
            X86_64 => CPU_TYPE_X86_64,
            X86 => CPU_TYPE_X86,
            ARM64 => CPU_TYPE_ARM64,
            ARMv7 => CPU_TYPE_ARM,
            Unknown => 0
        })
    }
}

pub struct Mach<'a> {
    ctx: Ctx,
    target: Target,
    code: HashMap<&'a str, Code>,
    data: HashMap<&'a str, Data>,
}

impl<'a> Mach<'a> {
    pub fn new(name: Option<String>, target: Target) -> Self {
        let ctx = Ctx::from(target.clone());
        let name = name.unwrap_or("goblin_mach".to_owned());
        let code = HashMap::new();
        let data = HashMap::new();
        Mach {
            ctx,
            target,
            code,
            data,
        }
    }
    fn header(&self) -> Header {
        let mut header = Header::new(&self.ctx);
        header.filetype = MH_OBJECT;
        // safe to divide up the sections into sub-sections via symbols for dead code stripping
        header.flags = MH_SUBSECTIONS_VIA_SYMBOLS;
        header.cputype = CpuType::from(self.target).0;
        header.cpusubtype = 3;
        header
    }
    pub fn write<T: Write + Seek>(self, file: T) -> error::Result<()> {
        let mut file = BufWriter::new(file);
        let mut header = self.header();
        header.ncmds = 1;
        file.iowrite_with(header, self.ctx)?;
        file.iowrite_with(Segment::new(self.ctx, &[]), self.ctx)?;
        let after_header = file.seek(Current(0))?;
        Ok(())
    }
}

impl<'a> Object for Mach<'a> {
    fn to_bytes(artifact: &Artifact) -> error::Result<Vec<u8>> {
        let mach = Mach::new(Some(artifact.name.to_string()), artifact.target.clone());
        let mut buffer = Cursor::new(Vec::new());
        mach.write(&mut buffer)?;
        Ok(buffer.into_inner())
    }
}
