extern crate faerie;
extern crate env_logger;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;

use structopt::StructOpt;

use faerie::{error, Elf, Mach, Target, ArtifactBuilder};
use std::path::Path;
use std::fs::File;
use std::env;

// ELF linking
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crt1.o food.o -lc -o food

// ELF try this for dynamically linked file
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crti.o /usr/lib/Scrt1.o /usr/lib/crtn.o test.o -lc -o test

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "prototype", about = "This is prototype binary for emitting object files; it is only meant for debugging, a reference, etc. - Knock yourself out")]
pub struct Args {
    #[structopt(short = "d", long = "debug", help = "Enable debug")]
    debug: bool,

    #[structopt(long = "mach", help = "Output mach file")]
    mach: bool,

    #[structopt(long = "library", help = "Output a static library")]
    library: bool,

    #[structopt(help = "The filename to output")]
    filename: String,
}

fn run (args: Args) -> error::Result<()> {
    let file = File::create(Path::new(&args.filename))?;
    let mut obj = ArtifactBuilder::new(Target::X86_64).name(args.filename).library(args.library).finish();
    // 55	push   %rbp
    // 48 89 e5	mov    %rsp,%rbp
    // b8 ef be ad de	mov    $0xdeadbeef,%eax
    // 5d	pop    %rbp
    // c3	retq
    obj.add_code("deadbeef",
        vec![0x55,
             0x48, 0x89, 0xe5,
             0xb8, 0xef, 0xbe, 0xad, 0xde,
             0x5d,
             0xc3]);
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
    obj.add_code("main",
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
             0xc3]);
    obj.add_data("str.1", b"deadbeef: 0x%x\n\0".to_vec());
    // relocations are relative to the symbol
    obj.link("main", "str.1", 19);
    obj.import("printf");
    obj.link_import("main", "printf", 29);
    obj.link_import("main", "deadbeef", 10);
    if args.mach {
        obj.write::<Mach>(file)?;
    } else {
        obj.write::<Elf>(file)?;
        println!("res: {:#?}", obj);
    }
    Ok(())
}

fn main () {
    let args = Args::from_args();
    if args.debug { ::env::set_var("RUST_LOG", "faerie=debug"); };
    env_logger::init().unwrap();
    match run(args) {
        Ok(()) => (),
        Err(err) => println!("{:#}", err)
    }
}
