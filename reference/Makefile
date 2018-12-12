# Build the object files that are used as a reference for src/bin/main.rs

CC = gcc
CFLAGS = -ffunction-sections -fdata-sections -fPIC -g

LD = $(CC)
LDFLAGS =
LDLIBS = -lc

# Other variants:
#CC = clang
#LDFLAGS += -fuse-ld=gold

all: test

test: test.o deadbeef.o
	$(LD) $(LDFLAGS) $^ $(LDLIBS) -o $@

%.o: %.c
	$(CC) $(CFLAGS) -c $^ -o $@

clean:
	-rm *.o test
