extern crate faerie;
#[macro_use]
extern crate target_lexicon;
#[cfg(test)]
extern crate goblin;

use faerie::*;
use std::str::FromStr;

#[test]
fn duplicate_declarations_are_ok() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());

    obj.declare("str.0", faerie::Decl::data_import())
        .expect("initial declaration");

    obj.declare("str.0", faerie::Decl::data().local().read_only())
        .expect("declare should be compatible");

    obj.define("str.0", b"hello world\0".to_vec())
        .expect("define");

    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    let decls = vec![
        ("str.0", faerie::Decl::data_import().into()),
        ("str.0", faerie::Decl::data().global().read_only().into()),
        ("str.0", faerie::Decl::data_import().into()),
        ("str.0", faerie::Decl::data_import().into()),
        ("str.0", faerie::Decl::data().global().read_only().into()),
        ("f", faerie::Decl::function_import().into()),
        ("f", faerie::Decl::function().global().into()),
        ("f", faerie::Decl::function_import().into()),
        ("f", faerie::Decl::function_import().into()),
        ("f", faerie::Decl::function().global().into()),
    ];
    obj.declarations(decls.into_iter())
        .expect("multiple declarations are ok");
}

#[test]
fn multiple_different_declarations_are_not_ok() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());

    obj.declare("f", faerie::Decl::function_import())
        .expect("initial declaration");

    assert!(obj.declare("f", faerie::Decl::data(),).is_err());
}

#[test]
fn multiple_different_conflicting_declarations_are_not_ok_and_do_not_overwrite() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    assert!(obj
        .declarations(
            vec![
                ("f", faerie::Decl::function_import().into()),
                ("f", faerie::Decl::function().global().into()),
                ("f", faerie::Decl::function_import().into()),
                ("f", faerie::Decl::function_import().into()),
                ("f", faerie::Decl::function().into()),
            ]
            .into_iter(),
        )
        .is_err()); // multiple conflicting declarations are not ok
}

#[test]
fn import_declarations_fill_imports_correctly() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    obj.declarations(
        vec![
            ("f", faerie::Decl::function_import().into()),
            ("f", faerie::Decl::function_import().into()),
            ("d", faerie::Decl::data_import().into()),
        ]
        .into_iter(),
    )
    .expect("can declare");
    let imports = obj.imports().collect::<Vec<_>>();
    assert_eq!(imports.len(), 2);
}

#[test]
fn import_declarations_work_with_redeclarations() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    obj.declarations(
        vec![
            ("f", faerie::Decl::function_import().into()),
            ("d", faerie::Decl::data_import().into()),
            ("d", faerie::Decl::data_import().into()),
            ("f", faerie::Decl::function().global().into()),
            ("f", faerie::Decl::function_import().into()),
        ]
        .into_iter(),
    )
    .expect("can declare");
    let imports = obj.imports().collect::<Vec<_>>();
    assert_eq!(imports.len(), 1);
}

#[test]
fn import_helper_adds_declaration_only_once() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    obj.import("f", faerie::ImportKind::Function)
        .expect("can import");
    let imports = obj.imports().collect::<Vec<_>>();
    assert_eq!(imports.len(), 1);
}

#[test]
fn reject_duplicate_definitions() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    obj.declarations(
        vec![
            ("f", faerie::Decl::function().global().into()),
            ("g", faerie::Decl::function().into()),
        ]
        .into_iter(),
    )
    .expect("can declare");

    obj.define("g", vec![1, 2, 3, 4]).expect("can define");
    // Reject duplicate definition:
    assert!(obj.define("g", vec![1, 2, 3, 4]).is_err());

    obj.define("f", vec![4, 3, 2, 1]).expect("can define");
    // Reject duplicate definitions:
    assert!(obj.define("g", vec![1, 2, 3, 4]).is_err());
    assert!(obj.define("f", vec![1, 2, 3, 4]).is_err());
}

#[test]
fn undefined_symbols() {
    let mut obj = Artifact::new(triple!("x86_64"), "t.o".into());
    obj.declarations(
        vec![
            ("f", faerie::Decl::function().global().into()),
            ("g", faerie::Decl::function().into()),
        ]
        .into_iter(),
    )
    .expect("can declare");
    assert_eq!(
        obj.undefined_symbols(),
        vec![String::from("f"), String::from("g")]
    );

    obj.define("g", vec![1, 2, 3, 4]).expect("can define");
    assert_eq!(obj.undefined_symbols(), vec![String::from("f")]);

    obj.define("f", vec![4, 3, 2, 1]).expect("can define");
    assert!(obj.undefined_symbols().is_empty());
}

#[test]
fn vary_output_formats() {
    use goblin::Object;
    use target_lexicon::BinaryFormat;

    let obj = Artifact::new(triple!("x86_64"), "t.o".into());
    assert!(obj.emit().is_err());

    let elf = obj.emit_as(BinaryFormat::Elf).unwrap();
    match Object::parse(&elf).unwrap() {
        Object::Elf(_) => {}
        _ => panic!("emitted as ELF but didn't parse as ELF"),
    }

    let mach = obj.emit_as(BinaryFormat::Macho).unwrap();
    match Object::parse(&mach).unwrap() {
        Object::Mach(_) => {}
        _ => panic!("emitted as MachO but didn't parse as MachO"),
    }

    /* TODO: Enable when COFF is supported.
    let coff = obj.emit_as(BinaryFormat::Coff).unwrap();
    match Object::parse(&coff).unwrap() {
         Object::PE(_) => {}
         _ => panic!("emitted as COFF but didn't parse as COFF"),
    }
    */
}

#[test]
fn bss() {
    use goblin::{mach::Mach, Object};
    use std::io::Write;
    use target_lexicon::BinaryFormat;

    const SIZE: usize = 100_000_000_000_000;

    let mut artifact = Artifact::new(triple!("x86_64"), "bss".into());
    artifact.declare("my_data", Decl::data().global()).unwrap();
    artifact.define_zero_init("my_data", SIZE).unwrap();

    let elf = artifact.emit_as(BinaryFormat::Elf).unwrap();
    assert!(elf.len() < SIZE);
    match Object::parse(&elf).unwrap() {
        Object::Elf(elf) => assert!(!elf.syms.is_empty()),
        _ => panic!("emitted as ELF but did not parse as ELF"),
    }

    let mach = artifact.emit_as(BinaryFormat::Macho).unwrap();
    let mut file = std::fs::File::create("mach.o").unwrap();
    file.write_all(&mach).unwrap();
    assert!(mach.len() < SIZE);
    match Object::parse(&mach).unwrap() {
        Object::Mach(Mach::Binary(mach)) => {
            assert!(mach
                .segments
                .iter()
                .any(|segment| segment.vmsize == SIZE as u64));
        }
        _ => panic!("emitted as MACHO but did not parse as MACHO"),
    }
}

#[test]
fn invalid_bss() {
    let mut artifact = Artifact::new(triple!("x86_64"), "bss".into());
    artifact.declare("my_func", Decl::function()).unwrap();
    assert!(artifact.define_zero_init("my_func", 100).is_err());
    artifact
        .declare("my_section", Decl::section(SectionKind::Data))
        .unwrap();
    assert!(artifact.define_zero_init("my_section", 100).is_err());
}
