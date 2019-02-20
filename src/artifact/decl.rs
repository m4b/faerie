use crate::artifact::ArtifactError;
use failure::Error;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Decl {
    /// A function defined in this artifact
    Function(FunctionDecl),
    /// A data object defined in this artifact
    Data(DataDecl),
    /// A null-terminated string object defined in this artifact
    CString(CStringDecl),
    /// A DWARF debug section defined in this artifact
    DebugSection(DebugSectionDecl),
}

impl Decl {
    /// Accessor to determine whether scope is global
    pub fn is_global(&self) -> bool {
        match self {
            Decl::Function(f) => f.is_global(),
            Decl::Data(d) => d.is_global(),
            Decl::CString(cs) => cs.is_global(),
            Decl::DebugSection(ds) => ds.is_global(),
        }
    }

    /// Accessor to determine whether contents are writable
    pub fn is_writable(&self) -> bool {
        match self {
            Decl::Data(d) => d.is_writable(),
            Decl::Function { .. } | Decl::CString { .. } | Decl::DebugSection { .. } => false,
        }
    }
}

impl Decl {
    /// A function defined in this artifact
    pub fn function() -> FunctionDecl {
        FunctionDecl::default()
    }
    /// A data object defined in this artifact
    pub fn data() -> DataDecl {
        DataDecl::default()
    }
    /// A null-terminated string object defined in this artifact
    pub fn cstring() -> CStringDecl {
        CStringDecl::default()
    }
    /// A DWARF debug section defined in this artifact
    pub fn debug_section() -> DebugSectionDecl {
        DebugSectionDecl::default()
    }
/* XXX this has to move if imports are a separate thing
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
            Decl::Import(ImportKind::Data) => {
                match other {
                    // data imports can be upgraded to any kind of data declaration
                    Decl::Artifact(Decl::Data { .. }) => {
                        *self = other;
                        Ok(())
                    }
                    Decl::Import(ImportKind::Data) => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration {
                        old: *self,
                        new: other,
                    }
                    .into()),
                }
            }
            Decl::Import(ImportKind::Function) => {
                match other {
                    // function imports can be upgraded to any kind of function declaration
                    Decl::Artifact(ADecl::Function { .. }) => {
                        *self = other;
                        Ok(())
                    }
                    Decl::Import(ImportKind::Function) => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration {
                        old: *self,
                        new: other,
                    }
                    .into()),
                }
            }
            // a previous data declaration can only be re-declared a data import, or it must match exactly the
            // next declaration
            decl @ Decl::Artifact(ADecl::Data { .. }) => match other {
                Decl::Import(ImportKind::Data) => Ok(()),
                other => {
                    if decl == other {
                        Ok(())
                    } else {
                        Err(ArtifactError::IncompatibleDeclaration {
                            old: *self,
                            new: other,
                        }
                        .into())
                    }
                }
            },
            // a previous function decl can only be re-declared a function import, or it must match exactly
            // the next declaration
            decl @ Decl::Artifact(ADecl::Function { .. }) => match other {
                Decl::Import(ImportKind::Function) => Ok(()),
                other => {
                    if decl == other {
                        Ok(())
                    } else {
                        Err(ArtifactError::IncompatibleDeclaration {
                            old: *self,
                            new: other,
                        }
                        .into())
                    }
                }
            },
            decl => {
                if decl == other {
                    Ok(())
                } else {
                    Err(ArtifactError::IncompatibleDeclaration {
                        old: *self,
                        new: other,
                    }
                    .into())
                }
            }
        }
    }
*/
    /// Is this a section?
    pub fn is_section(&self) -> bool {
        match *self {
            Decl::DebugSection { .. } => true,
            _ => false,
        }
    }
}


#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunctionDecl {
    global: bool,
}

impl Default for FunctionDecl {
    fn default() -> Self {
        FunctionDecl { global: false }
    }
}

impl FunctionDecl {
    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
    pub fn local(mut self) -> Self {
        self.global = false;
        self
    }
    pub fn is_global(&self) -> bool {
        self.global
    }
}

impl Into<Decl> for FunctionDecl {
    fn into(self) -> Decl {
        Decl::Function(self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataDecl {
    global: bool,
    writable: bool,
}

impl Default for DataDecl {
    fn default() -> Self {
        DataDecl {
            global: false,
            writable: false,
        }
    }
}

impl DataDecl {
    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
    pub fn local(mut self) -> Self {
        self.global = false;
        self
    }
    pub fn is_global(&self) -> bool {
        self.global
    }
    pub fn writable(mut self) -> Self {
        self.writable = true;
        self
    }
    pub fn read_only(mut self) -> Self {
        self.writable = false;
        self
    }
    pub fn is_writable(&self) -> bool {
        self.writable
    }
}

impl Into<Decl> for DataDecl {
    fn into(self) -> Decl {
        Decl::Data(self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CStringDecl {
    global: bool,
}

impl Default for CStringDecl {
    fn default() -> Self {
        CStringDecl { global: false }
    }
}

impl CStringDecl {
    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
    pub fn local(mut self) -> Self {
        self.global = false;
        self
    }
    pub fn is_global(&self) -> bool {
        self.global
    }
}

impl Into<Decl> for CStringDecl {
    fn into(self) -> Decl {
        Decl::CString(self)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DebugSectionDecl {}

impl DebugSectionDecl {
    pub fn is_global(&self) -> bool {
        false
    }
}

impl Default for DebugSectionDecl {
    fn default() -> Self {
        DebugSectionDecl {}
    }
}

impl Into<Decl> for DebugSectionDecl {
    fn into(self) -> Decl {
        Decl::DebugSection(self)
    }
}
