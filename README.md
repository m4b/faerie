# faerie [![Build Status](https://travis-ci.org/m4b/faerie.svg?branch=master)](https://travis-ci.org/m4b/faerie)

Emit some object files, at your leisure:

```rust
let mut obj = Artifact::new(Target::X86_64, Some(String::from("test.o")));
// 55	push   %rbp
// 48 89 e5	mov    %rsp,%rbp
// b8 ef be ad de	mov    $0xdeadbeef,%eax
// 5d	pop    %rbp
// c3	retq

obj.add_code("deadbeef", vec![0x55, 0x48, 0x89, 0xe5, 0xb8, 0xef, 0xbe, 0xad, 0xde, 0x5d, 0xc3]);
// main:
// 55	push   %rbp
// 48 89 e5	mov    %rsp,%rbp
// b8 00 00 00 00	mov    $0x0,%eax
// e8 d4 ff ff ff	callq  0x0 <deadbeef>
// 89 c6	mov    %eax,%esi
// 48 8d 3d 00 00 00 00	lea    0x0(%rip),%rdi # will be: deadbeef: 0x%x\n
// b8 00 00 00 00	mov    $0x0,%eax
// e8 00 00 00 00	callq  0x3f <main+33>  # printf
// b8 00 00 00 00	mov    $0x0,%eax
// 5d	pop    %rbp
// c3	retq

obj.add_code("main", vec![0x55, 0x48, 0x89, 0xe5, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xe8, 0xe2, 0xff, 0xff, 0xff, 0x89, 0xc6, 0x48, 0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xe8, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x5d, 0xc3]);

obj.add_data("str.1", b"deadbeef: 0x%x\n\0".to_vec());

obj.link("main", "str.1", 19);

obj.import("printf");

obj.link_import("main", "printf", 29);

println!("res: {:#?}", obj);

obj.write::<Elf>(::std::fs::File::create(Path::new(&arg))?)?;
```

Will emit an object file like this:

```
ELF REL X86_64-little-endian @ 0x0:

e_phoff: 0x0 e_shoff: 0x226 e_flags: 0x0 e_ehsize: 64 e_phentsize: 56 e_phnum: 0 e_shentsize: 64 e_shnum: 7 e_shstrndx: 1

SectionHeaders(7):

  Idx   Name                     Type   Flags                  Offset   Addr   Size    Link        Info              Entsize   Align  
  0                          SHT_NULL                          0x0      0x0    0x0                                   0x0       0x0    
  1     strtab             SHT_STRTAB   ALLOC                  0x83     0x0    0x9b                                  0x0       0x1    
  2     symtab             SHT_SYMTAB   ALLOC                  0x11e    0x0    0xd8    strtab(1)   global start: 5   0x18      0x8    
  3     .text.deadbeef   SHT_PROGBITS   ALLOC EXECINSTR        0x40     0x0    0xb                                   0x0       0x10   
  4     .text.main       SHT_PROGBITS   ALLOC EXECINSTR        0x4b     0x0    0x28                                  0x0       0x10   
  5     .data.str.1      SHT_PROGBITS   ALLOC MERGE STRINGS    0x73     0x0    0x10                                  0x1       0x1    
  6     .reloc.main          SHT_RELA                          0x1f6    0x0    0x30    symtab(2)   .text.main(4)     0x18      0x8    

Syms(9):

               Addr   Bind       Type        Symbol     Size    Section             Other  
                 0    LOCAL      NOTYPE                 0x0                         0x0    
                 0    LOCAL      FILE        test.o     0x0     ABS                 0x0    
                 0    LOCAL      SECTION                0x0     .text.deadbeef(3)   0x0    
                 0    LOCAL      SECTION                0x0     .text.main(4)       0x0    
                 0    LOCAL      SECTION                0x0     .data.str.1(5)      0x0    
                 0    GLOBAL     FUNC        deadbeef   0xb     .text.deadbeef(3)   0x0    
                 0    GLOBAL     FUNC        main       0x28    .text.main(4)       0x0    
                 0    GLOBAL     OBJECT      str.1      0x10    .data.str.1(5)      0x0    
                 0    GLOBAL     NOTYPE      printf     0x0                         0x0    

Shdr Relocations(2):

  .text.main(2)
              13 X86_64_PC32 .data.str.1
              1d X86_64_PLT32 printf+0xfffffffffffffffc

```

:sunglasses:
