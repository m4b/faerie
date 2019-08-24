extern crate env_logger;
extern crate faerie;
extern crate failure;
extern crate goblin;
extern crate structopt;
extern crate target_lexicon;

use failure::Error;
use structopt::StructOpt;
use target_lexicon::{Architecture, BinaryFormat, Environment, OperatingSystem, Triple, Vendor};

use faerie::{ArtifactBuilder, Decl, Link, Reloc, SectionKind};
use std::env;
use std::fs::File;
use std::path::Path;
use std::process::Command;

// ELF linking
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crt1.o food.o -lc -o food

// ELF try this for dynamically linked file
// ld -e _start -I/usr/lib/ld-linux-x86-64.so.2 -L/usr/lib/ /usr/lib/crti.o /usr/lib/Scrt1.o /usr/lib/crtn.o test.o -lc -o test

// example to run
// ./prototype --deadbeef deadbeef.o
// ./prototype --link test test.o deadbeef.o
#[derive(StructOpt, Debug, Clone)]
#[structopt(
    name = "prototype",
    about = "This is prototype binary for emitting object files;
 it is only meant for debugging, a reference, etc. - Knock yourself out"
)]
pub struct Args {
    #[structopt(
        long = "deadbeef",
        help = "Generate deadbeef object file to link against main program"
    )]
    deadbeef: bool,

    #[structopt(short = "l", long = "link", help = "Link the file with this name")]
    link: Option<String>,

    #[structopt(short = "d", long = "debug", help = "Enable debug")]
    debug: bool,

    #[structopt(long = "mach", help = "Output mach file")]
    mach: bool,

    #[structopt(long = "library", help = "Output a static library (Unimplemented)")]
    library: bool,

    #[structopt(long = "dwarf", help = "Emit some DWARF sections")]
    dwarf: bool,

    #[structopt(help = "The filename to output")]
    filename: String,

    #[structopt(help = "Additional files to link")]
    linkline: Vec<String>,
}

#[rustfmt::skip]
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
    let declarations: Vec<(&'static str, Decl)> = vec![
        ("deadbeef", Decl::function().into()),
        ("main", Decl::function().global().into()),
        ("str.1", Decl::cstring().into()),
        ("DEADBEEF", Decl::data_import().into()),
        ("STATIC", Decl::data().global().writable().into()),
        ("STATIC_REF", Decl::data().global().writable().with_align(Some(64)).into()),
        ("printf", Decl::function_import().into()),
    ];
    obj.declarations(declarations.into_iter())?;

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

    // define a custom section
    obj.declare(".faerie", Decl::section(SectionKind::Data))?;
    obj.define(".faerie", b"some data".to_vec())?;

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

#[rustfmt::skip]
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
    obj.declare("DEADBEEF", Decl::data().global().read_only())?;
    obj.define("DEADBEEF", [0xef, 0xbe, 0xad, 0xde].to_vec())?;

    if args.dwarf {
        // DWARF sections
        obj.declare(".debug_abbrev", Decl::section(SectionKind::Debug))?;
        obj.declare(".debug_info", Decl::section(SectionKind::Debug))?;
        obj.declare(".debug_str", Decl::section(SectionKind::Debug))?;

        obj.define(".debug_str",
            concat![
                // 0x00:
                "faerie\0",
                // 0x07:
                "/faerie/reference\0",
                // 0x19:
                "deadbeef.c\0",
                // 0x24:
                "DEADBEEF\0",
            ].as_bytes().to_vec())?;
        obj.define(".debug_abbrev",
            vec![
                // Abbrev 1: DW_TAG_compile_unit, DW_CHILDREN_yes
                0x01, 0x11, 0x01,
                // DW_AT_producer, DW_FORM_strp
                0x25, 0x0e,
                // DW_AT_language, DW_FORM_data1
                0x13, 0x0b,
                // DW_AT_name, DW_FORM_strp
                0x03, 0x0e,
                // DW_AT_comp_dir, DW_FORM_strp
                0x1b, 0x0e,
                // null
                0x00, 0x00,

                // Abbrev 2: DW_TAG_variable, DW_CHILDREN_no
                0x02, 0x34, 0x00,
                // DW_AT_name, DW_FORM_strp
                0x03, 0x0e,
                // DW_AT_type, DW_FORM_ref4
                0x49, 0x13,
                // DW_AT_external, DW_FORM_flag_present
                0x3f, 0x19,
                // DW_AT_location, DW_FORM_exprloc
                0x02, 0x18,
                // null
                0x00, 0x00,

                // Abbrev 3: DW_TAG_base_type, DW_CHILDREN_no
                0x03, 0x24, 0x00,
                // DW_AT_name, DW_FORM_string
                0x03, 0x08,
                // DW_AT_byte_size, DW_FORM_data1
                0x0b, 0x0b,
                // DW_AT_encoding, DW_FORM_data1
                0x3e, 0x0b,
                // null
                0x00, 0x00,

                // null
                0x00,
            ])?;
        let mut debug_info =
            vec![
                // 0x00: Length = 0x34 - 4
                0x30, 0x00, 0x00, 0x00,
                // 0x04: Version
                0x04, 0x00,
                // 0x06: Abbrev offset (needs reloc)
                0x00, 0x00, 0x00, 0x00,
                // 0x0a: Address size
                0x08,

                // 0x0b: Abbrev 1 = DW_TAG_compile_unit
                0x01,
                // 0x0c: DW_AT_producer = 0x00 (needs reloc)
                0x00, 0x00, 0x00, 0x00,
                // 0x10: DW_AT_language = DW_LANG_C
                0x02,
                // 0x11: DW_AT_name = 0x19 (needs reloc)
                0x00, 0x00, 0x00, 0x00,
                // 0x15: DW_AT_comp_dir = 0x07 (needs reloc)
                0x00, 0x00, 0x00, 0x00,

                // 0x19: Abbrev 2 = DW_TAG_variable
                0x02,
                // 0x1a: DW_AT_name = 0x24 (needs reloc)
                0x00, 0x00, 0x00, 0x00,
                // 0x1e: DW_AT_type = offset of int base_type
                0x2c, 0x00, 0x00, 0x00,
                // 0x22: DW_FORM_flag_present = no data needed
                // 0x22: DW_AT_location = len 9, DW_OP_addr DEADBEEF (needs reloc)
                0x09, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,

                // 0x2c: Abbrev 3 = DW_TAG_base_type
                0x03,
                // 0x2d: DW_AT_name = "int"
                b'i', b'n', b't', 0x00,
                // 0x31: DW_AT_byte_size = 4
                0x04,
                // 0x32: DW_AT_encoding = DW_ATE_signed
                0x05,

                // 0x33: End of children
                0x00,
            ];

        if args.mach {
            // No relocation needed for Mach.
            debug_info[0x11] = 0x19;
            debug_info[0x15] = 0x7;
            debug_info[0x1a] = 0x24;
        } else {
            // abbrev offset
            obj.link_with(
                Link { from: ".debug_info", to: ".debug_abbrev", at: 0x06},
                Reloc::Debug { size: 4, addend: 0x0},
            )?;
            // producer
            obj.link_with(
                Link { from: ".debug_info", to: ".debug_str", at: 0x0c},
                Reloc::Debug { size: 4, addend: 0x0},
            )?;
            // CU name
            obj.link_with(
                Link { from: ".debug_info", to: ".debug_str", at: 0x11},
                Reloc::Debug { size: 4, addend: 0x19},
            )?;
            // comp dir
            obj.link_with(
                Link { from: ".debug_info", to: ".debug_str", at: 0x15},
                Reloc::Debug { size: 4, addend: 0x7},
            )?;
            // var name
            obj.link_with(
                Link { from: ".debug_info", to: ".debug_str", at: 0x1a},
                Reloc::Debug { size: 4, addend: 0x24},
            )?;
        }
        // var location
        obj.link_with(
            Link { from: ".debug_info", to: "DEADBEEF", at: 0x24},
            Reloc::Debug { size: 8, addend: 0x0},
        )?;

        obj.define(".debug_info", debug_info)?;
    }

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
        .args(&[name, "-o", output])
        .spawn()?;
    let child = child.wait_with_output()?;
    println!(
        "{}",
        ::std::str::from_utf8(child.stdout.as_slice()).unwrap()
    );
    Ok(())
}

fn main() {
    let args = Args::from_args();
    if args.debug {
        env::set_var("RUST_LOG", "faerie=debug");
    };
    env_logger::init();
    let res = if args.deadbeef {
        deadbeef(args)
    } else {
        run(args)
    };
    match res {
        Ok(()) => (),
        Err(err) => println!("{:#}", err),
    }
}
