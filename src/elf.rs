use goblin;
use {error, Artifact, Object, Target, Code, Data};

use std::collections::HashMap;
use std::fmt;
use std::io::{Seek, Cursor, BufWriter, Write};
use std::io::SeekFrom::*;
use scroll::IOwrite;
use shawshank;
use ordermap::OrderMap;

use goblin::elf::header::{self, Header};
use goblin::elf::section_header::{SectionHeader};
use goblin::elf::reloc;

// interned string idx
type StringIndex = usize;
// an offset into the object file
type Offset = usize;
type Relocation = goblin::elf::reloc::Reloc;
type Symbol = goblin::elf::sym::Sym;
type Section = SectionHeader;
type Ctx = goblin::container::Ctx;

struct MachineTag(u16);

impl From<Target> for MachineTag {
    fn from(target: Target) -> MachineTag {
        use self::Target::*;
        use goblin::elf::header::{EM_NONE, EM_386, EM_X86_64, EM_ARM, EM_AARCH64};
        MachineTag(match target {
            X86_64 => EM_X86_64,
            X86 => EM_386,
            ARM64 => EM_AARCH64,
            ARMv7 => EM_ARM,
            Unknown => EM_NONE
        })
    }
}

pub enum SymbolType {
    Function,
    Elf,
    Import,
    Section,
    File,
    None,
}

pub struct SymbolBuilder {
    name_offset: usize,
    global: bool,
    size: u64,
    typ: SymbolType,
}

impl SymbolBuilder {
    pub fn new(typ: SymbolType) -> Self {
        SymbolBuilder {
            global: false,
            name_offset: 0,
            typ,
            size: 0,
        }
    }
    pub fn size(mut self, size: usize) -> Self {
        self.size = size as u64; self
    }
    pub fn global(mut self) -> Self {
        self.global = true; self
    }
    pub fn name_offset(mut self, name_offset: usize) -> Self {
        self.name_offset = name_offset; self
    }
    pub fn create(self) -> Symbol {
        use goblin::elf::sym::{STT_NOTYPE, STT_FILE, STT_FUNC, STT_SECTION, STT_OBJECT, STB_LOCAL, STB_GLOBAL};
        use goblin::elf::section_header::SHN_ABS;
        let mut st_shndx = 0;
        let mut st_info = 0;
        let st_value = 0;
        match self.typ {
            SymbolType::Function => {
                st_info |= STT_FUNC;
            },
            SymbolType::Elf => {
                st_info |= STT_OBJECT;
            },
            SymbolType::Import => {
                st_info = STT_NOTYPE;
                st_info |= STB_GLOBAL << 4;
            },
            SymbolType::Section => {
                st_info |= STT_SECTION;
                st_info |= STB_LOCAL << 4;
            },
            SymbolType::File => {
                st_info = STT_FILE;
                // knowledge™
                st_shndx = SHN_ABS as usize;
            },
            SymbolType::None => {
                st_info = STT_NOTYPE
            },
        }
        if self.global {
            st_info |= STB_GLOBAL << 4;
        } else {
            st_info |= STB_LOCAL << 4;
        }
        Symbol {
            st_name: self.name_offset,
            st_other: 0,
            st_size: self.size,
            st_info,
            st_shndx,
            st_value,
        }

    }
}

pub enum SectionType {
    Bits,
    String,
    StrTab,
    SymTab,
    Relocation,
    None,
}

pub struct SectionBuilder {
    typ: SectionType,
    exec: bool,
    write: bool,
    size: u64,
    name_offset: usize,
}

impl SectionBuilder {
    pub fn new(size: u64) -> Self {
        SectionBuilder {
            typ: SectionType::None,
            exec: false,
            write: false,
            name_offset: 0,
            size,
        }
    }
    pub fn exec(mut self) -> Self {
        self.exec = true; self
    }
    pub fn name_offset(mut self, name_offset: usize) -> Self {
        self.name_offset = name_offset; self
    }
    fn section_type(mut self, typ: SectionType) -> Self {
        self.typ = typ; self
    }
    pub fn create(self, ctx: &Ctx) -> Section {
        use goblin::elf::section_header::*;
        let mut shdr = Section::default();
        shdr.sh_flags = SHF_ALLOC as u64;
        shdr.sh_size = self.size;
        shdr.sh_name = self.name_offset;
        if self.exec {
            shdr.sh_flags |= SHF_EXECINSTR as u64
        }
        if self.write {
            shdr.sh_flags |= SHF_WRITE as u64
        }
        match self.typ {
            SectionType::Bits => {
                shdr.sh_addralign = if self.exec { 0x10 } else if self.write { 0x8 } else { 1 };
                shdr.sh_type = SHT_PROGBITS
            },
            SectionType::String => {
                shdr.sh_addralign = if self.exec { 0x10 } else if self.write { 0x8 } else { 1 };
                shdr.sh_type = SHT_PROGBITS;
                shdr.sh_flags |= (SHF_MERGE | SHF_STRINGS) as u64;
            },
            SectionType::StrTab => {
                shdr.sh_addralign = 0x1;
                shdr.sh_type = SHT_STRTAB;
            },
            SectionType::SymTab => {
                shdr.sh_entsize = Symbol::size(ctx.container) as u64;
                shdr.sh_addralign = 0x8;
                shdr.sh_type = SHT_SYMTAB;
            },
            SectionType::Relocation => {
                // FIXME: hardcodes to use rela
                shdr.sh_entsize = Relocation::size(true, *ctx) as u64;
                shdr.sh_addralign = 0x8;
                shdr.sh_flags = 0;
                shdr.sh_type = SHT_RELA
            },
            SectionType::None => shdr.sh_type = SHT_NULL,
        }
        shdr
    }
}

// r_offset: 17 r_typ: 4 r_sym: 12 r_addend: fffffffffffffffc rela: true,
pub struct RelocationBuilder {
    rel: bool,
    addend: isize,
    sym_idx: usize,
    offset: usize,
    typ: u32,
}

impl RelocationBuilder {
    pub fn new(typ: u32) -> Self {
        RelocationBuilder {
            rel: false,
            addend: 0,
            offset: 0,
            sym_idx: 0,
            typ,
        }
    }
    pub fn rel(mut self) -> Self {
        self.rel = true; self
    }
    pub fn addend(mut self, addend: isize) -> Self {
        self.rel = false;
        self.addend = addend; self
    }
    pub fn offset(mut self, offset: usize) -> Self {
        self.offset = offset; self
    }
    pub fn sym(mut self, sym_idx: usize) -> Self {
        self.sym_idx = sym_idx; self
    }
    pub fn create(self) -> Relocation {
        Relocation {
            r_offset: self.offset,
            r_addend: self.addend,
            r_sym: self.sym_idx,
            r_type: self.typ,
            is_rela: !self.rel,
        }
    }
}

//#[derive(Debug)]
pub struct Elf {
    name: String,
    code: OrderMap<StringIndex, Code>,
    data: OrderMap<StringIndex, Data>,
    relocations: HashMap<StringIndex, (Section, Vec<Relocation>)>,
    symbols: OrderMap<StringIndex, Symbol>,
    section_symbols: OrderMap<StringIndex, Symbol>,
    imports: HashMap<StringIndex, u64>,
    sections: HashMap<StringIndex, Section>,
    offsets: HashMap<StringIndex, Offset>,
    sizeof_strtab: Offset,
    strings: shawshank::ArenaSet<String>,
    sizeof_bits: Offset,
    nsections: u16,
    ctx: Ctx,
    target: Target,
}

impl fmt::Debug for Elf {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        writeln!(fmt, "{}", self.name)?;
        writeln!(fmt, "{:?}", self.code)?;
        writeln!(fmt, "{:?}", self.        data)?;
        writeln!(fmt, "{:#?}", self.        relocations)?;
        writeln!(fmt, "{:#?}", self.        symbols)?;
        writeln!(fmt, "{:?}", self.        imports)?;
        writeln!(fmt, "{:?}", self.        sections)?;
        writeln!(fmt, "{:?}", self.        offsets)?;
        writeln!(fmt, "SizeofStrtab: {:?}", self.        sizeof_strtab)?;
        writeln!(fmt, "SizeofBits: {:?}", self.        sizeof_bits)?;
        //writeln!(fmt, "SymtabOffset: {:?}", self.        symtab_offset)?;
        writeln!(fmt, "Strings: {:?}", self.        strings.count())?;
        writeln!(fmt, "{:?}", self.        ctx)
    }
}

const STRTAB_LINK: u16 = 1;
const SYMTAB_LINK: u16 = 2;

impl Elf {
    pub fn new(name: Option<String>, target: Target) -> Self {
        let ctx = Ctx::from(target.clone());
        let name = name.unwrap_or("goblin".to_owned());
        let mut offsets = HashMap::new();
        let mut strings = shawshank::string_arena_set();
        let mut section_symbols = OrderMap::new();
        let mut sizeof_strtab = 1;

        {
            let mut push_strtab = |name: &str| {
                let name = name.to_owned();
                let size = name.len() + 1;
                let idx = strings.intern(name).unwrap();
                let offset = sizeof_strtab;
                offsets.insert(idx, offset);
                sizeof_strtab += size;
                (idx, offset)
            };

            push_strtab("strtab");
            push_strtab("symtab");
            let (idx, offset) = push_strtab(&name);
            // NOTE: using 0 as the idx is a hack;
            // but we need to insert a null symbol as the first symbol, otherwise linkers explode
            section_symbols.insert(0, Symbol::default());
            section_symbols.insert(idx, SymbolBuilder::new(SymbolType::File).name_offset(offset).create());

        }

        let sizeof_bits = Header::size(&ctx);
        Elf {
            name,
            code:        OrderMap::new(),
            data:        OrderMap::new(),
            relocations: HashMap::new(),
            imports:     HashMap::new(),
            symbols:     OrderMap::new(),
            section_symbols,
            sections:    HashMap::new(),
            nsections:   3,
            offsets,
            strings,
            sizeof_strtab,
            sizeof_bits,
            ctx,
            target,
        }
    }
    fn new_string(&mut self, name: String) -> (StringIndex, usize) {
        let size = name.len() + 1;
        let offset = self.sizeof_strtab;
        let idx = self.strings.intern(name).unwrap();
        self.offsets.insert(idx, offset);
        self.sizeof_strtab += size;
        (idx, offset)
    }
    pub fn add_code(&mut self, name: String, code: Code) {
        // intern section and symbol name strings
        let (_reloc_idx, _reloc_section_offset) = self.new_string(format!(".reloc.text.{}", name));
        let (_text_idx, section_offset) = self.new_string(format!(".text.{}", name));
        // can do prefix optimization here actually, because .text.*
        let (idx, offset) = self.new_string(name);
        // store the size of this code
        let size = code.len();
        debug!("idx: {:?} @ {:#x} - new strtab offset: {:#x}", idx, offset, self.sizeof_strtab);
        // build symbol based on this 
        let mut symbol = SymbolBuilder::new(SymbolType::Function).size(size).name_offset(offset).global().create();
        // the symbols section reference/index will be the current number of sections
        symbol.st_shndx = self.symbols.len() + 3; // null + strtab + symtab
        let mut section_symbol = SymbolBuilder::new(SymbolType::Section).create(); //.name_offset(section_offset)
        // the symbols section reference/index will be the current number of sections
        section_symbol.st_shndx = self.symbols.len() + 3; // null + strtab + symtab
        // insert it into our symbol table
        self.symbols.insert(idx, symbol);
        self.section_symbols.insert(idx, section_symbol);
        // FIXME: probably add padding alignment
        // create a PROGBITS section based on these code bytes
        let mut section = SectionBuilder::new(size as u64).name_offset(section_offset).section_type(SectionType::Bits).exec().create(&self.ctx);
        // the offset is the head of how many program bits we've added
        section.sh_offset = self.sizeof_bits as u64;
        self.sections.insert(idx, section);
        self.nsections += 1;
        // increment the size
        self.sizeof_bits += size;

        self.code.insert(idx, code);
    }
    pub fn add_data(&mut self, name: String, data: Data) {
        let (_reloc_idx, _reloc_section_offset) = self.new_string(format!(".reloc.data.{}", name));
        let (_text_idx, section_offset) = self.new_string(format!(".data.{}", name));
        let (idx, offset) = self.new_string(name);

        // we add +4 because using -4 addend for all PC32 relocs for now because ihavenoideawhatimdoing
        let size = data.len();
        debug!("idx: {:?} @ {:#x} - new strtab offset: {:#x}", idx, offset, self.sizeof_strtab);
        // TODO: extract this into a local function, DRY and shit
        let mut symbol = SymbolBuilder::new(SymbolType::Elf).size(size).name_offset(offset).global().create();
        symbol.st_shndx = self.symbols.len() + 3; // null + strtab + symtab
        let mut section_symbol = SymbolBuilder::new(SymbolType::Section).create(); //.name_offset(section_offset)
        // the symbols section reference/index will be the current number of sections
        section_symbol.st_shndx = self.symbols.len() + 3; // null + strtab + symtab
        // insert it into our symbol table
        self.symbols.insert(idx, symbol);
        self.section_symbols.insert(idx, section_symbol);

        let mut section = SectionBuilder::new(size as u64).name_offset(section_offset).section_type(SectionType::String).create(&self.ctx);
        // the offset is the head of how many program bits we've added
        section.sh_offset = self.sizeof_bits as u64;
        section.sh_entsize = 1;
        self.sections.insert(idx, section);
        self.nsections += 1;
        self.sizeof_bits += size;

        self.data.insert(idx, data);
    }
    pub fn import(&mut self, import: String) {
        let (idx, offset) = self.new_string(import);
        let symbol = SymbolBuilder::new(SymbolType::Import).name_offset(offset).create();
        self.symbols.insert(idx, symbol);
    }
    pub fn link_import(&mut self, caller: &str, import: &str, offset: usize) {
        let idx = self.strings.intern(caller).unwrap();
        let import_idx = self.strings.intern(import).unwrap();
        let (idx, _, _) = self.symbols.get_pair_index(&idx).unwrap();
        let (import_idx, _, _) = self.symbols.get_pair_index(&import_idx).unwrap();
        let mut reloc = RelocationBuilder::new(reloc::R_X86_64_PLT32).sym(import_idx + self.section_symbols.len()).offset(offset).create();
        reloc.r_addend = -4;
        self.add_reloc(caller, reloc, idx)
    }
    pub fn link(&mut self, to: &str, from: &str, offset: usize) {
        let idx = self.strings.intern(to).unwrap();
        let from_idx = self.strings.intern(from).unwrap();
        let (idx, _, _) = self.symbols.get_pair_index(&idx).unwrap();
        let (from_idx, _, _) = self.symbols.get_pair_index(&from_idx).unwrap();
        // +2 for NOTYPE and FILE symbols
        let mut reloc = RelocationBuilder::new(reloc::R_X86_64_PC32).sym(from_idx + 2).offset(offset).create();
        // TODO: figure out wtf addends are for ;)
        reloc.r_addend = 0;
        self.add_reloc(to, reloc, idx)
    }
    fn add_reloc(&mut self, relocee: &str, reloc: Relocation, idx: usize) {
        debug!("add reloc for symbol {} - reloc: {:?}", idx, &reloc);
        let reloc_size = Relocation::size(reloc.is_rela, self.ctx) as u64;
        if self.relocations.contains_key(&idx) {
            debug!("{} has relocs", relocee);
            let &mut (ref mut section, ref mut relocs) = self.relocations.get_mut(&idx).unwrap();
            // its size is currently how many relocations there are
            section.sh_size += section.sh_entsize;
            relocs.push(reloc);
        } else {
            debug!("{} does NOT have relocs", relocee);
            // now create the relocation section
            let (_reloc_idx, reloc_section_offset) = self.new_string(format!(".reloc.{}", relocee));
            let mut reloc_section = SectionBuilder::new(reloc_size).name_offset(reloc_section_offset).section_type(SectionType::Relocation).create(&self.ctx);
            // its sh_link always points to the symtable
            reloc_section.sh_link = SYMTAB_LINK as u32;
            // info tells us which relocation this is relative to
            reloc_section.sh_info = (idx + 3) as u32;
            self.relocations.insert(idx, (reloc_section, vec![reloc]));
            self.nsections += 1;
        }
    }
    pub fn write<T: Write + Seek>(mut self, file: T) -> goblin::error::Result<()> {
        let mut file = BufWriter::new(file);
        /////////////////////////////////////
        // Compute Offsets
        /////////////////////////////////////
        let sizeof_symtab = (self.symbols.len() + self.section_symbols.len()) * Symbol::size(self.ctx.container);
        let sizeof_relocs = self.relocations.iter().fold(0, |acc, (_, &(ref _shdr, ref rels))| rels.len() + acc) * Relocation::size(true, self.ctx);
        let strtab_offset = self.sizeof_bits as u64;
        let symtab_offset = strtab_offset + self.sizeof_strtab as u64;
        let reloc_offset = symtab_offset + sizeof_symtab as u64;
        let sh_offset = reloc_offset + sizeof_relocs as u64;

        debug!("strtab: {:#x} symtab {:#x} relocs {:#x} sh_offset {:#x}", strtab_offset, symtab_offset, reloc_offset, sh_offset);

        /////////////////////////////////////
        // Header
        /////////////////////////////////////
        let mut header = Header::new(self.ctx);
        let machine: MachineTag = self.target.clone().into();
        header.e_machine = machine.0;
        header.e_type = header::ET_REL;
        header.e_shoff = sh_offset;
        header.e_shnum = self.nsections;
        header.e_shstrndx = STRTAB_LINK;
        
        file.iowrite_with(header, self.ctx)?;
        let after_header = file.seek(Current(0))?;
        debug!("after_header {:#x}, expect: {:#x} - {}", after_header, Header::size(&self.ctx), after_header == Header::size(&self.ctx) as u64);

        /////////////////////////////////////
        // Code
        /////////////////////////////////////

        for (_idx, function) in self.code.drain(..) {
            file.write(function.as_slice())?;
        }
        let after_code = file.seek(Current(0))?;
        debug!("after_code {:#x}, expect: {:#x} - {}", after_code, strtab_offset, after_code == strtab_offset);

        /////////////////////////////////////
        // Data
        /////////////////////////////////////

        for (_idx, data) in self.data.drain(..) {
            file.write(data.as_slice())?;
        }
        let after_data = file.seek(Current(0))?;
        debug!("after_data {:#x}, expect: {:#x} - {}", after_data, strtab_offset, after_data == strtab_offset);

        /////////////////////////////////////
        // Init sections
        /////////////////////////////////////

        let mut section_headers = vec![SectionHeader::default()];
        let mut strtab = {
            let offset = *(self.offsets.get(&0).unwrap());
            SectionBuilder::new(self.sizeof_strtab as u64).name_offset(offset).section_type(SectionType::StrTab).create(&self.ctx)
        };
        strtab.sh_offset = strtab_offset;
        section_headers.push(strtab);

        let mut symtab = {
            let offset = *(self.offsets.get(&1).unwrap());
            SectionBuilder::new(sizeof_symtab as u64).name_offset(offset).section_type(SectionType::SymTab).create(&self.ctx)
        };
        symtab.sh_offset = symtab_offset;
        symtab.sh_link = 1; // we link to our strtab above
        // FunFact: symtab.sh_info acts as a delimiter pointing to which are the "external" functions in the object file; if this isn't correct, it will segfault linkers or cause them to _sometimes_ emit garbage, ymmv
        symtab.sh_info = self.section_symbols.len() as u32;
        section_headers.push(symtab);

        /////////////////////////////////////
        // Strtab
        /////////////////////////////////////

        let strtab = (0..self.strings.count()).into_iter().map(|i| {
            let symbol = self.strings.disintern(i).unwrap();
            let offset = self.offsets.get(&i).unwrap();
            (*offset, symbol)
        }).collect::<Vec<(usize, String)>>();
        debug!("symbol {:?}", strtab);

        file.seek(Start(strtab_offset))?;
        file.iowrite(0u8)?; // for the null value in the strtab;
        for (_offset, string) in strtab {
            debug!("String: {:?}", string);
            file.write(string.as_str().as_bytes())?;
            file.iowrite(0u8)?;
        }
        let after_strtab = file.seek(Current(0))?;
        debug!("after_strtab {:#x}, expect: {:#x} - {}", after_strtab, symtab_offset, after_strtab == symtab_offset);

        /////////////////////////////////////
        // Symtab
        /////////////////////////////////////
        for (_id, symbol) in self.section_symbols.into_iter() {
            debug!("Section Symbol: {:?}", symbol);
            file.iowrite_with(symbol, self.ctx)?;
        }
        for (id, symbol) in self.symbols.into_iter() {
            debug!("Symbol: {:?}", symbol);
            file.iowrite_with(symbol, self.ctx)?;
            match self.sections.get(&id) {
                Some(section) => {
                    section_headers.push(section.clone());
                },
                None => () // FIXME: warn
            }
        }
        let after_symtab = file.seek(Current(0))?;
        debug!("after_symtab {:#x}, expect: {:#x} - {} - shdr_size {}", after_symtab, sh_offset, after_symtab == sh_offset, Section::size(&self.ctx) as u64);

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
                file.iowrite_with(relocation, (relocation.is_rela, self.ctx))?;
            }
        }

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
        debug!("after_shdrs {:#x}, expect: {:#x} - {}", after_shdrs, expected, after_shdrs == expected);
        debug!("done");
        Ok(())
    }
}

impl Object for Elf {
    fn to_bytes(artifact: &Artifact) -> error::Result<Vec<u8>> {
        let mut elf = Elf::new(Some(artifact.name.to_owned()), artifact.target.clone());
        for &(ref name, ref code) in &artifact.code {
            // todo just make this a reference, since we need to copy into the vector of bytes anyway
            elf.add_code(name.to_string(), code.clone());
        }
        for &(ref name, ref data) in &artifact.data {
            elf.add_data(name.to_string(), data.clone());
        }
        for &(ref referrer, ref referree, ref offset) in &artifact.links {
            elf.link(referrer, referree, *offset);
        }
        for import in &artifact.imports {
            elf.import(import.to_string());
        }
        for &(ref caller, ref import, ref offset) in &artifact.import_links {
            elf.link_import(caller, import, *offset);
        }
        let mut buffer = Cursor::new(Vec::new());
        elf.write(&mut buffer)?;
        Ok(buffer.into_inner())
    }
}
