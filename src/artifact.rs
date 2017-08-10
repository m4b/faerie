use std::io::{Write};
use std::fs::{File};

use error;
use Target;
use Code;
use Data;

#[derive(Debug)]
/// An abstract binary artifact, which contains code, data, imports, and relocations
pub struct Artifact {
    pub code: Vec<(String, Code)>,
    pub data: Vec<(String, Data)>,
    pub imports: Vec<String>,
    pub target: Target,
    pub name: String,
    pub import_links: Vec<(String, String, usize)>,
    pub links: Vec<(String, String, usize)>,
    // relocations :/
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
        }
    }
    /// Add a new function with `name`, whose body is in `code`
    pub fn add_code<T: ToString>(&mut self, name: T, code: Code) {
        self.code.push((name.to_string(), code));
    }
    /// Add a byte blob of non-function data
    pub fn add_data<T: ToString>(&mut self, name: T, data: Data) {
        self.data.push((name.to_string(), data));
    }
    /// Create a new function import, to be used subsequently in [link_import](struct.Artifact.method#import.html)
    pub fn import<T: ToString>(&mut self, import: T) {
        self.imports.push(import.to_string());
    }
    /// Link a new relocation at `offset` into `caller`, for `import`
    pub fn link_import<T: ToString>(&mut self, caller: T, import: T, offset: usize) {
        self.import_links.push((caller.to_string(), import.to_string(), offset));
    }
    /// link a relocation into `object` at `offset`, referring to `reference` (currently, this will be a simple data object, like a string you previously added via add_data)
    pub fn link<T: ToString>(&mut self, object: T, reference: T, offset: usize) {
        self.links.push((object.to_string(), reference.to_string(), offset));
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
