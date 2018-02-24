//! An artifact is a platform independent binary object file format abstraction.

use string_interner::DefaultStringInterner;
use ordermap::OrderMap;
use failure::Error;

use std::io::Write;
use std::fs::File;
use std::collections::BTreeSet;

use Target;

/// A blob of binary bytes, representing a function body, or data object
pub type Data = Vec<u8>;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
/// A raw relocation and its addend, to optionally override the "auto" relocation behavior of faerie.
/// **NB**: This is implementation defined, and can break code invariants if used improperly, you have been warned.
pub struct RelocOverride {
    pub reloc: u32,
    pub addend: i32,
}

type StringID = usize;
type Relocation = (StringID, StringID, usize, Option<RelocOverride>);

/// The kinds of errors that can befall someone creating an Artifact
#[derive(Fail, Debug)]
pub enum ArtifactError {
    #[fail(display = "Undeclared symbolic reference to: {}", _0)]
    Undeclared(String),
    #[fail(display = "Attempt to define an undefined import: {}", _0)]
    ImportDefined(String),
    #[fail(display = "Attempt to add a relocation to an import: {}", _0)]
    RelocateImport(String),
    // FIXME: don't use debugging prints for decl formats
    #[fail(display = "Incompatible declarations, old declaration {:?} is incompatible with new {:?}", old, new)]
    /// An incompatble declaration occurred, please see the [absorb](enum.Decl.html#method.absorb) method on `Decl`
    IncompatibleDeclaration { old: Decl, new: Decl },
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
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Prop {
    pub global: bool,
    pub function: bool,
    pub writeable: bool,
    pub cstring: bool,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct InternalDefinition {
    prop: Prop,
    name: StringID,
    data: Data,
}
// end note
///////////////////////////////////////////////

/// The kind of declaration this is
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Decl {
    /// An import of a function/routine defined in a shared library
    FunctionImport,
    /// A GOT-based import of data defined in a shared library
    DataImport,
    /// A function defined in this artifact
    Function { global: bool },
    /// A data object defined in this artifact
    Data { global: bool, writeable: bool },
    /// A null-terminated string object defined in this artifact
    CString { global: bool }
}

impl Decl {
    /// If it is compatible, absorb the new declaration (`other`) into the old (`self`); otherwise returns an error.
    ///
    /// The rule here is "C-ish", but essentially:
    ///
    /// 1. Duplicate declarations are no-ops / ignored.
    /// 2. **If** the previous declaration was an [FunctionImport](enum.Decl.html#variant.FunctionImport) or [DataImport](enum.Decl.html#variant.DataImport),
    ///    **then** if the subsequent declaration is a corresponding matching [Function](enum.Decl.html#variant.Function) or [Data](enum.Decl.html#variant.Data)
    ///    declaration, it is said to be "upgraded", and forever after is considered a declaration in need of a definition.
    /// 3. **If** the previous declaration was a `Function` or `Data` declaration,
    ///    **then** a subsequent corresponding `FunctionImport` or `DataImport` is a no-op.
    /// 4. Anything else is a [IncompatibleDeclaration](enum.ArtifactError.html#variant.IncompatibleDeclaration) error!
    // ref https://github.com/m4b/faerie/issues/24
    // ref https://github.com/m4b/faerie/issues/18
    pub fn absorb(&mut self, other: Self) -> Result<(), Error> {
        // FIXME: i can't think of a way offhand to not clone here, without unusual contortions
        match self.clone() {
            Decl::DataImport => {
                match other {
                    // data imports can be upgraded to any kind of data declaration
                    Decl::Data { .. } => { *self = other; Ok(()) }
                    Decl::DataImport => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration { old:*self, new: other }.into()),
                }
            }
            Decl::FunctionImport => {
                match other {
                    // function imports can be upgraded to any kind of function declaration
                    Decl::Function { .. } => { *self = other; Ok(()) }
                    Decl::FunctionImport => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration { old:*self, new: other }.into()),
                }
            },
            // a previous data declaration can only be re-declared a data import, or it must match exactly the
            // next declaration
            decl@Decl::Data { .. } => {
                match other {
                    Decl::DataImport => Ok(()),
                    other => if decl == other { Ok(()) } else {
                        Err(ArtifactError::IncompatibleDeclaration { old:*self, new: other }.into())
                    }
                }
            }
            // a previous function decl can only be re-declared a function import, or it must match exactly
            // the next declaration
            decl@Decl::Function { .. } => {
                match other {
                    Decl::FunctionImport => Ok(()),
                    other => {
                        if decl == other { Ok(()) } else {
                            Err(ArtifactError::IncompatibleDeclaration { old:*self, new: other }.into())
                        }
                    },
                }
            },
            decl => if decl == other { Ok(()) } else {
                Err(ArtifactError::IncompatibleDeclaration { old:*self, new: other }.into())
            }
        }
    }
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

impl<'a> From<(&'a InternalDefinition, &'a DefaultStringInterner)> for Definition<'a> {
    fn from((def, strings): (&'a InternalDefinition, &'a DefaultStringInterner)) -> Self {
        Definition {
            name: strings.resolve(def.name).expect("internal definition to have name"),
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
}

/// The kind of import this is - either a function, or a copy relocation of data from a shared library
#[derive(Debug, Clone)]
pub enum ImportKind {
    /// A function
    Function,
    /// An imported piece of data
    Data,
}

impl ImportKind {
    fn from_decl(decl: &Decl) -> Option<Self> {
        match decl {
            &Decl::DataImport => {
                Some (ImportKind::Data)
            },
            &Decl::FunctionImport => {
                Some (ImportKind::Function)
            },
            _ => None
        }
    }
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
        self.name = Some(name);
        self
    }
    /// Set whether this will be a static library or not
    pub fn library(mut self, is_library: bool) -> Self {
        self.library = is_library;
        self
    }
    pub fn finish(self) -> Artifact {
        let name = self.name.unwrap_or("faerie.o".to_owned());
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
    pub target: Target,
    /// Whether this is a static library or not
    pub is_library: bool,
    // will keep this for now; may be useful to pre-partition code and data vectors, not sure
    code: Vec<(StringID, Data)>,
    data: Vec<(StringID, Data)>,
    imports: Vec<(StringID, ImportKind)>,
    import_links: Vec<Relocation>,
    links: Vec<Relocation>,
    declarations: OrderMap<StringID, Decl>,
    definitions: BTreeSet<InternalDefinition>,
    strings: DefaultStringInterner,
}

// api less subject to change
impl Artifact {
    /// Create a new binary Artifact, with `target` and optional `name`
    pub fn new(target: Target, name: String) -> Self {
        Artifact {
            code: Vec::new(),
            data: Vec::new(),
            imports: Vec::new(),
            import_links: Vec::new(),
            links: Vec::new(),
            name,
            target,
            is_library: false,
            declarations: OrderMap::new(),
            definitions: BTreeSet::new(),
            strings: DefaultStringInterner::default(),
        }
    }
    /// Get an iterator over this artifact's imports
    pub fn imports<'a>(&'a self) -> Box<Iterator<Item = (&'a str, &'a ImportKind)> + 'a> {
        Box::new(self.imports.iter().map(move |&(id, ref kind)| (self.strings.resolve(id).unwrap(), kind)))
    }
    pub(crate) fn definitions<'a>(&'a self) -> Box<Iterator<Item = Definition<'a>> + 'a> {
        Box::new(self.definitions.iter().map(move |int_def| Definition::from((int_def, &self.strings))))
    }
    /// Get this artifacts relocations
    pub(crate) fn links<'a>(&'a self) -> Box<Iterator<Item = LinkAndDecl<'a>> + 'a> {
        Box::new(self.links.iter().map(move |&(ref from, ref to, ref at, ref reloc)| {
            // FIXME: I think its safe to unwrap since the links are only ever constructed by us and we
            // ensure it has a declaration
            let (ref from_decl, ref to_decl) = (self.declarations.get(from).expect("declaration present"), self.declarations.get(to).unwrap());
            let from = Binding { name: self.strings.resolve(*from).expect("from link"), decl: from_decl};
            let to = Binding { name: self.strings.resolve(*to).expect("to link"), decl: to_decl};
            LinkAndDecl {
                from,
                to,
                at: *at,
                reloc: *reloc,
            }
        }))
    }
    /// Declare and define a new symbolic reference with the given `decl` and given `definition`.
    /// This is sugar for `declare` and then `define`
    pub fn declare_with<T: AsRef<str>>(&mut self, name: T, decl: Decl, definition: Vec<u8>) -> Result<(), Error> {
        self.declare(name.as_ref(), decl)?;
        self.define(name, definition)?;
        Ok(())
    }
    /// Declare a new symbolic reference, with the given `decl`.
    /// **Note**: All declarations _must_ precede their definitions.
    pub fn declare<T: AsRef<str>>(&mut self, name: T, decl: Decl) -> Result<(), Error> {
        let decl_name = self.strings.get_or_intern(name.as_ref());
        let previous_was_import;
        let new_decl = {
            let previous_decl = self.declarations.entry(decl_name).or_insert(decl.clone());
            previous_was_import = previous_decl.is_import();
            previous_decl.absorb(decl)?;
            &*previous_decl
        };
        match new_decl {
            &Decl::DataImport | &Decl::FunctionImport => {
                // we have to check because otherwise duplicate imports cause an error
                // FIXME: ditto fixme, below, use orderset
                let mut present = false;
                for &(ref name, _) in self.imports.iter() {
                    if *name == decl_name {
                        present = true;
                    }
                }
                if !present {
                    let kind = ImportKind::from_decl(new_decl)
                        .expect("can convert from explicitly matched decls to importkind");
                    self.imports.push((decl_name, kind));
                }
                Ok(())
            }
            // we have to delete it, because it was upgraded from an import :/
            _ if previous_was_import => {
                let mut index = None;
                // FIXME: do binary search or make imports an ordermap
                for (i, &(ref name, _)) in self.imports.iter().enumerate() {
                    if *name == decl_name {
                        index = Some (i);
                    }
                }
                let _ = self.imports.swap_remove(index.expect("previous import was not in the imports array"));
                Ok(())
            },
            _ => Ok(())
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
    /// Defines a _previously declared_ program object.
    /// **NB**: If you attempt to define an import, this will return an error.
    /// If you attempt to define something which has not been declared, this will return an error.
    pub fn define<T: AsRef<str>>(&mut self, name: T, data: Vec<u8>) -> Result<(), ArtifactError> {
        let decl_name = self.strings.get_or_intern(name.as_ref());
        match self.declarations.get(&decl_name) {
            Some(ref stype) => {
                let prop = match *stype {
                    &Decl::CString { global } => Prop { global, function: false, writeable: false, cstring: true },
                    &Decl::Data { global, writeable } => Prop { global, function: false, writeable, cstring: false },
                    &Decl::Function { global } => Prop { global, function: true, writeable: false, cstring: false},
                    _ if stype.is_import() => return Err(ArtifactError::ImportDefined(name.as_ref().to_string()).into()),
                    _ => unimplemented!("New Decl variant added but not covered in define method"),
                };
                self.definitions.insert(InternalDefinition {
                    name: decl_name,
                    data,
                    prop,
                });
            }
            None => return Err(ArtifactError::Undeclared(name.as_ref().to_string())),
        }
        Ok(())
    }
    /// Declare `import` to be an import with `kind`.
    /// This is just sugar for `declare("name", Decl::FunctionImport)` or `declare("data", Decl::DataImport)`
    pub fn import<T: AsRef<str>>(&mut self, import: T, kind: ImportKind) -> Result<(), Error> {
        self.declare(
            import.as_ref(),
            match &kind {
                &ImportKind::Function => Decl::FunctionImport,
                &ImportKind::Data => Decl::DataImport,
            },
        )?;
        Ok(())
    }
    /// Link a relocation at `link.at` from `link.from` to `link.to`
    /// **NB**: If either `link.from` or `link.to` is undeclared, then this will return an error.
    /// If `link.from` is an import you previously declared, this will also return an error.
    pub fn link<'a>(&mut self, link: Link<'a>) -> Result<(), Error> {
        self.link_aux(link, None)
    }
    /// A variant of `link` with a RelocOverride provided. Has all of the same invariants as
    /// `link`.
    pub fn link_with<'a>(&mut self, link: Link<'a>, reloc: RelocOverride) -> Result<(), Error> {
        self.link_aux(link, Some(reloc))
    }

    /// Shared implementation of `link` and `link_with`.
    fn link_aux<'a>(&mut self, link: Link<'a>, reloc: Option<RelocOverride>) -> Result<(), Error> {
        let (link_from, link_to) = (self.strings.get_or_intern(link.from), self.strings.get_or_intern(link.to));
        match (self.declarations.get(&link_from), self.declarations.get(&link_to)) {
            (Some(ref from_type), Some(_)) => {
                if from_type.is_import() {
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
