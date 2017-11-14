use std::io::{Write};
use std::fs::{File};

use error;
use Target;
use Code;
use Data;

pub type Relocation = (String, String, usize);

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
            links: Vec::new(),
            name: name.unwrap_or("goblin".to_owned()),
            target,
            is_library: false,
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
    /// Get this artifacts import relocations
    pub fn import_links(&self) -> &[(Relocation)] {
        &self.import_links
    }
    /// Add a new function with `name`, whose body is in `code`
    pub fn add_code<T: AsRef<str>>(&mut self, name: T, code: Code) {
        self.code.push((name.as_ref().to_string(), code));
    }
    /// Add a byte blob of non-function data
    pub fn add_data<T: AsRef<str>>(&mut self, name: T, data: Data) {
        self.data.push((name.as_ref().to_string(), data));
    }
    /// Create a new function import, to be used subsequently in [link_import](struct.Artifact.method#link_import.html)
    pub fn import<T: AsRef<str>>(&mut self, import: T, kind: ImportKind) {
        self.imports.push((import.as_ref().to_string(), kind));
    }
    /// Link a new relocation at offset `Link.at` into the caller at `Link.from`, for the import at `Link.to`
    pub fn link_import<'a>(&mut self, link: Link<'a>) {
        self.import_links.push((link.from.to_string(), link.to.to_string(), link.at));
    }
    /// Link a relocation into `object` at `offset`, referring to `reference` (currently, this will be a simple data object, like a string you previously added via add_data)
    pub fn link<'a>(&mut self, link: Link<'a>) {
        self.links.push((link.from.to_string(), link.to.to_string(), link.at));
    }
    /// Emit a blob of bytes that represents this object file
    pub fn emit<O: Object>(&self) -> error::Result<Vec<u8>> {
        O::to_bytes(self)
    }
    /// Emit and write to disk a blob of bytes that represents this object file
    pub fn write<O: Object>(&self, mut sink: File) -> error::Result<()> {
        let bytes = self.emit::<O>()?;
        sink.write_all(&bytes)?;
        Ok(())
    }
}

/// The interface for an object file which different binary container formats implement to marshall an artifact into a blob of bytes
pub trait Object {
    fn to_bytes(artifact: &Artifact) -> error::Result<Vec<u8>>;
}
