extern crate faerie;
extern crate goblin;
extern crate scroll;
#[macro_use]
extern crate target_lexicon;

use std::str::FromStr;

use faerie::{Artifact, Decl, Link};
use goblin::elf::*;

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
