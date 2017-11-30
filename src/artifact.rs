//! An artifact is a platform independent binary object file format abstraction.

use ordermap::OrderMap;
use failure::Error;

use std::io::{Write};
use std::fs::{File};
use std::collections::BTreeSet;

use {Target, Data};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct RelocOverride {
    pub elftype: u32,
    pub addend: u32,
}

type Relocation = (String, String, usize, Option<RelocOverride>);

/// The kinds of errors that can befall someone creating an Artifact
#[derive(Fail, Debug)]
pub enum ArtifactError {
    #[fail(display = "Undeclared symbolic reference to: {}", _0)]
    Undeclared (String),
    #[fail(display = "Attempt to define an undefined import: {}", _0)]
    ImportDefined(String),
    #[fail(display = "Attempt to add a relocation to an import: {}", _0)]
    RelocateImport(String),
    #[fail(display = "Duplicate declaration: {}", _0)]
    DuplicateDeclaration(String),
}

///////////////////////////////////////////////
// NOTE:
// Good citizen, you are hereby forewarned:
//
// Do not change the ordering of any fields in Prop or InternalDefinition
// because:
// 1. BTreeSet depends on it
// 2. Backends (e.g. ELF) rely on it to receive the definitions as locals first, etc.
//
// If it is changed, it must obey the invariant that:
//   iteration via `definitions()` returns _local_ (i.e., non global) definitions first
//   (the ordering of properties thereafter is not specified nor currently relevant)
//   _and then_ global definitions
///////////////////////////////////////////////
/// The properties associated with a symbolic reference
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Prop {
    pub global: bool,
    pub function: bool,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct InternalDefinition {
    prop: Prop,
    name: String, // this will (eventually) be a string index from interner
    data: Data,
}
// end note
///////////////////////////////////////////////

/// The kind of declaration this is
#[derive(Debug, Copy, Clone)]
pub enum Decl {
    /// An import of a function/routine defined in a shared library
    FunctionImport,
    /// A GOT-based import of data defined in a shared library
    DataImport,
    /// A function defined in this artifact
    Function { global: bool },
    /// An data object defined in this artifact
    Data { global: bool },
}

impl Decl {
    /// Is this an import (function or data) from a shared library?
    pub fn is_import(&self) -> bool {
        use Decl::*;
        match *self {
            FunctionImport => true,
            DataImport => true,
            _ => false,
        }
    }
}

/// A binding of a raw `name` to its declaration, `decl`
pub(crate) struct Binding<'a> {
    pub name: &'a str,
    pub decl: &'a Decl,
}

/// A relocation binding one declaration to another
pub(crate) struct LinkAndDecl<'a> {
    pub from: Binding<'a>,
    pub to: Binding<'a>,
    pub at: usize,
    pub reloc: Option<RelocOverride>,
}

/// A definition of a symbol with its properties the various backends receive
#[derive(Debug)]
pub(crate) struct Definition<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub prop: &'a Prop,
}

impl<'a> From<&'a InternalDefinition> for Definition<'a> {
    fn from(def: &'a InternalDefinition) -> Self {
        Definition {
            name: &def.name,
            data: &def.data,
            prop: &def.prop,
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
    pub at: usize,
    /// Relocation type:
    pub reloc: Option<RelocOverride>,
}

/// The kind of import this is - either a function, or a copy relocation of data from a shared library
#[derive(Debug, Clone)]
pub enum ImportKind {
    /// A function
    Function,
    /// An imported piece of data
    Data,
}

/// Builder for creating an artifact
pub struct ArtifactBuilder {
    target: Target,
    name: Option<String>,
    library: bool,
}

impl ArtifactBuilder {
    /// Create a new Artifact with `target` machine architecture
    pub fn new(target: Target) -> Self {
        ArtifactBuilder {
            target,
            name: None,
            library: false,
        }
    }
    /// Set this artifacts name
    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name); self
    }
    /// Set whether this will be a static library or not
    pub fn library(mut self, is_library: bool) -> Self {
        self.library = is_library; self
    }
    pub fn finish(self) -> Artifact {
        let mut artifact = Artifact::new(self.target, self.name);
        artifact.is_library = self.library;
        artifact
    }
}

#[derive(Debug)]
/// An abstract binary artifact, which contains code, data, imports, and relocations
pub struct Artifact {
    /// The name of this artifact
    pub name: String,
    /// The machine target this is intended for
    pub target: Target,
    /// Whether this is a static library or not
    pub is_library: bool,
    // will keep this for now; may be useful to pre-partition code and data vectors, not sure
    code: Vec<(String, Data)>,
    data: Vec<(String, Data)>,
    imports: Vec<(String, ImportKind)>,
    import_links: Vec<Relocation>,
    links: Vec<Relocation>,
    declarations: OrderMap<String, Decl>,
    definitions: BTreeSet<InternalDefinition>,
}

// api less subject to change
impl Artifact {
    /// Create a new binary Artifact, with `target` and optional `name`
    pub fn new(target: Target, name: Option<String>) -> Self {
        Artifact {
            code: Vec::new(),
            data: Vec::new(),
            imports: Vec::new(),
            import_links: Vec::new(),
            links: Vec::new(),
            name: name.unwrap_or("goblin".to_owned()),
            target,
            is_library: false,
            declarations: OrderMap::new(),
            definitions: BTreeSet::new(),
        }
    }
    /// Get this artifacts import vector
    pub fn imports(&self) -> &[(String, ImportKind)] {
        &self.imports
    }
    pub(crate) fn definitions<'a>(&'a self) -> Box<Iterator<Item = Definition<'a>> + 'a> {
        Box::new(self.definitions.iter().map(Definition::from))
    }
    /// Get this artifacts relocations
    pub(crate) fn links<'a>(&'a self) -> Box<Iterator<Item = LinkAndDecl<'a>> + 'a> {
        Box::new(self.links.iter().map(move |&(ref from, ref to, ref at, ref reloc)| {
            // FIXME: I think its safe to unwrap since the links are only ever constructed by us and we
            // ensure it has a declaration
            let (ref from_decl, ref to_decl) = (self.declarations.get(from).unwrap(), self.declarations.get(to).unwrap());
            LinkAndDecl {
                from: Binding { name: from, decl: from_decl},
                to: Binding { name: to, decl: to_decl},
                at: *at,
                reloc: *reloc,
            }
        }))
    }
    /// Declare a new symbolic reference, with the given `kind`.
    /// **Note**: All declarations _must_ precede their definitions.
    pub fn declare<T: AsRef<str>>(&mut self, name: T, decl: Decl) -> Result<(), Error> {
        let decl_name = name.as_ref().to_string();
        match self.declarations.insert(decl_name.clone(), decl) {
            Some(_) => {
                match decl {
                    // ref https://github.com/m4b/faerie/issues/18
                    Decl::DataImport |
                    Decl::FunctionImport => Ok(()),
                    _ => Err(ArtifactError::DuplicateDeclaration(decl_name).into())
                }
            },
            None => {
                match decl {
                    Decl::DataImport => { self.imports.push((decl_name, ImportKind::Data)); Ok(()) }
                    Decl::FunctionImport => { self.imports.push((decl_name, ImportKind::Function)); Ok(()) }
                    _ => Ok(())
                }

            }
        }
    }
    /// [Declare](struct.Artifact.html#method.declare) a sequence of name, [Decl](enum.Decl.html) pairs
    pub fn declarations<T: AsRef<str>, D: Iterator<Item = (T, Decl)>>(&mut self, declarations: D) -> Result<(), Error> {
        for (name, decl) in declarations {
            self.declare(name, decl)?;
        }
        Ok(())
    }
    /// Defines a _previously declared_ program object.
    /// **NB**: If you attempt to define an import, this will return an error.
    /// If you attempt to define something which has not been declared, this will return an error.
    pub fn define<T: AsRef<str>>(&mut self, name: T, data: Vec<u8>) -> Result<(), ArtifactError> {
        let decl_name = name.as_ref().to_string();
        match self.declarations.get(&decl_name) {
            Some(ref stype) => {
                let prop = match *stype {
                    &Decl::Data { global } => Prop { global, function: false },
                    &Decl::Function { global } => Prop { global, function: true },
                    _ if stype.is_import() => return Err(ArtifactError::ImportDefined(name.as_ref().to_string()).into()),
                    _ => unimplemented!("New Decl variant added but not covered in define method"),
                };
                self.definitions.insert(InternalDefinition { name: decl_name, data, prop });
            },
            None => {
                return Err(ArtifactError::Undeclared(decl_name))
            }
        }
        Ok(())
    }
    /// Declare `import` to be an import with `kind`.
    /// This is just sugar for `declare("name", Decl::FunctionImport)` or `declare("data", Decl::DataImport)`
    pub fn import<T: AsRef<str>>(&mut self, import: T, kind: ImportKind) -> Result<(), Error> {
        self.declare(import.as_ref(), match &kind { &ImportKind::Function => Decl::FunctionImport, &ImportKind::Data => Decl::DataImport})?;
        self.imports.push((import.as_ref().to_string(), kind));
        Ok(())
    }
    /// Link a relocation at `link.at` from `link.from` to `link.to`
    /// **NB**: If either `link.from` or `link.to` is undeclared, then this will return an error.
    /// If `link.from` is an import you previously declared, this will also return an error.
    pub fn link<'a>(&mut self, link: Link<'a>) -> Result<(), Error> {
        match (self.declarations.get(link.from), self.declarations.get(link.to)) {
            (Some(ref from_type), Some(_)) => {
                if from_type.is_import() {
                    return Err(ArtifactError::RelocateImport(link.from.to_string()).into());
                }
                let link = (link.from.to_string(), link.to.to_string(), link.at, link.reloc);
                self.links.push(link.clone());
            },
            (None, _) => {
                return Err(ArtifactError::Undeclared(link.from.to_string()).into());
            },
            (_, None) => {
                return Err(ArtifactError::Undeclared(link.to.to_string()).into());
            }
        }
        Ok(())
    }
    /// Emit a blob of bytes that represents this object file
    pub fn emit<O: Object>(&self) -> Result<Vec<u8>, Error> {
        O::to_bytes(self)
    }
    /// Emit and write to disk a blob of bytes that represents this object file
    pub fn write<O: Object>(&self, mut sink: File) -> Result<(), Error> {
        let bytes = self.emit::<O>()?;
        sink.write_all(&bytes)?;
        Ok(())
    }
}

/// The interface for an object file which different binary container formats implement to marshall an artifact into a blob of bytes
pub trait Object {
    fn to_bytes(artifact: &Artifact) -> Result<Vec<u8>, Error>;
}
