extern crate faerie;
extern crate goblin;
extern crate scroll;
#[macro_use]
extern crate target_lexicon;

use anyhow::{ensure, Error};
use faerie::{Artifact, ArtifactBuilder, Decl, Link};
use goblin::elf::*;
use std::str::FromStr;

#[test]
// This test is for a known bug (issue #31).
fn file_name_is_same_as_symbol_name_issue_31() {
    const NAME: &str = "a";
    let mut obj = Artifact::new(triple!("x86_64-unknown-unknown-unknown-elf"), "a".into());
    obj.declare(NAME, Decl::function().global())
        .expect("can declare");
    obj.define(NAME, vec![1, 2, 3, 4]).expect("can define");
    println!("\n{:#?}", obj);
    let bytes = obj.emit().expect("can emit elf file");
    let bytes = bytes.as_slice();
    println!("{:?}", bytes);

    // Presently, the following expect fails, `bytes` is not a valid Elf:
    let elf = goblin::Object::parse(&bytes).expect("can parse elf file");
    match elf {
        goblin::Object::Elf(elf) => {
            assert_eq!(elf.syms.len(), 4);
            let syms = elf.syms.iter().collect::<Vec<_>>();
            let sym = syms
                .iter()
                .find(|sym| sym.st_shndx == section_header::SHN_ABS as usize)
                .expect("There should be a SHN_ABS symbol");
            assert_eq!(&elf.strtab[sym.st_name], NAME);
            assert_eq!(sym.st_type(), sym::STT_FILE);

            let sym = syms
                .iter()
                .find(|sym| sym.st_type() == sym::STT_FUNC)
                .expect("There should be a STT_FUNC symbol");
            assert_eq!(&elf.strtab[sym.st_name], NAME);
        }
        _ => {
            println!("Elf file not parsed as elf file");
            assert!(false)
        }
    }
}

#[test]
// Regression test for issue 30: previously, if a non-import symbol was declared but not defined,
// the elf emit function would panic
fn link_symbol_pair_panic_issue_30() {
    let mut obj = Artifact::new(triple!("x86_64-unknown-unknown-unknown-elf"), "t.o".into());

    obj.declare("a", Decl::function().global())
        .expect("can declare a");
    obj.declare_with("b", Decl::function().global(), vec![1, 2, 3, 4])
        .expect("can declare and define b");

    obj.link(Link {
        to: "a",
        from: "b",
        at: 0,
    })
    .expect("can link from b to a");

    assert_eq!(obj.undefined_symbols(), vec![String::from("a")]);

    // The `emit` method will check that there are undefined symbols
    // and return an error describing them:
    assert!(obj.emit().is_err());
}

#[test]
fn decl_attributes() {
    decl_tests(vec![
        DeclTestCase::new("weak_func", Decl::function().weak(), |sym, sect| {
            ensure!(sym.is_function(), "symbol is function");
            ensure!(sym.st_bind() == sym::STB_WEAK, "symbol is weak");
            ensure!(
                sym.st_visibility() == sym::STV_DEFAULT,
                "symbol is default vis"
            );
            ensure!(sect.is_executable(), "executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new("weak_data", Decl::data().weak(), |sym, sect| {
            ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
            ensure!(sym.st_bind() == sym::STB_WEAK, "symbol is weak");
            ensure!(
                sym.st_visibility() == sym::STV_DEFAULT,
                "symbol is default vis"
            );
            ensure!(!sect.is_executable(), "not executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new(
            "weak_data_writable",
            Decl::data().weak().writable(),
            |sym, sect| {
                ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
                ensure!(sym.st_bind() == sym::STB_WEAK, "symbol is weak");
                ensure!(
                    sym.st_visibility() == sym::STV_DEFAULT,
                    "symbol is default vis"
                );
                ensure!(!sect.is_executable(), "not executable");
                ensure!(sect.is_writable(), "mutable");
                Ok(())
            },
        ),
        DeclTestCase::new("weak_cstring", Decl::cstring().weak(), |sym, sect| {
            ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
            ensure!(sym.st_bind() == sym::STB_WEAK, "symbol is weak");
            ensure!(
                sym.st_visibility() == sym::STV_DEFAULT,
                "symbol is default vis"
            );
            ensure!(!sect.is_executable(), "not executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new("hidden_func", Decl::function().hidden(), |sym, sect| {
            ensure!(sym.is_function(), "symbol is func");
            ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is local");
            ensure!(sym.st_visibility() == sym::STV_HIDDEN, "symbol is hidden");
            ensure!(sect.is_executable(), "executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new("hidden_data", Decl::data().hidden(), |sym, sect| {
            ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
            ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is local");
            ensure!(sym.st_visibility() == sym::STV_HIDDEN, "symbol is hidden");
            ensure!(!sect.is_executable(), "not executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new("hidden_cstring", Decl::cstring().hidden(), |sym, sect| {
            ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
            ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is weak");
            ensure!(sym.st_visibility() == sym::STV_HIDDEN, "symbol is hidden");
            ensure!(!sect.is_executable(), "not executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new(
            "protected_func",
            Decl::function().protected(),
            |sym, sect| {
                ensure!(sym.is_function(), "symbol is func");
                ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is local");
                ensure!(
                    sym.st_visibility() == sym::STV_PROTECTED,
                    "symbol is protected"
                );
                ensure!(sect.is_executable(), "executable");
                ensure!(!sect.is_writable(), "immutable");
                Ok(())
            },
        ),
        DeclTestCase::new("protected_data", Decl::data().protected(), |sym, sect| {
            ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
            ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is local");
            ensure!(
                sym.st_visibility() == sym::STV_PROTECTED,
                "symbol is protected"
            );
            ensure!(!sect.is_executable(), "not executable");
            ensure!(!sect.is_writable(), "immutable");
            Ok(())
        }),
        DeclTestCase::new(
            "protected_cstring",
            Decl::cstring().protected(),
            |sym, sect| {
                ensure!(sym.st_type() == sym::STT_OBJECT, "symbol is object");
                ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is weak");
                ensure!(
                    sym.st_visibility() == sym::STV_PROTECTED,
                    "symbol is protected"
                );
                ensure!(!sect.is_executable(), "not executable");
                ensure!(!sect.is_writable(), "immutable");
                Ok(())
            },
        ),
        DeclTestCase::new("ordinary_func", Decl::function(), |sym, sect| {
            ensure!(sym.is_function(), "symbol is function");
            ensure!(sym.st_bind() == sym::STB_LOCAL, "symbol is local");
            ensure!(
                sym.st_visibility() == sym::STV_DEFAULT,
                "symbol is default vis"
            );
            ensure!(sect.is_executable(), "executable");
            ensure!(!sect.is_writable(), "immutable");
            ensure!(sect.sh_addralign == 16, "aligned to 16");
            Ok(())
        }),
        DeclTestCase::new(
            "custom_align_func",
            Decl::function().with_align(Some(64)),
            |_sym, sect| {
                ensure!(
                    sect.sh_addralign == 64,
                    "expected aligned to 64, got {}",
                    sect.sh_addralign
                );
                Ok(())
            },
        ),
        DeclTestCase::new(
            "custom_align_data",
            Decl::data().with_align(Some(128)),
            |_sym, sect| {
                ensure!(
                    sect.sh_addralign == 128,
                    "expected aligned to 128, got {}",
                    sect.sh_addralign
                );
                Ok(())
            },
        ),
        DeclTestCase::new(
            "executable_data",
            Decl::data().with_executable(true),
            |_sym, sect| {
                ensure!(sect.is_executable(), "executable");
                Ok(())
            },
        ),
        DeclTestCase::new(
            "mutable_function",
            Decl::function().writable(),
            |_sym, sect| {
                ensure!(sect.is_writable(), "writable");
                Ok(())
            },
        ),
    ]);
}

#[test]
// Can't test with DeclTestCase, as section declarations don't generate symbols
fn section_permissions() {
    let mut obj = Artifact::new(triple!("x86_64-unknown-unknown-unknown-elf"), "a".into());
    obj.declare(
        "test",
        Decl::section(faerie::SectionKind::Text)
            .with_loaded(true)
            .with_writable(true)
            .with_executable(true),
    )
    .expect("Can declare section with permissions");
    obj.define("test", vec![1, 2, 3, 4])
        .expect("Can define section");

    let bytes = obj.emit().expect("can emit elf file");
    if let goblin::Object::Elf(elf) = goblin::Object::parse(&bytes).expect("can parse elf file") {
        let sect = elf
            .section_headers
            .iter()
            .find(|section| &elf.shdr_strtab[section.sh_name] == "test");

        if let Some(section) = sect {
            assert!(section.is_alloc());
            assert!(section.is_writable());
            assert!(section.is_executable());
        } else {
            panic!("Could not find test section")
        }
    } else {
        panic!("Elf file not parsed as elf file");
    }
}

/* test scaffolding: */

fn decl_tests(tests: Vec<DeclTestCase>) {
    let mut obj = Artifact::new(triple!("x86_64-unknown-unknown-unknown-elf"), "a".into());
    for t in tests.iter() {
        t.define(&mut obj);
    }

    println!("\n{:#?}", obj);
    let bytes = obj.emit().expect("can emit elf file");
    let bytes = bytes.as_slice();
    println!("{:?}", bytes);

    let elf = goblin::Object::parse(&bytes).expect("can parse elf file");

    match elf {
        goblin::Object::Elf(elf) => {
            for t in tests {
                t.check(&elf)
            }
        }
        _ => {
            panic!("Elf file not parsed as elf file");
        }
    }
}

struct DeclTestCase {
    name: String,
    decl: Decl,
    pred: Box<dyn Fn(&Sym, &SectionHeader) -> Result<(), Error>>,
}
impl DeclTestCase {
    fn new<D, F>(name: &str, decl: D, pred: F) -> Self
    where
        D: Into<Decl>,
        F: Fn(&Sym, &SectionHeader) -> Result<(), Error> + 'static,
    {
        Self {
            name: name.to_owned(),
            decl: decl.into(),
            pred: Box::new(pred),
        }
    }
    fn define(&self, art: &mut Artifact) {
        art.declare(&self.name, self.decl)
            .expect(&format!("declare {}", self.name));
        art.define(&self.name, vec![1, 2, 3, 4])
            .expect(&format!("define {}", self.name));
    }
    fn check(&self, elf: &goblin::elf::Elf) {
        let sym = elf
            .syms
            .iter()
            .find(|sym| &elf.strtab[sym.st_name] == self.name)
            .expect("symbol should exist");
        let sectheader = elf
            .section_headers
            .get(sym.st_shndx)
            .expect("section header should exist");
        (self.pred)(&sym, sectheader).expect(&format!("check {}", self.name))
    }
}

#[test]
fn extended_symtab_issue_76() {
    let name = "extended_symtab.o";
    let mut obj = ArtifactBuilder::new(triple!("x86_64-unknown-unknown-unknown-elf"))
        .name(name.to_string())
        .finish();
    for i in 0..0x10000 {
        let n = format!("func{}", i);
        let decl: Decl = Decl::function().global().into();
        obj.declare_with(n, decl, vec![0xcc])
            .expect("can declare a function");
    }
    let bytes = obj
        .emit()
        .expect("can emit elf object file with 0x10000 functions");
    let elf = goblin::Object::parse(&bytes).expect("can parse elf file");
    match elf {
        goblin::Object::Elf(elf) => {
            assert_eq!(elf.header.e_shnum, 0);
            assert_eq!(elf.section_headers.len(), 65541);
            assert_eq!(elf.shdr_relocs.len(), 0);
            assert_eq!(elf.syms.len(), 131074);
        }
        _ => {
            panic!("Elf file not parsed as elf file");
        }
    }
}
