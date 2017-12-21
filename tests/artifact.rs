extern crate faerie;

use faerie::*;

#[test]
fn duplicate_declarations_are_ok() {

    let mut obj = Artifact::new(Target::X86_64, "t.o".into());

    obj.declare("str.0", faerie::Decl::DataImport {}).expect(
        "initial declaration",
    );

    obj.declare(
        "str.0",
        faerie::Decl::Data {
            global: false,
            writeable: false,
        },
    ).expect("declare should be compatible");

    obj.define("str.0", b"hello world\0".to_vec())
       .expect("define");

    let mut obj = Artifact::new(Target::X86_64, "t.o".into());
    obj.declarations(vec![
        ("str.0", faerie::Decl::DataImport),
        ("str.0", faerie::Decl::Data {
            global: true,
            writeable: false
            }),
        ("str.0", faerie::Decl::DataImport),
        ("str.0", faerie::Decl::DataImport),
        ("str.0", faerie::Decl::Data {
            global: true,
            writeable: false

        }),

        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::Function { global: true }),
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::Function { global: true }),
    ].into_iter()
    ).expect("multiple declarations are ok");
}

#[test]
fn multiple_different_declarations_are_not_ok() {

    let mut obj = Artifact::new(Target::X86_64, "t.o".into());

    obj.declare("f", faerie::Decl::FunctionImport {}).expect(
        "initial declaration",
    );

    assert!(obj.declare(
        "f",
        faerie::Decl::Data {
            global: false,
            writeable: false,
        },
    ).is_err());
}

#[test]
#[should_panic]
fn multiple_different_conflicting_declarations_are_not_ok_and_do_not_overwrite() {
    let mut obj = Artifact::new(Target::X86_64, "t.o".into());
    obj.declarations(vec![
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::Function { global: true }),
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::Function { global: false }),
    ].into_iter()
    ).expect("multiple conflicting declarations are not ok");
}

#[test]
fn import_declarations_fill_imports_correctly() {
    let mut obj = Artifact::new(Target::X86_64, "t.o".into());
    obj.declarations(vec![
        ("f", faerie::Decl::FunctionImport),
        ("f", faerie::Decl::FunctionImport),
        ("d", faerie::Decl::DataImport),
    ].into_iter()
    ).expect("can declare");
    let imports = obj.imports().collect::<Vec<_>>();
    assert_eq!(imports.len(), 2);
}

#[test]
fn import_declarations_work_with_redeclarations() {
    let mut obj = Artifact::new(Target::X86_64, "t.o".into());
    obj.declarations(vec![
        ("f", faerie::Decl::FunctionImport),
        ("d", faerie::Decl::DataImport),
        ("d", faerie::Decl::DataImport),
        ("f", faerie::Decl::Function { global: true }),
        ("f", faerie::Decl::FunctionImport),
    ].into_iter()
    ).expect("can declare");
    let imports = obj.imports().collect::<Vec<_>>();
    assert_eq!(imports.len(), 1);
}
