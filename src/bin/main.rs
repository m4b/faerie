extern crate faerie;
extern crate env_logger;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;
extern crate failure;
extern crate target_lexicon;

use structopt::StructOpt;
use failure::Error;
use target_lexicon::{Architecture, Vendor, OperatingSystem, Environment, BinaryFormat, Triple};

use faerie::{Link, ArtifactBuilder, Decl};
use std::path::Path;
use std::fs::File;
use std::env;
use std::process::Command;

// ELF linking
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crt1.o food.o -lc -o food

// ELF try this for dynamically linked file
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crti.o /usr/lib/Scrt1.o /usr/lib/crtn.o test.o -lc -o test

// example to run
// ./prototype --deadbeef deadbeef.o
// ./prototype --link test test.o deadbeef.o
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
    let target = Triple {
        architecture: Architecture::X86_64,
        vendor: Vendor::Unknown,
        operating_system: OperatingSystem::Unknown,
        environment: Environment::Unknown,
        binary_format: if args.mach {
            BinaryFormat::Macho
        } else {
            BinaryFormat::Elf
        },
    };
    let mut obj = ArtifactBuilder::new(target)
        .name(args.filename.clone())
        .library(args.library)
        .finish();

    // first we declare our symbolic references;
    // it is a runtime error to define a symbol _without_ declaring it first
    obj.declarations(
        [
            ("deadbeef",   Decl::Function { global: false }),
            ("main",       Decl::Function { global: true }),
            ("str.1",      Decl::CString { global: false }),
            ("DEADBEEF",   Decl::DataImport),
            ("STATIC",     Decl::Data { global: true, writable: true }),
            ("STATIC_REF", Decl::Data { global: true, writable: true }),
            ("printf",     Decl::FunctionImport),
        ].into_iter().cloned()
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
    // 48 83 ec 10	sub    $0x10,%rsp
    // c7 45 fc 00 00 00 00	movl   $0x0,-0x4(%rbp)
    // b8 00 00 00 00	mov    $0x0,%eax
    // e8 00 00 00 00	callq  0x16 <deadbeef>
    // 48 8d 3d 00 00 00 00	lea    0x0(%rip),%rdi        # 0x1d <main+29> will be: "deadbeef: 0x%x - %d\n"
    // 48 8b 0d 00 00 00 00	mov    0x0(%rip),%rcx        # 0x24 <main+36>
    // 8b 11	mov    (%rcx),%edx
    // 89 c6	mov    %eax,%esi
    // b0 00	mov    $0x0,%al
    // e8 00 00 00 00	callq  0x2f <main+47> # printf
    // 31 d2	xor    %edx,%edx
    // 89 45 f8	mov    %eax,-0x8(%rbp)
    // 89 d0	mov    %edx,%eax
    // 48 83 c4 10	add    $0x10,%rsp
    // 5d	pop    %rbp
    // c3	retq
    obj.define("main",
        vec![
             0x55,
             0x48, 0x89, 0xe5,
             0x48, 0x83, 0xec, 0x10,
             0xc7, 0x45, 0xfc, 0x00, 0x00, 0x00, 0x00,
             0xb8, 0x00, 0x00, 0x00, 0x00,
             0xe8, 0x00, 0x00, 0x00, 0x00,
             0x48, 0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00,
             0x48, 0x8b, 0x0d, 0x00, 0x00, 0x00, 0x00,
             0x8b, 0x11,
             0x89, 0xc6,
             0xb0, 0x00,
             0xe8, 0x00, 0x00, 0x00, 0x00,
             0x31, 0xd2,
             0x89, 0x45, 0xf8,
             0x89, 0xd0,
             0x48, 0x83, 0xc4, 0x10,
             0x5d,
             0xc3,
        ])?;
    // define static data
    obj.define("str.1", b"deadbeef: 0x%x - 0x%x\n\0".to_vec())?;
    obj.define("STATIC",     [0xbe, 0xba, 0xfe, 0xca].to_vec())?;
    // .data static references need to be zero'd out explicitly for now.
    obj.define("STATIC_REF", vec![0; 8])?;

    // Next, we declare our relocations,
    // which are _always_ relative to the `from` symbol
    // -- main relocations --
    obj.link(Link { from: "main", to: "deadbeef", at: 0x15 })?;
    obj.link(Link { from: "main", to: "str.1", at: 0x1c })?;
    obj.link(Link { from: "main", to: "STATIC_REF", at: 0x23 })?;
    obj.link(Link { from: "main", to: "printf", at: 0x2e })?;

    // -- deadbeef relocations --
    obj.link(Link { from: "deadbeef", to: "DEADBEEF", at: 0x7 })?;

    // -- static data relocations --
    // this is a reference to an object in the data section, so we are always at relative offset 0
    obj.link(Link { from: "STATIC_REF", to: "STATIC", at: 0 })?;

    // Finally, we emit the object file
    obj.write(file)?;
    if let Some(output) = args.link {
        link(&args.filename, &output, &args.linkline)?;
    }
    Ok(())
}

fn deadbeef (args: Args) -> Result<(), Error> {
    let file = File::create(Path::new(&args.filename))?;
    let target = Triple {
        architecture: Architecture::X86_64,
        vendor: Vendor::Unknown,
        operating_system: OperatingSystem::Unknown,
        environment: Environment::Unknown,
        binary_format: if args.mach {
            BinaryFormat::Macho
        } else {
            BinaryFormat::Elf
        },
    };
    let mut obj = ArtifactBuilder::new(target)
        .name(args.filename.clone())
        .library(args.library)
        .finish();

    // FIXME: need to state this isn't a string, but some linkers don't seem to care \o/
    // gold complains though:
    // ld.gold: warning: deadbeef.o: last entry in mergeable string section '.data.DEADBEEF' not null terminated
    obj.declare("DEADBEEF", Decl::Data { global: true, writable: false })?;
    obj.define("DEADBEEF", [0xef, 0xbe, 0xad, 0xde].to_vec())?;
    obj.write(file)?;
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
    env_logger::init();
    let res = if args.deadbeef { deadbeef(args) } else { run(args) };
    match res {
        Ok(()) => (),
        Err(err) => println!("{:#}", err)
    }
}
