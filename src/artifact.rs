use std::io::{Write};
use std::fs::{File};

use error;
use Target;
use Code;
use Data;

#[derive(Debug)]
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
    pub fn add_code<T: ToString>(&mut self, name: T, code: Code) {
        self.code.push((name.to_string(), code));
    }
    pub fn add_data<T: ToString>(&mut self, name: T, data: Data) {
        self.data.push((name.to_string(), data));
    }
    pub fn import<T: ToString>(&mut self, import: T) {
        self.imports.push(import.to_string());
    }
    pub fn link_import<T: ToString>(&mut self, caller: T, import: T, offset: usize) {
        self.import_links.push((caller.to_string(), import.to_string(), offset));
    }
    pub fn link<T: ToString>(&mut self, to: T, from: T, offset: usize) {
        self.links.push((to.to_string(), from.to_string(), offset));
    }
    pub fn emit<O: Object>(&self) -> error::Result<Vec<u8>> {
        O::to_bytes(self)
    }
    pub fn write<O: Object>(&self, mut sink: File) -> error::Result<()> {
        let bytes = self.emit::<O>()?;
        sink.write_all(&bytes)?;
        Ok(())
    }
}

pub trait Object {
    fn to_bytes(artifact: &Artifact) -> error::Result<Vec<u8>>;
}
