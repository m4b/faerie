extern crate faerie;
extern crate env_logger;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;
extern crate failure;

use structopt::StructOpt;
use failure::Error;

use faerie::{Link, Elf, Mach, Target, ArtifactBuilder, SymbolType};
use std::path::Path;
use std::fs::File;
use std::env;
use std::process::Command;

// ELF linking
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crt1.o food.o -lc -o food

// ELF try this for dynamically linked file
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crti.o /usr/lib/Scrt1.o /usr/lib/crtn.o test.o -lc -o test

#[derive(StructOpt, Debug, Clone)]
#[structopt(name = "prototype", about = "This is prototype binary for emitting object files;
 it is only meant for debugging, a reference, etc. - Knock yourself out")]
pub struct Args {
    #[structopt(long = "deadbeef", help = "Generate deadbeef object file to link against main program")]
    deadbeef: bool,

    #[structopt(short = "l", long = "link", help = "Link the file with this name")]
    link: Option<String>,

    #[structopt(short = "d", long = "debug", help = "Enable debug")]
    debug: bool,

    #[structopt(long = "mach", help = "Output mach file")]
    mach: bool,

    #[structopt(long = "library", help = "Output a static library (Unimplemented)")]
    library: bool,

    #[structopt(help = "The filename to output")]
    filename: String,

    #[structopt(help = "Additional files to link")]
    linkline: Vec<String>
}

fn run (args: Args) -> Result<(), Error> {
    let file = File::create(Path::new(&args.filename))?;
    let mut obj = ArtifactBuilder::new(Target::X86_64)
        .name(args.filename.clone())
        .library(args.library)
        .finish();

    // first we declare our symbolic references;
    // it is a runtime error to define a symbol _without_ declaring it first
    obj.declare("deadbeef", SymbolType::Function { local: true });
    obj.declare("main", SymbolType::Function { local: false });
    obj.declare("str.1", SymbolType::Data { local: true });
    obj.declare("DEADBEEF", SymbolType::DataImport);
    obj.declare("printf", SymbolType::FunctionImport);

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
    obj.link2(Link { from: "main", to: "str.1", at: 19 })?;
    obj.link2(Link { from: "main", to: "printf", at: 29 })?;
    obj.link2(Link { from: "main", to: "deadbeef", at: 10 })?;
    obj.link2(Link { from: "deadbeef", to: "DEADBEEF", at: 7 })?;

    // Finally, we write which object file we desire
    if args.mach {
        obj.write::<Mach>(file)?;
    } else {
        obj.write::<Elf>(file)?;
        println!("res: {:#?}", obj);
    }
    if let Some(output) = args.link {
        link(&args.filename, &output, &args.linkline)?;
    }
    Ok(())
}

fn deadbeef (args: Args) -> Result<(), Error> {
    let file = File::create(Path::new(&args.filename))?;
    let mut obj = ArtifactBuilder::new(Target::X86_64)
        .name(args.filename.clone())
        .library(args.library)
        .finish();

    // FIXME: need to state this isn't a string, but some linkers don't seem to care \o/
    // gold complains though:
    // ld.gold: warning: deadbeef.o: last entry in mergeable string section '.data.DEADBEEF' not null terminated
    obj.declare("DEADBEEF", SymbolType::Data { local: false });
    obj.define("DEADBEEF", [0xef, 0xbe, 0xad, 0xde].to_vec())?;
    if args.mach {
        obj.write::<Mach>(file)?;
    } else {
        obj.write::<Elf>(file)?;
        println!("res: {:#?}", obj);
    }
    if let Some(output) = args.link {
        link(&args.filename, &output, &args.linkline)?;
    }
    Ok(())
}

fn link(name: &str, output: &str, linkline: &[String]) -> Result<(), Error> {
    //ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crti.o /usr/lib/Scrt1.o /usr/lib/crtn.o test.o -lc -o test
    let child = Command::new("cc")
                        .args(linkline)
                        .args(&[name,
                                "-o", output,
                            ])
                        .spawn()?;
    let child = child.wait_with_output()?;
    println!("{}", ::std::str::from_utf8(child.stdout.as_slice()).unwrap());
    Ok(())
}

fn main () {
    let args = Args::from_args();
    if args.debug { ::env::set_var("RUST_LOG", "faerie=debug"); };
    env_logger::init().unwrap();
    let res = if args.deadbeef { deadbeef(args) } else { run(args) };
    match res {
        Ok(()) => (),
        Err(err) => println!("{:#}", err)
    }
}
