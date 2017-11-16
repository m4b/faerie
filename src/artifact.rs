use ordermap::OrderMap;
use failure::Error;

use std::io::{Write};
use std::fs::{File};

use Target;
use Code;
use Data;

pub type Relocation = (String, String, usize);

#[derive(Fail, Debug)]
pub enum ArtifactError {
    #[fail(display = "Undeclared symbolic reference to: {}", _0)]
    Undeclared (String),
    #[fail(display = "Attempt to define an undefined import: {}", _0)]
    ImportDefined(String)
}

#[derive(Debug)]
pub struct Prop {
    pub local: bool,
    pub function: bool,
}

// FIXME: choose better name, def something shorter, perhaps Decl
#[derive(Debug)]
pub enum SymbolType {
    FunctionImport,
    DataImport,
    Function { local: bool },
    Data { local: bool },
}

impl SymbolType {
    pub fn is_import(&self) -> bool {
        use SymbolType::*;
        match *self {
            FunctionImport => true,
            DataImport => true,
            _ => false,
        }
    }
}

pub(crate) struct Binding<'a> {
    pub name: &'a str,
    pub kind: &'a SymbolType,
}

pub(crate) struct LinkAndDecl<'a> {
    pub from: Binding<'a>,
    pub to: Binding<'a>,
    pub at: usize,
}

pub(crate) struct Definition<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub prop: &'a Prop,
}

/// An abstract relocation linking one symbol to another, at an offset
pub struct Link<'a> {
    /// The relocation is relative `from` this symbol
    pub from: &'a str,
    /// The relocation is `to` this symbol
    pub to: &'a str,
    /// The byte offset _relative_ to `from` where the relocation should be performed
    pub at: usize,
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
    pub name: String,
    pub target: Target,
    pub is_library: bool,
    code: Vec<(String, Code)>,
    data: Vec<(String, Data)>,
    imports: Vec<(String, ImportKind)>,
    import_links: Vec<Relocation>,
    links: Vec<Relocation>,
    all_links: Vec<Relocation>,
    declarations: OrderMap<String, SymbolType>,
    definitions: Vec<(String, Data, Prop)>,
}

// api completely subject to change
impl Artifact {
    /// Create a new binary Artifact, with `target` and optional `name`
    pub fn new(target: Target, name: Option<String>) -> Self {
        Artifact {
            code: Vec::new(),
            data: Vec::new(),
            imports: Vec::new(),
            import_links: Vec::new(),
            all_links: Vec::new(),
            links: Vec::new(),
            name: name.unwrap_or("goblin".to_owned()),
            target,
            is_library: false,
            declarations: OrderMap::new(),
            definitions: Vec::new(),
        }
    }
    /// Get this artifacts code vector
    pub fn code(&self) -> &[(String, Code)] {
        &self.code
    }
    /// Get this artifacts data vector
    pub fn data(&self) -> &[(String, Data)] {
        &self.data
    }
    /// Get this artifacts import vector
    pub fn imports(&self) -> &[(String, ImportKind)] {
        &self.imports
    }
    /// Get this artifacts relocations
    pub fn links(&self) -> &[(Relocation)] {
        &self.links
    }
    pub(crate) fn definitions<'a>(&'a self) -> Box<Iterator<Item = Definition<'a>> + 'a> {
        Box::new(self.definitions.iter().map(move |&(ref name, ref data, ref prop)| {
            Definition {
                name,
                data,
                prop,
            }
        }))
    }
    // FIXME: rename to links after return value determined
    pub(crate) fn all_links<'a>(&'a self) -> Box<Iterator<Item = LinkAndDecl<'a>> + 'a> {
        Box::new(self.all_links.iter().map(move |&(ref from, ref to, ref at)| {
            // FIXME: I think its safe to unwrap since the links are only ever constructed by us and we
            // ensure it has a declaration
            let (ref from_type, ref to_type) = (self.declarations.get(from).unwrap(), self.declarations.get(to).unwrap());
            LinkAndDecl {
                from: Binding { name: from, kind: from_type},
                to: Binding { name: to, kind: to_type},
                at: *at,
            }
        }))
    }
    // FIXME: deprecated/remove
    /// Get this artifacts import relocations
    pub fn import_links(&self) -> &[(Relocation)] {
        &self.import_links
    }
    // FIXME: deprecated/remove
    /// Add a new function with `name`, whose body is in `code`
    pub fn add_code<T: AsRef<str>>(&mut self, name: T, code: Code) {
        if !self.declarations.contains_key(name.as_ref()) {
            panic!("Declaration not found for {}", name.as_ref());
        }
        self.code.push((name.as_ref().to_string(), code));
    }
    // FIXME: deprecated/remove
    /// Add a byte blob of non-function data
    pub fn add_data<T: AsRef<str>>(&mut self, name: T, data: Data) {
        if !self.declarations.contains_key(name.as_ref()) {
            panic!("Declaration not found for {}", name.as_ref());
        }
        self.data.push((name.as_ref().to_string(), data));
    }
    pub fn define<T: AsRef<str>>(&mut self, name: T, data: Vec<u8>) -> Result<(), ArtifactError> {
        let decl_name = name.as_ref().to_string();
        match self.declarations.get(&decl_name) {
            Some(ref stype) => {
                let prop = match *stype {
                    &SymbolType::Data { local } => Prop { local, function: false },
                    &SymbolType::Function { local } => Prop { local, function: true },
                    _ if stype.is_import() => return Err(ArtifactError::ImportDefined(name.as_ref().to_string()).into()),
                    _ => unimplemented!("New SymbolType variant added but not covered in define method"),
                };
                self.definitions.push((decl_name, data, prop));
            },
            None => {
                return Err(ArtifactError::Undeclared(decl_name))
            }
        }
        Ok(())
    }
    /// Declare a new symbolic reference, with the given `kind`.
    /// **Note**: All declarations _must_ precede their definitions.
    pub fn declare<T: AsRef<str>>(&mut self, name: T, kind: SymbolType) {
        let decl_name = name.as_ref().to_string();
        match kind {
            SymbolType::DataImport => self.imports.push((decl_name.clone(), ImportKind::Data)),
            SymbolType::FunctionImport => self.imports.push((decl_name.clone(), ImportKind::Function)),
            _ => ()
        }
        // FIXME: error out when there's a duplicate declaration
        self.declarations.insert(decl_name, kind);
    }
    // FIXME: have this be sugar and add the decl as well
    /// Create a new function import, to be used subsequently in [link_import](struct.Artifact.method#link_import.html)
    pub fn import<T: AsRef<str>>(&mut self, import: T, kind: ImportKind) {
        self.imports.push((import.as_ref().to_string(), kind));
    }
    // FIXME: DEPRECATED/remove
    /// Link a new relocation at offset `Link.at` into the caller at `Link.from`, for the import at `Link.to`
    pub fn link_import<'a>(&mut self, link: Link<'a>) {
        self.import_links.push((link.from.to_string(), link.to.to_string(), link.at));
    }
    /// FIXME: add docs, replace link with this
    pub fn link2<'a>(&mut self, link: Link<'a>) -> Result<(), Error> {
        match (self.declarations.get(link.from), self.declarations.get(link.to)) {
            (Some(ref _from_type), Some(ref to_type)) => {
                let link = (link.from.to_string(), link.to.to_string(), link.at);
                self.all_links.push(link.clone());
                if to_type.is_import() {
                    self.import_links.push(link);
                } else {
                    self.links.push(link);
                }
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
    /// Link a relocation into `object` at `offset`, referring to `reference` (currently, this will be a simple data object, like a string you previously added via add_data)
    pub fn link<'a>(&mut self, link: Link<'a>) {
        self.links.push((link.from.to_string(), link.to.to_string(), link.at));
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
