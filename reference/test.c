extern int printf(const char *format, ...);
extern int DEADBEEF;

int STATIC = 0xcafebabe;
int* STATIC_REF = &STATIC;

int deadbeef() {
  return DEADBEEF + 1;
}

int main() {
  printf("deadbeef: 0x%x - 0x%x\n", deadbeef(), *STATIC_REF);
  return 0;
}
