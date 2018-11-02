//! The Mach 32/64 bit backend for transforming an artifact to a valid, mach-o object file.

use {Artifact, Ctx};
use artifact::{Decl, Definition};
use target::make_ctx;

use failure::Error;
use indexmap::IndexMap;
use string_interner::{DefaultStringInterner};
//use std::collections::HashMap;
use std::io::{Seek, Cursor, BufWriter, Write};
use std::io::SeekFrom::*;
use scroll::{Pwrite, IOwrite};
use scroll::ctx::SizeWith;
use target_lexicon::Architecture;

use goblin::mach::cputype;
use goblin::mach::segment::{Section, Segment};
use goblin::mach::load_command::SymtabCommand;
use goblin::mach::header::{Header, MH_OBJECT, MH_SUBSECTIONS_VIA_SYMBOLS};
use goblin::mach::symbols::Nlist;
use goblin::mach::relocation::{RelocationInfo, RelocType, SIZEOF_RELOCATION_INFO};
use goblin::mach::constants::{S_REGULAR, S_CSTRING_LITERALS, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS};

struct CpuType(cputype::CpuType);

impl From<Architecture> for CpuType {
    fn from(architecture: Architecture) -> CpuType {
        use target_lexicon::Architecture::*;
        use mach::cputype::*;
        CpuType(match architecture {
            X86_64 => CPU_TYPE_X86_64,
            I386 |
            I586 |
            I686 => CPU_TYPE_X86,
            Aarch64 => CPU_TYPE_ARM64,
            Arm |
            Armv4t |
            Armv5te |
            Armv7 |
            Armv7s |
            Thumbv6m |
            Thumbv7em |
            Thumbv7m => CPU_TYPE_ARM,
            Sparc => CPU_TYPE_SPARC,
            Powerpc  => CPU_TYPE_POWERPC,
            Powerpc64 |
            Powerpc64le => CPU_TYPE_POWERPC64,
            Unknown => 0,
            _ => panic!("requested architecture does not exist in MachO"),
        })
    }
}

type SectionIndex = usize;
type StrtableOffset = u64;

const CODE_SECTION_INDEX: SectionIndex = 0;
const DATA_SECTION_INDEX: SectionIndex = 1;
const CSTRING_SECTION_INDEX: SectionIndex = 2;

/// A builder for creating a 32/64 bit Mach-o Nlist symbol
#[derive(Debug)]
struct SymbolBuilder {
    name: StrtableOffset,
    section: Option<SectionIndex>,
    global: bool,
    import: bool,
    offset: u64,
    segment_relative_offset: u64,
}

impl SymbolBuilder {
    /// Create a new symbol with `typ`
    pub fn new(name: StrtableOffset) -> Self {
        SymbolBuilder {
            name,
            section: None,
            global: false,
            import: false,
            offset: 0,
            segment_relative_offset: 0,
        }
    }
    /// The section this symbol belongs to
    pub fn section(mut self, section_index: SectionIndex) -> Self {
        self.section = Some(section_index); self
    }
    /// Is this symbol global?
    pub fn global(mut self, global: bool) -> Self {
        self.global = global; self
    }
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset; self
    }
    /// Set the segment relative offset of this symbol, required for relocations
    pub fn relative_offset(mut self, relative_offset: u64) -> Self {
        self.segment_relative_offset = relative_offset; self
    }
    /// Returns the offset of this symbol relative to the segment it is apart of
    pub fn get_segment_relative_offset(&self) -> u64 {
        self.segment_relative_offset
    }
    /// Is this symbol an import?
    pub fn import(mut self) -> Self {
        self.import = true; self
    }
    /// Finalize and create the symbol
    /// The n_value (offset into section) is still unset, and needs to be generated by the client
    pub fn create(self) -> Nlist {
        use goblin::mach::symbols::{N_EXT, N_UNDF, N_SECT, NO_SECT};
        let n_strx = self.name;
        let mut n_sect = 0;
        let mut n_type = N_UNDF;
        let mut n_value = self.offset;
        let n_desc = 0;
        if self.global {
            n_type |= N_EXT;
        } else {
            n_type &= !N_EXT;
        }
        if let Some(idx) = self.section {
            n_sect = idx + 1; // add 1 because n_sect expects ordinal
            n_type |= N_SECT;
        }

        if self.import {
            n_sect = NO_SECT as usize;
            // FIXME: this is broken i believe; we need to make it both undefined + global for imports
            n_type = N_EXT;
            n_value = 0;
        } else {
            n_type |= N_SECT;
        }

        Nlist {
            n_strx: n_strx as usize,
            n_type,
            n_sect,
            n_desc,
            n_value
        }
    }
}

/// An index into the symbol table
type SymbolIndex = usize;

/// Mach relocation builder
#[derive(Debug)]
struct RelocationBuilder {
    symbol: SymbolIndex,
    relocation_offset: u64,
    absolute: bool,
    r_type: RelocType,
}

impl RelocationBuilder {
    /// Create a relocation for `symbol`, starting at `relocation_offset`
    pub fn new(symbol: SymbolIndex, relocation_offset: u64, r_type: RelocType) -> Self {
        RelocationBuilder {
            symbol,
            relocation_offset,
            absolute: false,
            r_type,
        }
    }
    /// This is an absolute relocation
    pub fn absolute(mut self) -> Self {
        self.absolute = true; self
    }
    /// Finalize and create the relocation
    pub fn create(self) -> RelocationInfo {
        // it basically goes sort of backwards than what you'd expect because C bitfields are bonkers
        let r_symbolnum: u32 = self.symbol as u32;
        let r_pcrel: u32 = if self.absolute { 0 } else { 1 } << 24;
        let r_length: u32 = if self.absolute { 3 } else { 2 } << 25;
        let r_extern: u32 = 1 << 27;
        let r_type = (self.r_type as u32) << 28;
        // r_symbolnum, 24 bits, r_pcrel 1 bit, r_length 2 bits, r_extern 1 bit, r_type 4 bits
        let r_info = r_symbolnum | r_pcrel | r_length | r_extern | r_type;
        RelocationInfo {
            r_address: self.relocation_offset as i32,
            r_info,
        }
    }
}

/// Helper to build sections
#[derive(Debug, Clone)]
struct SectionBuilder {
    addr: u64,
    align: u64,
    offset: u64,
    size: u64,
    flags: u32,
    sectname: &'static str,
    segname: &'static str,
}

impl SectionBuilder {
    /// Create a new section builder with `sectname`, `segname` and `size`
    pub fn new(sectname: &'static str, segname: &'static str, size: u64) -> Self {
        SectionBuilder {
            addr: 0,
            align: 4,
            offset: 0,
            flags: S_REGULAR,
            size,
            sectname,
            segname,
        }
    }
    /// Set the vm address of this section
    pub fn addr(mut self, addr: u64) -> Self {
        self.addr = addr; self
    }
    /// Set the file offset of this section
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset; self
    }
    /// Set the alignment of this section
    pub fn align(mut self, align: u64) -> Self {
        self.align = align; self
    }
    /// Set the flags of this section
    pub fn flags(mut self, flags: u32) -> Self {
        self.flags = flags; self
    }
    /// Finalize and create the actual Mach-o section
    pub fn create(self) -> Section {
        let mut sectname = [0u8; 16];
        sectname.pwrite(&self.sectname, 0).unwrap();
        let mut segname = [0u8; 16];
        segname.pwrite(&self.segname, 0).unwrap();
        Section {
            sectname,
            segname,
            addr: self.addr,
            size: self.size,
            offset: self.offset as u32,
            align: self.align as u32,
            // FIXME, client needs to set after all offsets known
            reloff: 0,
            nreloc: 0,
            flags: self.flags
        }
    }
}

type ArtifactCode<'a> = Vec<Definition<'a>>;
type ArtifactData<'a> = Vec<Definition<'a>>;

type StrTableIndex = usize;
type StrTable = DefaultStringInterner;
type Symbols = IndexMap<StrTableIndex, SymbolBuilder>;
type Relocations = Vec<Vec<RelocationInfo>>;

/// A mach object symbol table
#[derive(Debug, Default)]
struct SymbolTable {
    symbols: Symbols,
    strtable: StrTable,
    indexes: IndexMap<StrTableIndex, SymbolIndex>,
    strtable_size: StrtableOffset,
}

/// The kind of symbol this is
enum SymbolType {
    /// Which `section` this is defined in, the `absolute_offset` in the binary, and its
    /// `segment_relative_offset`
    Defined { section: SectionIndex, absolute_offset: u64, segment_relative_offset: u64, global: bool },
    /// An undefined symbol (an import)
    Undefined,
}

impl SymbolTable {
    /// Create a new symbol table. The first strtable entry (like ELF) is always nothing
    pub fn new() -> Self {
        let mut strtable = StrTable::default();
        strtable.get_or_intern("");
        let strtable_size = 1;
        SymbolTable {
            symbols: Symbols::new(),
            strtable,
            strtable_size,
            indexes: IndexMap::new(),
        }
    }
    /// The number of symbols in this table
    pub fn len(&self) -> usize {
        self.symbols.len()
    }
    /// Returns size of the string table, in bytes
    pub fn sizeof_strtable(&self) -> u64 {
        self.strtable_size
    }
    /// Lookup this symbols offset in the segment
    pub fn offset(&self, symbol_name: &str) -> Option<u64> {
        self.strtable.get(symbol_name)
         .and_then(|idx| self.symbols.get(&idx))
         .and_then(|sym| Some(sym.get_segment_relative_offset()))
    }
    /// Lookup this symbols ordinal index in the symbol table, if it has one
    pub fn index(&self, symbol_name: &str) -> Option<SymbolIndex> {
         self.strtable.get(symbol_name)
         .and_then(|idx| self.indexes.get(&idx).cloned())
    }
    /// Insert a new symbol into this objects symbol table
    pub fn insert(&mut self, symbol_name: &str, kind: SymbolType) {
        // mach-o requires _ prefixes on every symbol, we will allow this to be configurable later
        //let name = format!("_{}", symbol_name);
        let name = symbol_name;
        // 1 for null terminator and 1 for _ prefix (defered until write time);
        let name_len = name.len() as u64 + 1 + 1;
        let last_index = self.strtable.len();
        let name_index = self.strtable.get_or_intern(name);
        debug!("{}: {} <= {}", symbol_name, last_index, name_index);
        // the string is new: NB: relies on name indexes incrementing in sequence, starting at 0
        if name_index == last_index {
            debug!("Inserting new symbol: {}", self.strtable.resolve(name_index).unwrap());
            // TODO: add code offset into symbol n_value
            let builder = match kind {
                SymbolType::Undefined => SymbolBuilder::new(self.strtable_size).global(true).import(),
                SymbolType::Defined { section, absolute_offset, global, segment_relative_offset } => {
                    SymbolBuilder::new(self.strtable_size).global(global)
                        .offset(absolute_offset)
                        .relative_offset(segment_relative_offset)
                        .section(section)
                }
            };
            // insert the builder for this symbol, using its strtab index
            self.symbols.insert(name_index, builder);
            // now create the symbols index, and using strtab name as lookup
            self.indexes.insert(name_index, self.symbols.len() - 1);
            // NB do not move this, otherwise all offsets will be off
            self.strtable_size += name_len;
        }
    }
}

#[derive(Debug)]
/// A Mach-o program segment
struct SegmentBuilder {
    /// The sections that belong to this program segment; currently only 2 (text + data)
    pub sections: [SectionBuilder; SegmentBuilder::NSECTIONS],
    /// A stupid offset value I need to refactor out
    pub offset: u64,
    size: u64,
}

impl SegmentBuilder {
    pub const NSECTIONS: usize = 3;
    /// The size of this segment's _data_, in bytes
    pub fn size(&self) -> u64 {
        self.size
    }
    /// The size of this segment's _load command_, including its associated sections, in bytes
    pub fn load_command_size(ctx: &Ctx) -> u64 {
        Segment::size_with(&ctx) as u64 + (Self::NSECTIONS as u64 * Section::size_with(&ctx) as u64)
    }
    fn _section_data_file_offset(ctx: &Ctx) -> u64 {
        // section data
        Header::size_with(&ctx.container) as u64 + Self::load_command_size(ctx)
    }
    // FIXME: this is in desperate need of refactoring, obviously
    fn build_section(symtab: &mut SymbolTable, sectname: &'static str, segname: &'static str, offset: &mut u64, addr: &mut u64, symbol_offset: &mut u64, section: SectionIndex, definitions: &[Definition], alignment_exponent: u64, flags: Option<u32>) -> SectionBuilder {
        let mut local_size = 0;
        let mut segment_relative_offset = 0;
        for def in definitions {
            local_size += def.data.len() as u64;
            symtab.insert(def.name, SymbolType::Defined { section, segment_relative_offset, absolute_offset: *symbol_offset, global: def.prop.global });
            *symbol_offset += def.data.len() as u64;
            segment_relative_offset += def.data.len() as u64;
        }
        let mut section = SectionBuilder::new(sectname, segname, local_size).offset(*offset).addr(*addr).align(alignment_exponent);
        if let Some(flags) = flags {
            section = section.flags(flags);
        }
        *offset += local_size;
        *addr += local_size;
        section
    }
    /// Create a new program segment from an `artifact`, symbol table, and context
    // FIXME: this is pub(crate) for now because we can't leak pub(crate) Definition
    pub(crate) fn new(artifact: &Artifact, code: &[Definition], data: &[Definition], cstrings: &[Definition], symtab: &mut SymbolTable, ctx: &Ctx) -> Self {
        let mut offset = Header::size_with(&ctx.container) as u64;
        let mut size = 0;
        let mut symbol_offset = 0;
        let text = Self::build_section(symtab, "__text", "__TEXT", &mut offset, &mut size, &mut symbol_offset, CODE_SECTION_INDEX, &code, 4, Some(S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS));
        let data = Self::build_section(symtab, "__data", "__DATA", &mut offset, &mut size, &mut symbol_offset, DATA_SECTION_INDEX, &data, 3, None);
        let cstrings = Self::build_section(symtab, "__cstring", "__TEXT", &mut offset, &mut size, &mut symbol_offset, CSTRING_SECTION_INDEX, &cstrings, 0, Some(S_CSTRING_LITERALS));
        for (ref import, _) in artifact.imports() {
            symtab.insert(import, SymbolType::Undefined);
        }
        // FIXME re add assert
        //assert_eq!(offset, Header::size_with(&ctx.container) + Self::load_command_size(ctx));
        debug!("Segment Size: {} Symtable LoadCommand Offset: {}", size, offset);
        let sections = [text, data, cstrings];
        SegmentBuilder {
            size,
            sections,
            offset,
        }
    }
}

/// A Mach-o object file container
#[derive(Debug)]
struct Mach<'a> {
    ctx: Ctx,
    architecture: Architecture,
    symtab: SymbolTable,
    segment: SegmentBuilder,
    relocations: Relocations,
    code: ArtifactCode<'a>,
    data: ArtifactData<'a>,
    cstrings: Vec<Definition<'a>>,
    _p: ::std::marker::PhantomData<&'a ()>,
}

impl<'a> Mach<'a> {
    pub fn new(artifact: &'a Artifact) -> Self {
        let ctx = make_ctx(&artifact.target);
        // FIXME: I believe we can avoid this partition by refactoring SegmentBuilder::new
        let (mut code, mut data, mut cstrings) = (Vec::new(), Vec::new(), Vec::new());
        for def in artifact.definitions() {
            if def.prop.function {
                code.push(def);
            } else if def.prop.cstring {
                cstrings.push(def)
            } else {
                data.push(def);
            }
        }

        let mut symtab = SymbolTable::new();
        let segment = SegmentBuilder::new(&artifact, &code, &data, &cstrings, &mut symtab, &ctx);
        let relocations = build_relocations(&artifact, &symtab);

        Mach {
            ctx,
            architecture: artifact.target.architecture,
            symtab,
            segment,
            relocations,
            _p: ::std::marker::PhantomData::default(),
            code,
            data,
            cstrings,
        }
    }
    fn header(&self, sizeofcmds: u64) -> Header {
        let mut header = Header::new(&self.ctx);
        header.filetype = MH_OBJECT;
        // safe to divide up the sections into sub-sections via symbols for dead code stripping
        header.flags = MH_SUBSECTIONS_VIA_SYMBOLS;
        header.cputype = CpuType::from(self.architecture).0;
        header.cpusubtype = 3;
        header.ncmds = 2;
        header.sizeofcmds = sizeofcmds as u32;
        header
    }
    pub fn write<T: Write + Seek>(self, file: T) -> Result<(), Error> {
        let mut file = BufWriter::new(file);
        // FIXME: this is ugly af, need cmdsize to get symtable offset
        // construct symtab command
        let mut symtab_load_command = SymtabCommand::new();
        let segment_load_command_size = SegmentBuilder::load_command_size(&self.ctx);
        let sizeof_load_commands = segment_load_command_size + symtab_load_command.cmdsize as u64;
        let symtable_offset = self.segment.offset + sizeof_load_commands;
        let strtable_offset = symtable_offset + (self.symtab.len() as u64 * Nlist::size_with(&self.ctx) as u64);
        let relocation_offset_start = strtable_offset + self.symtab.sizeof_strtable();
        let first_section_offset = Header::size_with(&self.ctx) as u64 + sizeof_load_commands;
        // start with setting the headers dependent value
        let header = self.header(sizeof_load_commands);
        
        debug!("Symtable: {:#?}", self.symtab);
        // marshall the sections into something we can actually write
        let mut raw_sections = Cursor::new(Vec::<u8>::new());
        let mut relocation_offset = relocation_offset_start;
        let mut section_offset = first_section_offset;
        for (idx, section) in self.segment.sections.into_iter().cloned().enumerate() {
            let mut section: Section = section.create();
            section.offset = section_offset as u32;
            section_offset += section.size;
            debug!("{}: Setting nrelocs", idx);
            // relocations are tied to segment/sections
            // TODO: move this also into SegmentBuilder
            if idx < self.relocations.len() {
                let nrelocs = self.relocations[idx].len();
                section.nreloc = nrelocs as _;
                section.reloff = relocation_offset as u32;
                relocation_offset += nrelocs as u64 * SIZEOF_RELOCATION_INFO as u64;
            }
            debug!("Section: {:#?}", section);
            raw_sections.iowrite_with(&section, self.ctx)?;
        }
        let raw_sections = raw_sections.into_inner();
        debug!("Raw sections len: {} - Section start: {} Strtable size: {} - Segment size: {}", raw_sections.len(), first_section_offset, self.symtab.sizeof_strtable(), self.segment.size());

        let mut segment_load_command = Segment::new(self.ctx, &raw_sections);
        segment_load_command.nsects = self.segment.sections.len() as u32;
        // FIXME: de-magic number these
        segment_load_command.initprot = 7;
        segment_load_command.maxprot = 7;
        segment_load_command.filesize = self.segment.size();
        segment_load_command.vmsize = segment_load_command.filesize;
        segment_load_command.fileoff = first_section_offset;
        debug!("Segment: {:#?}", segment_load_command);

        debug!("Symtable Offset: {:#?}", symtable_offset);
        assert_eq!(symtable_offset, self.segment.offset + segment_load_command.cmdsize as u64 + symtab_load_command.cmdsize as u64);
        symtab_load_command.nsyms = self.symtab.len() as u32;
        symtab_load_command.symoff = symtable_offset as u32;
        symtab_load_command.stroff = strtable_offset as u32;
        symtab_load_command.strsize = self.symtab.sizeof_strtable() as u32;

        debug!("Symtab Load command: {:#?}", symtab_load_command);

        //////////////////////////////
        // write header
        //////////////////////////////
        file.iowrite_with(&header, self.ctx)?;
        debug!("SEEK: after header: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write load commands
        //////////////////////////////
        file.iowrite_with(&segment_load_command, self.ctx)?;
        file.write_all(&raw_sections)?;
        file.iowrite_with(&symtab_load_command, self.ctx.le)?;
        debug!("SEEK: after load commands: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write code
        //////////////////////////////
        for code in self.code {
            file.write_all(code.data)?;
        }
        debug!("SEEK: after code: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write data
        //////////////////////////////
        for data in self.data {
            file.write_all(data.data)?;
        }
        debug!("SEEK: after data: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write cstrings
        //////////////////////////////
        for cstring in self.cstrings {
            file.write_all(cstring.data)?;
        }
        debug!("SEEK: after cstrings: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write symtable
        //////////////////////////////
        for (idx, symbol) in self.symtab.symbols.into_iter() {
            let symbol = symbol.create();
            debug!("{}: {:?}", idx, symbol);
            file.iowrite_with(&symbol, self.ctx)?;
        }
        debug!("SEEK: after symtable: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write strtable
        //////////////////////////////
        // we need to write first, empty element - but without an underscore
        file.iowrite(&0u8)?;
        for (idx, string) in self.symtab.strtable.into_iter().skip(1) {
            debug!("{}: {:?}", idx, string);
            // yup, an underscore
            file.iowrite(&0x5fu8)?;
            file.write_all(string.as_bytes())?;
            file.iowrite(&0u8)?;
        }
        debug!("SEEK: after strtable: {}", file.seek(Current(0))?);

        //////////////////////////////
        // write relocations
        //////////////////////////////
        for section_relocations in self.relocations.into_iter() {
            debug!("Relocations: {}", section_relocations.len());
            for reloc in section_relocations.into_iter() {
                debug!("  {:?}", reloc);
                file.iowrite_with(&reloc, self.ctx.le)?;
            }
        }
        debug!("SEEK: after relocations: {}", file.seek(Current(0))?);

        file.iowrite(&0u8)?;

        Ok(())
    }
}

// FIXME: this should actually return a runtime error if we encounter a from.decl to.decl pair which we don't explicitly match on
fn build_relocations(artifact: &Artifact, symtab: &SymbolTable) -> Relocations {
    use goblin::mach::relocation::{X86_64_RELOC_BRANCH, X86_64_RELOC_SIGNED, X86_64_RELOC_UNSIGNED, X86_64_RELOC_GOT_LOAD};
    let mut text_relocations = Vec::new();
    let mut data_relocations = Vec::new();
    debug!("Generating relocations");
    for link in artifact.links() {
        debug!("Import links for: from {} to {} at {:#x} with {:?}", link.from.name, link.to.name, link.at, link.to.decl);
        let (absolute, reloc) = match (link.from.decl, link.to.decl) {
            // NB: we currenetly deduce the meaning of our relocation from from decls -> to decl relocations
            // e.g., global static data references, are constructed from Data -> Data links
            // various static function pointers in the .data section
            (&Decl::Data {..}, &Decl::Function {..}) => (true, X86_64_RELOC_UNSIGNED),
            (&Decl::Data {..}, &Decl::FunctionImport {..}) => (true, X86_64_RELOC_UNSIGNED),
            // anything else is just a regular relocation/callq
            (_, &Decl::Function {..}) => (false, X86_64_RELOC_BRANCH),
            // we are a relocation in the data section to another object in the data section, e.g., a static reference
            (&Decl::Data {..}, &Decl::Data {..}) => (true, X86_64_RELOC_UNSIGNED),
            (_, &Decl::Data {..}) => (false, X86_64_RELOC_SIGNED),
            // TODO: we will also need to specify relocations from Data to Cstrings, e.g., char * STR = "a global static string";
            (_, &Decl::CString {..}) => (false, X86_64_RELOC_SIGNED),
            (_, &Decl::FunctionImport) => (false, X86_64_RELOC_BRANCH),
            (_, &Decl::DataImport) => (false, X86_64_RELOC_GOT_LOAD),
        };
        match (symtab.offset(link.from.name), symtab.index(link.to.name)) {
            (Some(base_offset), Some(to_symbol_index)) => {
                debug!("{} offset: {}", link.to.name, base_offset + link.at);
                let builder = RelocationBuilder::new(to_symbol_index, base_offset + link.at, reloc);
                // NB: we currently associate absolute relocations with data relocations; this may prove
                // too fragile for future additions; needs analysis
                if absolute {
                    data_relocations.push(builder.absolute().create());
                } else {
                    text_relocations.push(builder.create());
                }
            },
            _ => error!("Import Relocation from {} to {} at {:#x} has a missing symbol. Dumping symtab {:?}", link.from.name, link.to.name, link.at, symtab)
        }
    }
    vec![text_relocations, data_relocations]
}

pub fn to_bytes(artifact: &Artifact) -> Result<Vec<u8>, Error> {
    let mach = Mach::new(&artifact);
    let mut buffer = Cursor::new(Vec::new());
    mach.write(&mut buffer)?;
    Ok(buffer.into_inner())
}
