#![cfg_attr(rustfmt, rustfmt_skip)]

use faerie::{ArtifactBuilder, ArtifactError, triple, Link, Decl};
use std::path::Path;
use std::fs::File;

pub fn main() -> Result<(), ArtifactError> {
    let name = "test.o";
    let file = File::create(Path::new(name))?;
    let mut obj = ArtifactBuilder::new(triple!("x86_64-unknown-unknown-unknown-elf"))
        .name(name.to_owned())
        .finish();

    // first we declare our symbolic references;
    // it is a runtime error to define a symbol _without_ declaring it first
    obj.declarations(
        [
            ("deadbeef", Decl::function().into()),
            ("main",     Decl::function().global().into()),
            ("str.1",    Decl::cstring().into()),
            ("DEADBEEF", Decl::data_import().into()),
            ("printf",   Decl::function_import().into()),
        ].iter().cloned()
    )?;

    // we now define our local functions and data
    // 0000000000000000 <deadbeef>:
    //    0:	55                   	push   %rbp
    //    1:	48 89 e5             	mov    %rsp,%rbp
    //    4:	48 8b 05 00 00 00 00 	mov    0x0(%rip),%rax        # b <deadbeef+0xb>
    // 			7: R_X86_64_GOTPCREL	DEADBEEF-0x4
    //    b:	8b 08                	mov    (%rax),%ecx
    //    d:	83 c1 01             	add    $0x1,%ecx
    //   10:	89 c8                	mov    %ecx,%eax
    //   12:	5d                   	pop    %rbp
    //   13:	c3                   	retq
    obj.define("deadbeef",
        vec![0x55,
            0x48, 0x89, 0xe5,
            0x48, 0x8b, 0x05, 0x00, 0x00, 0x00, 0x00,
            0x8b, 0x08,
            0x83, 0xc1, 0x01,
            0x89, 0xc8,
            0x5d,
            0xc3])?;
    // main:
    // 55	push   %rbp
    // 48 89 e5	mov    %rsp,%rbp
    // b8 00 00 00 00	mov    $0x0,%eax
    // e8 00 00 00 00   callq  0x0 <deadbeef>
    // 89 c6	mov    %eax,%esi
    // 48 8d 3d 00 00 00 00 lea    0x0(%rip),%rdi # will be: deadbeef: 0x%x\n
    // b8 00 00 00 00	mov    $0x0,%eax
    // e8 00 00 00 00	callq  0x3f <main+33>  # printf
    // b8 00 00 00 00	mov    $0x0,%eax
    // 5d	pop    %rbp
    // c3	retq
    obj.define("main",
        vec![0x55,
            0x48, 0x89, 0xe5,
            0xb8, 0x00, 0x00, 0x00, 0x00,
            0xe8, 0x00, 0x00, 0x00, 0x00,
            0x89, 0xc6,
            0x48, 0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00,
            0xb8, 0x00, 0x00, 0x00, 0x00,
            0xe8, 0x00, 0x00, 0x00, 0x00,
            0xb8, 0x00, 0x00, 0x00, 0x00,
            0x5d,
            0xc3])?;
    obj.define("str.1", b"deadbeef: 0x%x\n\0".to_vec())?;

    // Next, we declare our relocations,
    // which are _always_ relative to the `from` symbol
    obj.link(Link { from: "main", to: "str.1", at: 19 })?;
    obj.link(Link { from: "main", to: "printf", at: 29 })?;
    obj.link(Link { from: "main", to: "deadbeef", at: 10 })?;
    obj.link(Link { from: "deadbeef", to: "DEADBEEF", at: 7 })?;

    // Finally, we write the object file
    obj.write(file)?;

    Ok(())
}