//! An artifact is a platform independent binary object file format abstraction.

use failure::Error;
use indexmap::IndexMap;
use string_interner::StringInterner;
use target_lexicon::{BinaryFormat, Triple};

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Write;

use crate::{elf, mach};

pub(crate) mod decl;
pub use crate::artifact::decl::{
    DataType, Decl, DefinedDecl, ImportKind, Scope, SectionKind, Visibility,
};

// we need Ord so that `InternalDefinition` can go in a BTreeSet
/// The data to be stored in an artifact, representing a function body or data object.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum Data {
    /// A blob of binary bytes, representing a function body, or data object
    Blob(Vec<u8>),
    /// Zero-initialized data with a given size. This is implemented as a .bss section.
    ZeroInit(usize),
}

/// The kind of relocation for a link.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum Reloc {
    /// Automatic relocation determined by the `from` and `to` of the link.
    Auto,
    /// A raw relocation and its addend, to optionally override the "auto" relocation behavior of faerie.
    /// **NB**: This is implementation defined, and can break code invariants if used improperly, you have been warned.
    Raw {
        /// Raw relocation, as an integer value to be encoded by the backend
        reloc: u32,
        /// Raw addend, significance depends on the raw relocation used
        addend: i32,
    },
    /// A relocation in a debug section.
    Debug {
        /// Size (in bytes) of the pointer to be relocated
        size: u8,
        /// Addend for the relocation
        addend: i32,
    },
}

type StringID = usize;
type Relocation = (StringID, StringID, u64, Reloc);

/// The kinds of errors that can befall someone creating an Artifact
#[derive(Fail, Debug)]
pub enum ArtifactError {
    #[fail(display = "Undeclared symbolic reference to: {}", _0)]
    /// Undeclarated symbolic reference
    Undeclared(String),
    #[fail(display = "Attempt to define an undefined import: {}", _0)]
    /// Attempt to define an undefined import
    ImportDefined(String),
    #[fail(display = "Attempt to add a relocation to an import: {}", _0)]
    /// Attempt to use a relocation inside an import
    RelocateImport(String),
    // FIXME: don't use debugging prints for decl formats
    #[fail(
        display = "Incompatible declarations, old declaration {:?} is incompatible with new {:?}",
        old, new
    )]
    /// An incompatble declaration occurred, please see the [absorb](enum.Decl.html#method.absorb) method on `Decl`
    IncompatibleDeclaration {
        /// Previously provided declaration
        old: Decl,
        /// Declaration that caused this error
        new: Decl,
    },
    #[fail(display = "Duplicate definition of symbol: {}", _0)]
    /// A duplicate definition
    DuplicateDefinition(String),
    #[fail(
        display = "ZeroInit data is only allowed for DataDeclarations, got {:?}",
        _0
    )]
    /// ZeroInit is only allowed for data
    InvalidZeroInit(DefinedDecl),

    /// A non section declaration got custom symbols during definition.
    #[fail(
        display = "Attempt to add custom symbols {:?} to non section declaration {:?}",
        _1, _0
    )]
    NonSectionCustomSymbols(DefinedDecl, BTreeMap<String, u64>),
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct InternalDefinition {
    decl: DefinedDecl,
    name: StringID,
    symbols: BTreeMap<String, u64>,
    data: Data,
}

/// A declaration, plus a flag to track whether we have a definition for it yet
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct InternalDecl {
    decl: Decl,
    defined: bool,
}

impl Data {
    /// Return the length of the data stored in this object.
    ///
    /// For `ZeroInit` variant, returns the amount of space
    /// that would be taken up when the program is loaded into memory.
    pub fn len(&self) -> usize {
        match self {
            Data::Blob(blob) => blob.len(),
            Data::ZeroInit(_) => 0,
        }
    }
    /// Return whether the data has at least one byte defined
    pub fn is_empty(&self) -> bool {
        match self {
            Data::Blob(blob) => blob.is_empty(),
            Data::ZeroInit(size) => *size == 0,
        }
    }
    /// Return whether this data is a ZeroInit variant
    pub fn is_zero_init(&self) -> bool {
        match self {
            Data::ZeroInit(_) => true,
            Data::Blob(_) => false,
        }
    }
}

impl InternalDecl {
    /// Wrap up a declaration. Initially marked as not defined.
    pub fn new(decl: Decl) -> Self {
        Self {
            decl,
            defined: false,
        }
    }
    /// Mark a declaration as defined.
    pub fn define(&mut self) {
        self.defined = true;
    }
}

/// A binding of a raw `name` to its declaration, `decl`
#[derive(Debug)]
pub struct Binding<'a> {
    /// Name of symbol
    pub name: &'a str,
    /// Declaration of symbol
    pub decl: &'a Decl,
}

/// A relocation binding one declaration to another
#[derive(Debug)]
pub struct LinkAndDecl<'a> {
    /// Relocation is inside this symbol
    pub from: Binding<'a>,
    /// Targeting this symbol
    pub to: Binding<'a>,
    /// Offset into `from`
    pub at: u64,
    /// Type of relocation to use
    pub reloc: Reloc,
}

/// A definition of a symbol with its properties the various backends receive
#[derive(Debug, Clone)]
pub(crate) struct Definition<'a> {
    /// Name of symbol
    pub name: &'a str,
    /// Contents of definition
    pub data: &'a Data,
    /// Custom symbols referencing this section, or none for other definition types.
    pub symbols: &'a BTreeMap<String, u64>,
    /// Declaration of symbol
    pub decl: &'a DefinedDecl,
}

impl<'a> From<(&'a InternalDefinition, &'a StringInterner<StringID>)> for Definition<'a> {
    fn from((def, strings): (&'a InternalDefinition, &'a StringInterner<StringID>)) -> Self {
        Definition {
            name: strings
                .resolve(def.name)
                .expect("internal definition to have name"),
            data: &def.data,
            symbols: &def.symbols,
            decl: &def.decl,
        }
    }
}

/// An abstract relocation linking one symbol to another, at an offset
pub struct Link<'a> {
    /// The relocation is relative `from` this symbol
    pub from: &'a str,
    /// The relocation is `to` this symbol
    pub to: &'a str,
    /// The byte offset _relative_ to `from` where the relocation should be performed
    pub at: u64,
}

/// Builder for creating an artifact
pub struct ArtifactBuilder {
    target: Triple,
    name: Option<String>,
    library: bool,
}

impl ArtifactBuilder {
    /// Create a new Artifact with `target` machine architecture
    pub fn new(target: Triple) -> Self {
        ArtifactBuilder {
            target,
            name: None,
            library: false,
        }
    }
    /// Set this artifacts name
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }
    /// Set whether this will be a static library or not
    pub fn library(mut self, is_library: bool) -> Self {
        self.library = is_library;
        self
    }
    /// Build into an Artifact
    pub fn finish(self) -> Artifact {
        let name = self.name.unwrap_or_else(|| "faerie.o".to_owned());
        let mut artifact = Artifact::new(self.target, name);
        artifact.is_library = self.library;
        artifact
    }
}

#[derive(Debug, Clone)]
/// An abstract binary artifact, which contains code, data, imports, and relocations
pub struct Artifact {
    /// The name of this artifact
    pub name: String,
    /// The machine target this is intended for
    pub target: Triple,
    /// Whether this is a static library or not
    pub is_library: bool,
    // will keep this for now; may be useful to pre-partition code and data vectors, not sure
    imports: Vec<(StringID, ImportKind)>,
    links: Vec<Relocation>,
    declarations: IndexMap<StringID, InternalDecl>,
    local_definitions: BTreeSet<InternalDefinition>,
    nonlocal_definitions: BTreeSet<InternalDefinition>,
    strings: StringInterner<StringID>,
}

// api less subject to change
impl Artifact {
    /// Create a new binary Artifact, with `target` and optional `name`
    pub fn new(target: Triple, name: String) -> Self {
        Artifact {
            imports: Vec::new(),
            links: Vec::new(),
            name,
            target,
            is_library: false,
            declarations: IndexMap::new(),
            local_definitions: BTreeSet::new(),
            nonlocal_definitions: BTreeSet::new(),
            strings: StringInterner::new(),
        }
    }
    /// Get an iterator over this artifact's imports
    pub fn imports<'a>(&'a self) -> Box<dyn Iterator<Item = (&'a str, &'a ImportKind)> + 'a> {
        Box::new(
            self.imports
                .iter()
                .map(move |&(id, ref kind)| (self.strings.resolve(id).unwrap(), kind)),
        )
    }
    pub(crate) fn definitions<'a>(&'a self) -> Box<dyn Iterator<Item = Definition<'a>> + 'a> {
        Box::new(
            self.local_definitions
                .iter()
                .chain(self.nonlocal_definitions.iter())
                .map(move |int_def| Definition::from((int_def, &self.strings))),
        )
    }
    /// Get this artifacts relocations
    pub(crate) fn links<'a>(&'a self) -> Box<dyn Iterator<Item = LinkAndDecl<'a>> + 'a> {
        Box::new(
            self.links
                .iter()
                .map(move |&(ref from, ref to, ref at, ref reloc)| {
                    // FIXME: I think its safe to unwrap since the links are only ever constructed by us and we
                    // ensure it has a declaration
                    let (ref from_decl, ref to_decl) = (
                        self.declarations.get(from).expect("declaration present"),
                        self.declarations.get(to).unwrap(),
                    );
                    let from = Binding {
                        name: self.strings.resolve(*from).expect("from link"),
                        decl: &from_decl.decl,
                    };
                    let to = Binding {
                        name: self.strings.resolve(*to).expect("to link"),
                        decl: &to_decl.decl,
                    };
                    LinkAndDecl {
                        from,
                        to,
                        at: *at,
                        reloc: *reloc,
                    }
                }),
        )
    }
    /// Declare and define a new symbolic reference with the given `decl` and given `definition`.
    /// This is sugar for `declare` and then `define`
    pub fn declare_with<T: AsRef<str>, D: Into<Decl>>(
        &mut self,
        name: T,
        decl: D,
        definition: Vec<u8>,
    ) -> Result<(), Error> {
        self.declare(name.as_ref(), decl)?;
        self.define(name, definition)?;
        Ok(())
    }
    /// Declare a new symbolic reference, with the given `decl`.
    /// **Note**: All declarations _must_ precede their definitions.
    pub fn declare<T: AsRef<str>, D: Into<Decl>>(
        &mut self,
        name: T,
        decl: D,
    ) -> Result<(), ArtifactError> {
        let decl = decl.into();
        let decl_name = self.strings.get_or_intern(name.as_ref());
        let previous_was_import;
        let new_idecl = {
            let previous = self
                .declarations
                .entry(decl_name)
                .or_insert(InternalDecl::new(decl.clone()));
            previous_was_import = previous.decl.is_import();
            previous.decl.absorb(decl)?;
            previous
        };
        match new_idecl.decl {
            Decl::Import(_) => {
                // we have to check because otherwise duplicate imports cause an error
                // FIXME: ditto fixme, below, use orderset
                let mut present = false;
                for &(ref name, _) in self.imports.iter() {
                    if *name == decl_name {
                        present = true;
                    }
                }
                if !present {
                    let kind = ImportKind::from_decl(&new_idecl.decl)
                        .expect("can convert from explicitly matched decls to importkind");
                    self.imports.push((decl_name, kind));
                }
                Ok(())
            }
            // we have to delete it, because it was upgraded from an import :/
            _ if previous_was_import => {
                let mut index = None;
                // FIXME: do binary search or make imports an indexmap
                for (i, &(ref name, _)) in self.imports.iter().enumerate() {
                    if *name == decl_name {
                        index = Some(i);
                    }
                }
                let _ = self
                    .imports
                    .swap_remove(index.expect("previous import was not in the imports array"));
                Ok(())
            }
            _ => Ok(()),
        }
    }
    /// [Declare](struct.Artifact.html#method.declare) a sequence of name, [Decl](enum.Decl.html) pairs
    pub fn declarations<T: AsRef<str>, D: Iterator<Item = (T, Decl)>>(
        &mut self,
        declarations: D,
    ) -> Result<(), Error> {
        for (name, decl) in declarations {
            self.declare(name, decl)?;
        }
        Ok(())
    }
    /// Defines a _previously declared_ program object with the given data.
    /// **NB**: If you attempt to define an import, this will return an error.
    /// If you attempt to define something which has not been declared, this will return an error.
    ///
    /// See the documentation for [`Data`](type.Data.html) for the difference
    /// from `define_zero_init`.
    #[inline]
    pub fn define<T: AsRef<str>>(&mut self, name: T, data: Vec<u8>) -> Result<(), ArtifactError> {
        self.define_with_symbols(name, Data::Blob(data), BTreeMap::new())
    }

    /// Defines a _previously declared_ program object with all zeros.
    /// **NB**: If you attempt to define an import, this will return an error.
    /// If you attempt to define something which has not been declared, this will return an error.
    #[inline]
    pub fn define_zero_init<T: AsRef<str>>(
        &mut self,
        name: T,
        size: usize,
    ) -> Result<(), ArtifactError> {
        self.define_with_symbols(name, Data::ZeroInit(size), BTreeMap::new())
    }

    /// Same as `define` but also allows to add custom symbols referencing a section decl.
    ///
    /// # Examples
    ///
    /// Create a MachO file with a section called `.my_section`. This section has the content
    /// `de ad be ef`, with the symbol `a_symbol` referencing to `be`.
    ///
    /// ```rust
    /// # extern crate target_lexicon;
    /// #
    /// # use std::collections::BTreeMap;
    /// # use std::str::FromStr;
    /// #
    /// # use faerie::{Artifact, ArtifactBuilder, Data, Decl, Link, SectionKind};
    /// #
    /// let mut artifact = Artifact::new(target_lexicon::triple!("x86_64-apple-darwin"), "example".to_string());
    ///
    /// artifact.declare(".my_section", Decl::section(SectionKind::Data)).unwrap();
    ///
    /// let mut section_symbols = BTreeMap::new();
    /// section_symbols.insert("a_symbol".to_string(), 2);
    /// artifact.define_with_symbols(".my_section", Data::Blob(vec![0xde, 0xad, 0xbe, 0xef]), section_symbols).unwrap();
    ///
    /// let _blob = artifact.emit().unwrap();
    /// ```
    pub fn define_with_symbols<T: AsRef<str>>(
        &mut self,
        name: T,
        data: Data,
        symbols: BTreeMap<String, u64>,
    ) -> Result<(), ArtifactError> {
        let decl_name = self.strings.get_or_intern(name.as_ref());
        match self.declarations.get_mut(&decl_name) {
            Some(ref mut stype) => {
                if stype.defined {
                    Err(ArtifactError::DuplicateDefinition(
                        name.as_ref().to_string(),
                    ))?;
                }
                let decl = match stype.decl {
                    Decl::Defined(decl) => decl,
                    Decl::Import(_) => {
                        Err(ArtifactError::ImportDefined(name.as_ref().to_string()).into())?
                    }
                };

                match decl {
                    DefinedDecl::Section(_) => {}
                    _ => {
                        if !symbols.is_empty() {
                            return Err(ArtifactError::NonSectionCustomSymbols(decl, symbols));
                        }
                    }
                }
                match decl {
                    DefinedDecl::Data(_) => {}
                    _ => {
                        if let Data::ZeroInit(_) = data {
                            return Err(ArtifactError::InvalidZeroInit(decl));
                        }
                    }
                }

                if decl.is_global() {
                    self.nonlocal_definitions.insert(InternalDefinition {
                        name: decl_name,
                        data,
                        symbols,
                        decl,
                    });
                } else {
                    self.local_definitions.insert(InternalDefinition {
                        name: decl_name,
                        data,
                        symbols,
                        decl,
                    });
                }
                stype.define();
            }
            None => Err(ArtifactError::Undeclared(name.as_ref().to_string()))?,
        }
        Ok(())
    }
    /// Declare `import` to be an import with `kind`.
    /// This is just sugar for `declare("name", Decl::FunctionImport)` or `declare("data", Decl::DataImport)`
    pub fn import<T: AsRef<str>>(&mut self, import: T, kind: ImportKind) -> Result<(), Error> {
        self.declare(import.as_ref(), Decl::Import(kind))?;
        Ok(())
    }
    /// Link a relocation at `link.at` from `link.from` to `link.to`
    /// **NB**: If either `link.from` or `link.to` is undeclared, then this will return an error.
    /// If `link.from` is an import you previously declared, this will also return an error.
    pub fn link<'a>(&mut self, link: Link<'a>) -> Result<(), Error> {
        self.link_with(link, Reloc::Auto)
    }
    /// A variant of `link` with a `Reloc` provided. Has all of the same invariants as
    /// `link`.
    pub fn link_with<'a>(&mut self, link: Link<'a>, reloc: Reloc) -> Result<(), Error> {
        let (link_from, link_to) = (
            self.strings.get_or_intern(link.from),
            self.strings.get_or_intern(link.to),
        );
        match (
            self.declarations.get(&link_from),
            self.declarations.get(&link_to),
        ) {
            (Some(ref from_type), Some(_)) => {
                if from_type.decl.is_import() {
                    return Err(ArtifactError::RelocateImport(link.from.to_string()).into());
                }
                let link = (link_from, link_to, link.at, reloc);
                self.links.push(link);
            }
            (None, _) => {
                return Err(ArtifactError::Undeclared(link.from.to_string()).into());
            }
            (_, None) => {
                return Err(ArtifactError::Undeclared(link.to.to_string()).into());
            }
        }
        Ok(())
    }

    /// Get set of non-import declarations that have not been defined. This must be an empty set in
    /// order to `emit` the artifact.
    pub fn undefined_symbols(&self) -> Vec<String> {
        let mut syms = Vec::new();
        for (&name, _) in self
            .declarations
            .iter()
            .filter(|&(_, &int)| !int.defined && !int.decl.is_import())
        {
            syms.push(String::from(
                self.strings.resolve(name).expect("declaration has a name"),
            ));
        }
        syms
    }

    /// Emit a blob of bytes representing the object file in the format specified in the target the
    /// `Artifact` was constructed with.
    pub fn emit(&self) -> Result<Vec<u8>, Error> {
        self.emit_as(self.target.binary_format)
    }

    /// Emit a blob of bytes representing an object file in the given format.
    pub fn emit_as(&self, format: BinaryFormat) -> Result<Vec<u8>, Error> {
        let undef = self.undefined_symbols();
        if undef.is_empty() {
            match format {
                BinaryFormat::Elf => elf::to_bytes(self),
                BinaryFormat::Macho => mach::to_bytes(self),
                _ => Err(format_err!(
                    "binary format {} is not supported",
                    self.target.binary_format
                )),
            }
        } else {
            Err(format_err!(
                "the following symbols are declared but not defined: {:?}",
                undef
            ))
        }
    }

    /// Emit and write to disk a blob of bytes representing the object file in the format specified
    /// in the target the `Artifact` was constructed with.
    pub fn write(&self, sink: File) -> Result<(), Error> {
        self.write_as(sink, self.target.binary_format)
    }

    /// Emit and write to disk a blob of bytes representing an object file in the given format.
    pub fn write_as(&self, mut sink: File, format: BinaryFormat) -> Result<(), Error> {
        let bytes = self.emit_as(format)?;
        sink.write_all(&bytes)?;
        Ok(())
    }
}
