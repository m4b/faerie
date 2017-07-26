# faerie

Emit some object files, at your leisure:

```rust
let mut obj: Elf = Artifact::new(Target::X86_64, Some("test.o"));
// 55	push   %rbp
// 48 89 e5	mov    %rsp,%rbp
// b8 ef be ad de	mov    $0xdeadbeef,%eax
// 5d	pop    %rbp
// c3	retq

obj.add_code("deadbeef".to_owned(), vec![0x55, 0x48, 0x89, 0xe5, 0xb8, 0xef, 0xbe, 0xad, 0xde, 0x5d, 0xc3]);
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

obj.add_code("main".to_owned(), vec![0x55, 0x48, 0x89, 0xe5, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xe8, 0xe2, 0xff, 0xff, 0xff, 0x89, 0xc6, 0x48, 0x8d, 0x3d, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x00, 0x00, 0x00, 0x00, 0xe8, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x5d, 0xc3]);

obj.add_data("str.1".to_owned(), b"deadbeef: 0x%x\n\0".to_vec());

obj.link("main", "str.1", 19);

obj.import("printf".to_owned());

obj.link_import("main", "printf", 29);

println!("res: {:#?}", obj);

obj.write(::std::fs::File::create(Path::new(&arg))?)?;
```

:sunglasses:
