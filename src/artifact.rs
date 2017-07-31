use std::io::{Write};
use std::fs::{File};

use error;
use Target;
use Code;
use Data;

#[derive(Debug)]
pub struct Artifact<'a> {
    pub code: Vec<(&'a str, Code)>,
    pub data: Vec<(&'a str, Data)>,
    pub imports: Vec<&'a str>,
    pub target: Target,
    pub name: String,
    pub import_links: Vec<(&'a str, &'a str, usize)>,
    pub links: Vec<(&'a str, &'a str, usize)>,
    // relocations :/
}

// api completely subject to change
impl<'a> Artifact<'a> {
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
    pub fn add_code(&mut self, name: &'a str, code: Code) {
        self.code.push((name, code));
    }
    pub fn add_data(&mut self, name: &'a str, data: Data) {
        self.data.push((name, data));
    }
    pub fn import(&mut self, import: &'a str) {
        self.imports.push(import);
    }
    pub fn link_import(&mut self, caller: &'a str, import: &'a str, offset: usize) {
        self.import_links.push((caller, import, offset));
    }
    pub fn link(&mut self, to: &'a str, from: &'a str, offset: usize) {
        self.links.push((to, from, offset));
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
