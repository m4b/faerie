use std::io::{Write, Seek};

use error;
use Target;
use Code;
use Data;

// api completely subject to change
pub trait Artifact {
    fn new(target: Target, name: Option<String>) -> Self;
    fn add_code(&mut self, name: String, code: Code);
    fn add_data(&mut self, name: String, data: Data);
    fn import(&mut self, import: String);
    fn link_import(&mut self, caller: &str, import: &str, offset: usize);
    fn link(&mut self, to: &str, from: &str, offset: usize);
    fn write<T: Write + Seek + ::std::fmt::Debug>(self, file: T) -> error::Result<()>;
}
