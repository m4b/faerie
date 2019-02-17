use crate::artifact::ArtifactError;
use failure::Error;

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
    Data { global: bool, writable: bool },
    /// A null-terminated string object defined in this artifact
    CString { global: bool },
    /// A DWARF debug section defined in this artifact
    DebugSection,
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
                    Decl::Data { .. } => {
                        *self = other;
                        Ok(())
                    }
                    Decl::DataImport => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration {
                        old: *self,
                        new: other,
                    }
                    .into()),
                }
            }
            Decl::FunctionImport => {
                match other {
                    // function imports can be upgraded to any kind of function declaration
                    Decl::Function { .. } => {
                        *self = other;
                        Ok(())
                    }
                    Decl::FunctionImport => Ok(()),
                    _ => Err(ArtifactError::IncompatibleDeclaration {
                        old: *self,
                        new: other,
                    }
                    .into()),
                }
            }
            // a previous data declaration can only be re-declared a data import, or it must match exactly the
            // next declaration
            decl @ Decl::Data { .. } => match other {
                Decl::DataImport => Ok(()),
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
            decl @ Decl::Function { .. } => match other {
                Decl::FunctionImport => Ok(()),
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
    /// Is this an import (function or data) from a shared library?
    pub fn is_import(&self) -> bool {
        use Decl::*;
        match *self {
            FunctionImport => true,
            DataImport => true,
            _ => false,
        }
    }
    /// Is this a section?
    pub fn is_section(&self) -> bool {
        match *self {
            Decl::DebugSection { .. } => true,
            _ => false,
        }
    }
}
