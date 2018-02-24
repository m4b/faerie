# faerie [![Build Status](https://travis-ci.org/m4b/faerie.svg?branch=master)](https://travis-ci.org/m4b/faerie)

Emit some object files, at your leisure:

```rust
let name = "test.o";
let file = File::create(Path::new(name))?;
let mut obj = ArtifactBuilder::new(Target::X86_64)
    .name(name)
    .finish();

// first we declare our symbolic references;
// it is a runtime error to define a symbol _without_ declaring it first
obj.declarations(
    [
        ("deadbeef", Decl::Function { global: false }),
        ("main",     Decl::Function { global: true }),
        ("str.1",    Decl::CString { global: false }),
        ("DEADBEEF", Decl::DataImport),
        ("printf",   Decl::FunctionImport),
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

// Finally, we write which object file we desire
obj.write::<Elf>(file)?;
```

Will emit an object file like this:

<pre><font color="#D3D7CF">ELF </font><span style="background-color:#FCE94F"><font color="#555753">REL</font></span> <font color="#D3D7CF"><b>X86_64</b></font>-little-endian @ <font color="#CC0000">0x0</font>:

e_phoff: <font color="#C4A000">0x0</font> e_shoff: <font color="#C4A000">0x2a2</font> e_flags: 0x0 e_ehsize: 64 e_phentsize: 56 e_phnum: 0 e_shentsize: 64 e_shnum: 9 e_shstrndx: 1

<font color="#D3D7CF">SectionHeaders(9)</font>:
  <b>Idx</b>   <b>Name           </b>   <b>        Type</b>   <b>Flags               </b>   <b>Offset</b>   <b>Addr</b>   <b>Size </b>   <b>Link      </b>   <b>Entsize</b>   <b>Align</b>  
  <span style="background-color:#D3D7CF"><font color="#2E3436">0  </font></span>   <span style="background-color:#D3D7CF"><font color="#2E3436">               </font></span>       SHT_NULL                          <font color="#C4A000">0x0   </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x0  </b></font>                0x0       0x0    
  <span style="background-color:#2E3436"><font color="#D3D7CF">1  </font></span>   <span style="background-color:#2E3436"><font color="#D3D7CF">.strtab        </font></span>     SHT_STRTAB                          <font color="#C4A000">0x8c  </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0xc6 </b></font>                0x0       0x1    
  <span style="background-color:#D3D7CF"><font color="#2E3436">2  </font></span>   <span style="background-color:#D3D7CF"><font color="#2E3436">.symtab        </font></span>     SHT_SYMTAB                          <font color="#C4A000">0x152 </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0xf0 </b></font>   .strtab(1)   0x18      0x8    
  <span style="background-color:#2E3436"><font color="#D3D7CF">3  </font></span>   <span style="background-color:#2E3436"><font color="#D3D7CF">.data.str.1    </font></span>   SHT_PROGBITS   <b>ALLOC MERGE STRINGS </b>   <font color="#C4A000">0x40  </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x10 </b></font>                0x1       0x1    
  <span style="background-color:#D3D7CF"><font color="#2E3436">4  </font></span>   <span style="background-color:#D3D7CF"><font color="#2E3436">.text.deadbeef </font></span>   SHT_PROGBITS   <b>ALLOC EXECINSTR     </b>   <font color="#C4A000">0x50  </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x14 </b></font>                0x0       0x10   
  <span style="background-color:#2E3436"><font color="#D3D7CF">5  </font></span>   <span style="background-color:#2E3436"><font color="#D3D7CF">.text.main     </font></span>   SHT_PROGBITS   <b>ALLOC EXECINSTR     </b>   <font color="#C4A000">0x64  </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x28 </b></font>                0x0       0x10   
  <span style="background-color:#D3D7CF"><font color="#2E3436">6  </font></span>   <span style="background-color:#D3D7CF"><font color="#2E3436">.reloc.main    </font></span>       SHT_RELA                          <font color="#C4A000">0x242 </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x48 </b></font>   .symtab(2)   0x18      0x8    
  <span style="background-color:#2E3436"><font color="#D3D7CF">7  </font></span>   <span style="background-color:#2E3436"><font color="#D3D7CF">.reloc.deadbeef</font></span>       SHT_RELA                          <font color="#C4A000">0x28a </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x18 </b></font>   .symtab(2)   0x18      0x8    
  <span style="background-color:#D3D7CF"><font color="#2E3436">8  </font></span>   <span style="background-color:#D3D7CF"><font color="#2E3436">.note.GNU-stack</font></span>   SHT_PROGBITS                          <font color="#C4A000">0x0   </font>   <font color="#CC0000"><b>0x0 </b></font>   <font color="#4E9A06"><b>0x0  </b></font>                0x0       0x1    

<font color="#D3D7CF">Syms(10)</font>:
  <b>             Addr</b>   <b>Bind    </b>   <b>Type     </b>   <b>Symbol  </b>   <b>Size </b>   <b>Section          </b>   <b>Other</b>  
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   NOTYPE                 <font color="#4E9A06">0x0  </font>                       0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   FILE        <font color="#FCE94F"><b>test.o  </b></font>   <font color="#4E9A06">0x0  </font>   <font color="#D3D7CF"><i>ABS              </i></font>   0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   SECTION                <font color="#4E9A06">0x0  </font>   .data.str.1(3)      0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   SECTION                <font color="#4E9A06">0x0  </font>   .text.deadbeef(4)   0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   SECTION                <font color="#4E9A06">0x0  </font>   .text.main(5)       0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   <font color="#FCE94F"><b>OBJECT   </b></font>   <font color="#FCE94F"><b>str.1   </b></font>   <font color="#4E9A06">0x10 </font>   .data.str.1(3)      0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#34E2E2"><font color="#555753"><b>LOCAL   </b></font></span>   <font color="#EF2929"><b>FUNC     </b></font>   <font color="#FCE94F"><b>deadbeef</b></font>   <font color="#4E9A06">0x14 </font>   .text.deadbeef(4)   0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#EF2929"><font color="#555753"><b>GLOBAL  </b></font></span>   <font color="#EF2929"><b>FUNC     </b></font>   <font color="#FCE94F"><b>main    </b></font>   <font color="#4E9A06">0x28 </font>   .text.main(5)       0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#EF2929"><font color="#555753"><b>GLOBAL  </b></font></span>   NOTYPE      <font color="#FCE94F"><b>DEADBEEF</b></font>   <font color="#4E9A06">0x0  </font>                       0x0    
  <font color="#CC0000">               0 </font>   <span style="background-color:#EF2929"><font color="#555753"><b>GLOBAL  </b></font></span>   NOTYPE      <font color="#FCE94F"><b>printf  </b></font>   <font color="#4E9A06">0x0  </font>                       0x0    

<font color="#D3D7CF">Shdr Relocations(4)</font>:
<font color="#D3D7CF"><b>  .text.main</b></font>(3)
<font color="#CC0000">              13</font> X86_64_PC32 <font color="#C4A000"><b>.data.str.1</b></font>
<font color="#CC0000">              1d</font> X86_64_PLT32 <font color="#C4A000"><b>printf</b></font>+<font color="#CC0000">-4</font>
<font color="#CC0000">               a</font> X86_64_PLT32 <font color="#C4A000"><b>.text.deadbeef</b></font>+<font color="#CC0000">-4</font>

<font color="#D3D7CF"><b>  .text.deadbeef</b></font>(1)
<font color="#CC0000">               7</font> X86_64_GOTPCREL <font color="#C4A000"><b>DEADBEEF</b></font>+<font color="#CC0000">-4</font>
</pre>

:sunglasses:
