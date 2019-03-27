//! The ELF32/64 bit backend for transforming an artifact to a valid, ELF object file.
// FIXME: this is temporary, we anticipate None variant and pub fn rel being used in the future
// for: 1. object files with source file name symbols
//      2. 32-bit object files
// respectively; remove this once used again
#![allow(dead_code)]

use crate::{
    artifact::{
        self, Artifact, DataType, Decl, DefinedDecl, ImportKind, LinkAndDecl, Reloc, Scope,
        Visibility,
    },
    target::make_ctx,
    Ctx,
};
use failure::Error;
use goblin;

use indexmap::IndexMap;
use scroll::IOwrite;
use std::collections::{hash_map, HashMap};
use std::fmt;
use std::io::SeekFrom::*;
use std::io::{BufWriter, Cursor, Seek, Write};
use string_interner::DefaultStringInterner;
use target_lexicon::Architecture;

use goblin::elf::header::{self, Header};
use goblin::elf::reloc;
use goblin::elf::section_header::{self, SectionHeader};

// interned string idx
type StringIndex = usize;
// an offset into the object file
type Offset = usize;
type Relocation = goblin::elf::reloc::Reloc;
type Symbol = goblin::elf::sym::Sym;
type Section = SectionHeader;

struct MachineTag(u16);

impl From<Architecture> for MachineTag {
    fn from(architecture: Architecture) -> MachineTag {
        use goblin::elf::header::*;
        use target_lexicon::Architecture::*;
        MachineTag(match architecture {
            X86_64 => EM_X86_64,
            I386 | I586 | I686 => EM_386,
            Aarch64 => EM_AARCH64,
            Arm | Armebv7r | Armv4t | Armv5te | Armv6 | Armv7 | Armv7r | Armv7s | Thumbv6m
            | Thumbv7a | Thumbv7em | Thumbv7neon | Thumbv7m | Thumbv8mBase | Thumbv8mMain => EM_ARM,
            Mips | Mipsel | Mips64 | Mips64el => EM_MIPS,
            Powerpc => EM_PPC,
            Powerpc64 | Powerpc64le => EM_PPC64,
            Riscv32 | Riscv32imac | Riscv32imc | Riscv64 => EM_RISCV,
            S390x => EM_S390,
            Sparc => EM_SPARC,
            Sparc64 | Sparcv9 => EM_SPARCV9,
            Msp430 => EM_MSP430,
            Unknown => EM_NONE,
            Asmjs => panic!("asm.js does not exist in ELF"),
            Wasm32 => panic!("wasm32 does not exist in ELF"),
        })
    }
}

/// The kind of symbol this is; used in [SymbolBuilder](struct.SymbolBuilder.html)
enum SymbolType<'a> {
    /// From a definition
    Decl(&'a DefinedDecl),
    /// An import
    Import,
    /// A section reference
    Section,
    /// A file reference
    File,
    /// None
    None,
}

/// A builder for creating a 32/64 bit ELF symbol
struct SymbolBuilder<'a> {
    name_offset: usize,
    size: u64,
    typ: SymbolType<'a>,
    shndx: usize,
}

impl<'a> SymbolBuilder<'a> {
    /// Create a new symbol with `typ`
    pub fn new(typ: SymbolType<'a>) -> Self {
        SymbolBuilder {
            name_offset: 0,
            typ,
            size: 0,
            shndx: 0,
        }
    }
    pub fn from_decl(decl: &'a DefinedDecl) -> Self {
        SymbolBuilder::new(SymbolType::Decl(decl))
    }
    /// Set the size of this symbol; for functions, it should be the routines size in bytes
    pub fn size(mut self, size: usize) -> Self {
        self.size = size as u64;
        self
    }
    /// Set the symbol name as a byte offset into the corresponding strtab
    pub fn name_offset(mut self, name_offset: usize) -> Self {
        self.name_offset = name_offset;
        self
    }
    /// Set the section index
    pub fn section_index(mut self, shndx: usize) -> Self {
        // Underlying representation is only 16 bits. Catch this early!
        debug_assert!(shndx < u16::max_value() as usize);
        self.shndx = shndx;
        self
    }
    /// Finalize and create the symbol
    pub fn create(self) -> Symbol {
        use goblin::elf::section_header::SHN_ABS;
        use goblin::elf::sym::{
            STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FILE, STT_FUNC, STT_NOTYPE, STT_OBJECT,
            STT_SECTION, STV_DEFAULT, STV_HIDDEN, STV_PROTECTED,
        };
        let mut st_shndx = self.shndx;
        let mut st_info = 0;
        let mut st_other = 0;
        let st_value = 0;

        fn scope_stb_flags(s: Scope) -> u8 {
            let flag = match s {
                Scope::Local => STB_LOCAL,
                Scope::Global => STB_GLOBAL,
                Scope::Weak => STB_WEAK,
            };
            flag << 4
        }

        fn vis_stother_flags(v: Visibility) -> u8 {
            match v {
                Visibility::Default => STV_DEFAULT,
                Visibility::Hidden => STV_HIDDEN,
                Visibility::Protected => STV_PROTECTED,
            }
        }

        match self.typ {
            SymbolType::Decl(DefinedDecl::Function(d)) => {
                st_info |= STT_FUNC;
                st_info |= scope_stb_flags(d.get_scope());
                st_other |= vis_stother_flags(d.get_visibility());
            }
            SymbolType::Decl(DefinedDecl::Data(d)) => {
                st_info |= STT_OBJECT;
                st_info |= scope_stb_flags(d.get_scope());
                st_other |= vis_stother_flags(d.get_visibility());
            }
            SymbolType::Import => {
                st_info = STT_NOTYPE;
                st_info |= STB_GLOBAL << 4;
            }
            SymbolType::Decl(DefinedDecl::DebugSection(_)) | SymbolType::Section => {
                st_info |= STT_SECTION;
                st_info |= STB_LOCAL << 4;
            }
            SymbolType::File => {
                st_info = STT_FILE;
                // knowledge™
                st_shndx = SHN_ABS as usize;
            }
            SymbolType::None => st_info = STT_NOTYPE,
        }
        Symbol {
            st_name: self.name_offset,
            st_other,
            st_size: self.size,
            st_info,
            st_shndx,
            st_value,
        }
    }
}

/// The kind of section this can be; used in [SectionBuilder](struct.SectionBuilder.html)
enum SectionType {
    Bits,
    Data,
    String,
    StrTab,
    SymTab,
    Relocation,
    None,
}

/// A builder for creating a 32/64 bit section
struct SectionBuilder {
    typ: SectionType,
    exec: bool,
    write: bool,
    alloc: bool,
    size: u64,
    name_offset: usize,
    align: Option<usize>,
}

impl SectionBuilder {
    /// Create a new section with `size`
    pub fn new(size: u64) -> Self {
        SectionBuilder {
            typ: SectionType::None,
            exec: false,
            write: false,
            alloc: false,
            name_offset: 0,
            size,
            align: None,
        }
    }
    /// Make this section executable
    pub fn exec(mut self, executable: bool) -> Self {
        self.exec = executable;
        self
    }
    /// Make this section allocatable
    pub fn alloc(mut self) -> Self {
        self.alloc = true;
        self
    }
    /// Make this section writable
    pub fn writable(mut self, writable: bool) -> Self {
        self.write = writable;
        self
    }
    /// Specify section alignment
    pub fn align(mut self, align: Option<usize>) -> Self {
        self.align = align;
        self
    }

    /// Set the byte offset of this section's name in the corresponding strtab
    pub fn name_offset(mut self, name_offset: usize) -> Self {
        self.name_offset = name_offset;
        self
    }
    /// Set the type of this section
    fn section_type(mut self, typ: SectionType) -> Self {
        self.typ = typ;
        self
    }
    /// Finalize and create the actual section
    pub fn create(self, ctx: &Ctx) -> Section {
        use goblin::elf::section_header::*;
        let mut shdr = Section::default();
        shdr.sh_flags = 0u64;
        shdr.sh_size = self.size;
        shdr.sh_name = self.name_offset;
        if self.exec {
            shdr.sh_flags |= SHF_EXECINSTR as u64
        }
        if self.write {
            shdr.sh_flags |= SHF_WRITE as u64
        }
        if self.alloc {
            shdr.sh_flags |= SHF_ALLOC as u64
        }

        let align = if let Some(align) = self.align {
            align as u64
        } else if self.exec {
            0x10
        } else if self.write {
            0x8
        } else {
            1
        };

        match self.typ {
            SectionType::Bits => {
                shdr.sh_addralign = align;
                shdr.sh_type = SHT_PROGBITS;
            }
            SectionType::String => {
                shdr.sh_addralign = align;
                shdr.sh_type = SHT_PROGBITS;
                shdr.sh_flags |= (SHF_MERGE | SHF_STRINGS) as u64;
            }
            SectionType::Data => {
                shdr.sh_addralign = align;
                shdr.sh_type = SHT_PROGBITS;
            }
            SectionType::StrTab => {
                shdr.sh_addralign = 0x1;
                shdr.sh_type = SHT_STRTAB;
            }
            SectionType::SymTab => {
                shdr.sh_entsize = Symbol::size(ctx.container) as u64;
                shdr.sh_addralign = 0x8;
                shdr.sh_type = SHT_SYMTAB;
            }
            SectionType::Relocation => {
                // FIXME: hardcodes to use rela
                shdr.sh_entsize = Relocation::size(true, *ctx) as u64;
                shdr.sh_addralign = 0x8;
                shdr.sh_flags = 0;
                shdr.sh_type = SHT_RELA
            }
            SectionType::None => shdr.sh_type = SHT_NULL,
        }
        shdr
    }
}

#[derive(Debug)]
struct SectionInfo {
    header: Section,
    symbol: Symbol,
    name: StringIndex,
}

// r_offset: 17 r_typ: 4 r_sym: 12 r_addend: fffffffffffffffc rela: true,
/// A builder for constructing a cross platform relocation
struct RelocationBuilder {
    addend: Option<i64>,
    sym_idx: usize,
    offset: u64,
    typ: u32,
}

impl RelocationBuilder {
    /// Create a new relocation with `typ`
    pub fn new(typ: u32) -> Self {
        RelocationBuilder {
            addend: Some(0),
            offset: 0,
            sym_idx: 0,
            typ,
        }
    }
    /// Set this relocation to a relocation without an addend
    pub fn rel(mut self) -> Self {
        self.addend = None;
        self
    }
    /// Set this relocation to a relocation with an addend of `addend`.
    pub fn addend(mut self, addend: i64) -> Self {
        self.addend = Some(addend);
        self
    }
    /// Set the section relative offset this relocation refers to
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }
    /// Set the symbol index this relocation affects
    pub fn sym(mut self, sym_idx: usize) -> Self {
        self.sym_idx = sym_idx;
        self
    }
    /// Finalize and actually create this relocation
    pub fn create(self) -> Relocation {
        Relocation {
            r_offset: self.offset,
            r_addend: self.addend,
            r_sym: self.sym_idx,
            r_type: self.typ,
        }
    }
}

//#[derive(Debug)]
/// An intermediate ELF object file container
struct Elf<'a> {
    name: &'a str,
    code: IndexMap<StringIndex, &'a [u8]>,
    relocations: IndexMap<StringIndex, (Section, Vec<Relocation>)>,
    symbols: IndexMap<StringIndex, Symbol>,
    special_symbols: Vec<Symbol>,
    imports: HashMap<StringIndex, ImportKind>,
    sections: IndexMap<StringIndex, SectionInfo>,
    offsets: HashMap<StringIndex, Offset>,
    sizeof_strtab: Offset,
    strings: DefaultStringInterner,
    sizeof_bits: Offset,
    nsections: u16,
    ctx: Ctx,
    architecture: Architecture,
    nlocals: usize,
}

impl<'a> fmt::Debug for Elf<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "{}", self.name)?;
        writeln!(fmt, "{:?}", self.code)?;
        writeln!(fmt, "{:#?}", self.relocations)?;
        writeln!(fmt, "{:?}", self.imports)?;
        writeln!(fmt, "{:?}", self.sections)?;
        writeln!(fmt, "{:?}", self.offsets)?;
        writeln!(fmt, "SizeofStrtab: {:?}", self.sizeof_strtab)?;
        writeln!(fmt, "SizeofBits: {:?}", self.sizeof_bits)?;
        //writeln!(fmt, "SymtabOffset: {:?}", self.        symtab_offset)?;
        writeln!(fmt, "Strings: {:?}", self.strings.len())?;
        writeln!(fmt, "{:?}", self.ctx)
    }
}

const STRTAB_LINK: u16 = 1;
const SYMTAB_LINK: u16 = 2;

impl<'a> Elf<'a> {
    pub fn new(artifact: &'a Artifact) -> Self {
        let ctx = make_ctx(&artifact.target);
        let mut offsets = HashMap::new();
        let mut strings = DefaultStringInterner::default();
        let mut special_symbols = Vec::new();
        let mut sizeof_strtab = 1;

        {
            let mut push_strtab = |name: &str| {
                let name = name.to_owned();
                let size = name.len() + 1;
                let idx = strings.get_or_intern(name);
                let offset = sizeof_strtab;
                offsets.insert(idx, offset);
                sizeof_strtab += size;
                offset
            };

            push_strtab(".strtab");
            push_strtab(".symtab");
            let offset = push_strtab(&artifact.name);
            // ELF requires a null symbol as the first symbol.
            special_symbols.push(Symbol::default());
            special_symbols.push(
                SymbolBuilder::new(SymbolType::File)
                    .name_offset(offset)
                    .create(),
            );
        }

        let sizeof_bits = Header::size(&ctx);
        Elf {
            name: &artifact.name,
            code: IndexMap::new(),
            relocations: IndexMap::new(),
            imports: HashMap::new(),
            symbols: IndexMap::new(),
            special_symbols,
            sections: IndexMap::new(),
            nsections: 4,
            offsets,
            strings,
            sizeof_strtab,
            sizeof_bits,
            ctx,
            architecture: artifact.target.architecture,
            nlocals: 0,
        }
    }
    fn new_string(&mut self, name: String) -> (StringIndex, usize) {
        let size = name.len() + 1;
        let idx = self.strings.get_or_intern(name);
        match self.offsets.entry(idx) {
            hash_map::Entry::Occupied(entry) => (idx, *entry.get()),
            hash_map::Entry::Vacant(entry) => {
                let offset = self.sizeof_strtab;
                self.sizeof_strtab += size;
                (idx, *entry.insert(offset))
            }
        }
    }
    pub fn add_definition(&mut self, name: &str, data: &'a [u8], decl: &artifact::DefinedDecl) {
        let def_size = data.len();

        let section_name = match decl {
            DefinedDecl::Function(_) => format!(".text.{}", name),
            DefinedDecl::Data(d) => format!(
                ".{}.{}",
                if d.is_writable() { "data" } else { "rodata" },
                name
            ),
            DefinedDecl::DebugSection(_) => name.to_owned(),
        };

        let section = match decl {
            DefinedDecl::Function(d) => SectionBuilder::new(def_size as u64)
                .section_type(SectionType::Bits)
                .alloc()
                .writable(false)
                .exec(true)
                .align(d.get_align()),
            DefinedDecl::Data(d) => SectionBuilder::new(def_size as u64)
                .section_type(match d.get_datatype() {
                    DataType::Bytes => SectionType::Data,
                    DataType::String => SectionType::String,
                })
                .alloc()
                .writable(d.is_writable())
                .exec(false)
                .align(d.get_align()),
            DefinedDecl::DebugSection(d) => SectionBuilder::new(def_size as u64)
                .section_type(
                    // TODO: this behavior should be deprecated, but we need to warn users!
                    if name == ".debug_str" || name == ".debug_line_str" {
                        SectionType::String
                    } else {
                        match d.get_datatype() {
                            DataType::Bytes => SectionType::Bits,
                            DataType::String => SectionType::String,
                        }
                    },
                )
                .align(d.get_align()),
        };

        let shndx = self.add_progbits(section_name, section, data);

        match decl {
            DefinedDecl::Function(_) | DefinedDecl::Data(_) => {
                let (idx, offset) = self.new_string(name.to_string());
                debug!(
                    "idx: {:?} @ {:#x} - new strtab offset: {:#x}",
                    idx, offset, self.sizeof_strtab
                );
                // build symbol based on this _and_ the properties of the definition
                let symbol = SymbolBuilder::from_decl(decl)
                    .size(def_size)
                    .name_offset(offset)
                    .section_index(shndx)
                    .create();
                // insert it into our symbol table
                self.symbols.insert(idx, symbol);
                // sh_info requires nsections + nlocals to add as delimiter; see the associated FunFact
                // nonglobals go into the symbol table first (per iteration through definitions in
                // caller)
                if !decl.is_global() {
                    self.nlocals += 1;
                }
            }
            DefinedDecl::DebugSection(_) => {
                // No symbols in debug sections, yet...
            }
        }
    }
    /// Create a progbits section (and its section symbol), and return the section index.
    fn add_progbits(&mut self, name: String, section: SectionBuilder, data: &'a [u8]) -> usize {
        let (idx, offset) = self.new_string(name);
        debug!(
            "idx: {:?} @ {:#x} - new strtab offset: {:#x}",
            idx, offset, self.sizeof_strtab
        );
        // store the size of this code
        let size = data.len();
        // the symbols section reference/index will be the current number of sections
        let shndx = self.sections.len() + 3; // null + strtab + symtab
        let section_symbol = SymbolBuilder::new(SymbolType::Section)
            .section_index(shndx)
            .create();

        let mut section = section.name_offset(offset).create(&self.ctx);
        // the offset is the head of how many program bits we've added
        section.sh_offset = self.sizeof_bits as u64;
        self.sections.insert(
            idx,
            SectionInfo {
                header: section,
                symbol: section_symbol,
                name: idx,
            },
        );
        self.nsections += 1;
        // increment the size
        self.sizeof_bits += size;

        self.code.insert(idx, data);
        shndx
    }
    pub fn import(&mut self, import: String, kind: &ImportKind) {
        let (idx, offset) = self.new_string(import);
        let symbol = SymbolBuilder::new(SymbolType::Import)
            .name_offset(offset)
            .create();
        self.imports.insert(idx, kind.clone());
        self.symbols.insert(idx, symbol);
    }
    pub fn link(&mut self, l: &LinkAndDecl) {
        debug!("Link: {:?}", l);
        let (to_idx, to_shndx) = {
            let to_idx = self.strings.get_or_intern(l.to.name);
            if l.to.decl.is_section() {
                let (to_idx, _, _) = self
                    .sections
                    .get_full(&to_idx)
                    .expect("to_idx present in sections");
                // Section symbols come after special symbols.
                // The section index is after null + strtab + symtab.
                (to_idx + self.special_symbols.len(), to_idx + 3)
            } else {
                let (to_idx, _, symbol) = self
                    .symbols
                    .get_full(&to_idx)
                    .expect("to_idx present in symbols");
                // Normal symbols come after special symbols and section symbols.
                (
                    to_idx + self.special_symbols.len() + self.sections.len(),
                    symbol.st_shndx,
                )
            }
        };
        let (from_idx, from_shndx) = {
            let from_idx = self.strings.get_or_intern(l.from.name);
            if l.from.decl.is_section() {
                let (from_idx, _, _) = self
                    .sections
                    .get_full(&from_idx)
                    .expect("from_idx present in sections");
                // Section symbols come after special symbols.
                // The section index is after null + strtab + symtab.
                (from_idx + self.special_symbols.len(), from_idx + 3)
            } else {
                let (from_idx, _, symbol) = self
                    .symbols
                    .get_full(&from_idx)
                    .expect("from_idx present in symbols");
                // Normal symbols come after special symbols and section symbols.
                (
                    from_idx + self.special_symbols.len() + self.sections.len(),
                    symbol.st_shndx,
                )
            }
        };
        let (reloc, addend) = match l.reloc {
            Reloc::Auto => {
                match *l.from.decl {
                    Decl::Defined(DefinedDecl::Function { .. }) => {
                        match *l.to.decl {
                            // NB: this now forces _all_ function references, whether local or not, through the PLT
                            // although we're not in the worst company here: https://github.com/ocaml/ocaml/pull/1330
                            Decl::Defined(DefinedDecl::Function { .. })
                            | Decl::Import(ImportKind::Function) => (reloc::R_X86_64_PLT32, -4),
                            Decl::Defined(DefinedDecl::Data { .. }) => (reloc::R_X86_64_PC32, -4),
                            Decl::Import(ImportKind::Data) => (reloc::R_X86_64_GOTPCREL, -4),
                            _ => panic!("unsupported relocation {:?}", l),
                        }
                    }
                    Decl::Defined(DefinedDecl::Data { .. }) => {
                        if self.ctx.is_big() {
                            // Select an absolute relocation that is the size of a pointer.
                            (reloc::R_X86_64_64, 0)
                        } else {
                            (reloc::R_X86_64_32, 0)
                        }
                    }
                    _ => panic!("unsupported relocation {:?}", l),
                }
            }
            Reloc::Raw { reloc, addend } => (reloc, addend),
            Reloc::Debug { size, addend } => match size {
                4 => (reloc::R_X86_64_32, addend),
                8 => (reloc::R_X86_64_64, addend),
                _ => panic!("unsupported relocation {:?}", l),
            },
        };
        let addend = i64::from(addend);

        let sym_idx = match *l.to.decl {
            Decl::Defined(_) => {
                // We don't emit symbols for null + strtab + symtab, and
                // section symbols come after special symbols.
                (to_shndx - 3) + self.special_symbols.len()
            }
            Decl::Import(_) => to_idx,
        };

        let reloc = RelocationBuilder::new(reloc)
            .sym(sym_idx)
            .offset(l.at)
            .addend(addend)
            .create();
        self.add_reloc(l.from.name, reloc, from_idx, from_shndx)
    }
    fn add_reloc(&mut self, relocee: &str, reloc: Relocation, idx: usize, shndx: usize) {
        debug!(
            "add reloc for symbol {} section {} - reloc: {:?}",
            idx, shndx, &reloc
        );
        let reloc_size = Relocation::size(reloc.r_addend.is_some(), self.ctx) as u64;
        if self.relocations.contains_key(&shndx) {
            debug!("{} has relocs", relocee);
            let &mut (ref mut section, ref mut relocs) = self.relocations.get_mut(&shndx).unwrap();
            // its size is currently how many relocations there are
            section.sh_size += section.sh_entsize;
            relocs.push(reloc);
        } else {
            debug!("{} does NOT have relocs", relocee);
            // now create the relocation section
            let reloc_name = {
                let (_, section) = self
                    .sections
                    .get_index(shndx - 3)
                    .expect("shndx present in sections");
                let section_name = self
                    .strings
                    .resolve(section.name)
                    .expect("section name in strings");
                format!(".rela{}", section_name)
            };
            let (_reloc_idx, reloc_section_offset) = self.new_string(reloc_name);
            let mut reloc_section = SectionBuilder::new(reloc_size)
                .name_offset(reloc_section_offset)
                .section_type(SectionType::Relocation)
                .create(&self.ctx);
            // its sh_link always points to the symtable
            reloc_section.sh_link = SYMTAB_LINK as u32;
            // info tells us which section these relocations apply to
            reloc_section.sh_info = shndx as u32;
            reloc_section.sh_flags |= section_header::SHF_INFO_LINK as u64;
            self.relocations.insert(shndx, (reloc_section, vec![reloc]));
            self.nsections += 1;
        }
    }
    pub fn write<T: Write + Seek>(mut self, file: T) -> goblin::error::Result<()> {
        let mut file = BufWriter::new(file);
        /////////////////////////////////////
        // Compute Offsets
        /////////////////////////////////////
        let sizeof_symtab = (self.symbols.len() + self.special_symbols.len() + self.sections.len())
            * Symbol::size(self.ctx.container);
        let sizeof_relocs = self
            .relocations
            .iter()
            .fold(0, |acc, (_, &(ref _shdr, ref rels))| rels.len() + acc)
            * Relocation::size(true, self.ctx);
        let nonexec_stack_note_name_offset = self.new_string(".note.GNU-stack".into()).1;
        let strtab_offset = self.sizeof_bits as u64;
        let symtab_offset = strtab_offset + self.sizeof_strtab as u64;
        let reloc_offset = symtab_offset + sizeof_symtab as u64;
        let sh_offset = reloc_offset + sizeof_relocs as u64;

        debug!(
            "strtab: {:#x} symtab {:#x} relocs {:#x} sh_offset {:#x}",
            strtab_offset, symtab_offset, reloc_offset, sh_offset
        );

        /////////////////////////////////////
        // Header
        /////////////////////////////////////
        let mut header = Header::new(self.ctx);
        let machine: MachineTag = self.architecture.into();
        header.e_machine = machine.0;
        header.e_type = header::ET_REL;
        header.e_shoff = sh_offset;
        header.e_shnum = self.nsections;
        header.e_shstrndx = STRTAB_LINK;

        file.iowrite_with(header, self.ctx)?;
        let after_header = file.seek(Current(0))?;
        debug!("after_header {:#x}", after_header);
        assert_eq!(after_header, Header::size(&self.ctx) as u64);

        /////////////////////////////////////
        // Code
        /////////////////////////////////////

        for (_idx, bytes) in self.code.drain(..) {
            file.write_all(bytes)?;
        }
        let after_code = file.seek(Current(0))?;
        debug!("after_code {:#x}", after_code);
        assert_eq!(after_code, strtab_offset);

        /////////////////////////////////////
        // Init sections
        /////////////////////////////////////

        let mut section_headers = vec![SectionHeader::default()];
        let mut strtab = {
            let offset = *(self.offsets.get(&0).unwrap());
            SectionBuilder::new(self.sizeof_strtab as u64)
                .name_offset(offset)
                .section_type(SectionType::StrTab)
                .create(&self.ctx)
        };
        strtab.sh_offset = strtab_offset;
        section_headers.push(strtab);

        let mut symtab = {
            let offset = *(self.offsets.get(&1).unwrap());
            SectionBuilder::new(sizeof_symtab as u64)
                .name_offset(offset)
                .section_type(SectionType::SymTab)
                .create(&self.ctx)
        };
        symtab.sh_offset = symtab_offset;
        symtab.sh_link = 1; // we link to our strtab above
                            // FunFact: symtab.sh_info acts as a delimiter pointing to which are the "external" functions in the object file;
                            // if this isn't correct, it will segfault linkers or cause them to _sometimes_ emit garbage, ymmv
        symtab.sh_info = (self.special_symbols.len() + self.sections.len() + self.nlocals) as u32;
        section_headers.push(symtab);

        /////////////////////////////////////
        // Strtab
        /////////////////////////////////////
        file.seek(Start(strtab_offset))?;
        file.iowrite(0u8)?; // for the null value in the strtab;
        for (_id, string) in self.strings.iter() {
            debug!("String: {:?}", string);
            file.write_all(string.as_bytes())?;
            file.iowrite(0u8)?;
        }
        let after_strtab = file.seek(Current(0))?;
        debug!("after_strtab {:#x}", after_strtab);
        assert_eq!(after_strtab, symtab_offset);

        /////////////////////////////////////
        // Symtab
        /////////////////////////////////////
        for symbol in self.special_symbols.into_iter() {
            debug!("Special Symbol: {:?}", symbol);
            file.iowrite_with(symbol, self.ctx)?;
        }
        for (_id, section) in self.sections.into_iter() {
            debug!("Section Symbol: {:?}", section.symbol);
            file.iowrite_with(section.symbol, self.ctx)?;
            section_headers.push(section.header);
        }
        for (_id, symbol) in self.symbols.into_iter() {
            debug!("Symbol: {:?}", symbol);
            file.iowrite_with(symbol, self.ctx)?;
        }
        let after_symtab = file.seek(Current(0))?;
        debug!(
            "after_symtab {:#x} - shdr_size {}",
            after_symtab,
            Section::size(&self.ctx)
        );
        assert_eq!(after_symtab, reloc_offset);

        /////////////////////////////////////
        // Relocations
        /////////////////////////////////////
        let mut roffset = reloc_offset;
        for (_, (mut section, mut relocations)) in self.relocations.into_iter() {
            section.sh_offset = roffset;
            roffset += section.sh_size;
            section_headers.push(section);
            for relocation in relocations.drain(..) {
                debug!("Relocation: {:?}", relocation);
                file.iowrite_with(relocation, (relocation.r_addend.is_some(), self.ctx))?;
            }
        }
        let after_relocs = file.seek(Current(0))?;
        debug!("after_relocs {:#x}", after_relocs);
        assert_eq!(after_relocs, sh_offset);

        /////////////////////////////////////
        // Non-executable stack note.
        /////////////////////////////////////
        let nonexec_stack = SectionBuilder::new(0)
            .name_offset(nonexec_stack_note_name_offset)
            .section_type(SectionType::Bits)
            .create(&self.ctx);
        section_headers.push(nonexec_stack);

        /////////////////////////////////////
        // Sections
        /////////////////////////////////////
        let shdr_size = section_headers.len() as u64 * Section::size(&self.ctx) as u64;
        for shdr in section_headers {
            debug!("Section: {:?}", shdr);
            file.iowrite_with(shdr, self.ctx)?;
        }

        let after_shdrs = file.seek(Current(0))?;
        let expected = sh_offset + shdr_size;
        debug!("after_shdrs {:#x}", after_shdrs);
        assert_eq!(after_shdrs, expected);
        debug!("done");
        Ok(())
    }
}

pub fn to_bytes(artifact: &Artifact) -> Result<Vec<u8>, Error> {
    // TODO: make new fully construct the elf object, e.g., the definitions, imports, and links don't take self
    // this means that a call to new has a fully constructed object ready to marshal into bytes, similar to the mach backend
    let mut elf = Elf::new(&artifact);
    for def in artifact.definitions() {
        debug!("Def: {:?}", def);
        elf.add_definition(def.name, def.data, def.decl);
    }
    for (ref import, ref kind) in artifact.imports() {
        debug!("Import: {:?} -> {:?}", import, kind);
        elf.import(import.to_string(), kind);
    }
    for link in artifact.links() {
        elf.link(&link);
    }
    let mut buffer = Cursor::new(Vec::new());
    elf.write(&mut buffer)?;
    Ok(buffer.into_inner())
}
