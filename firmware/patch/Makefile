# Build a patched DM40 firmware update file.
#
#   make ORIG=../../dm40_v_2_6.bin            -> build/dm40_patched.bin
#   make ORIG=... SRCS="src/startup.c src/demo_version.c"

CROSS   ?= arm-none-eabi-
CC      := $(CROSS)gcc
OBJDUMP := $(CROSS)objdump
PYTHON  ?= python3

ORIG    ?= ../../dm40_v_2_6.bin
SRCS    ?= src/startup.c src/demo_version.c
BUILD   ?= build

CFLAGS  := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -Os -g -Wall -Wextra -ffreestanding -fno-common \
           -ffunction-sections -fdata-sections
LDFLAGS := -nostdlib -nostartfiles -Wl,--gc-sections -Wl,-Map=$(BUILD)/patch.map \
           -L. -T dm40.ld

OBJS := $(patsubst src/%.c,$(BUILD)/%.o,$(filter %.c,$(SRCS))) \
        $(patsubst src/%.S,$(BUILD)/%.o,$(filter %.S,$(SRCS))) \
        $(BUILD)/fw_v2_6.o

all: $(BUILD)/dm40_patched.bin

# Demo without the startup hook: the boot path of the firmware stays original
demo:
	$(MAKE) BUILD=build-demo SRCS=src/demo_version.c

# Live measurement over USB (EP0 vendor request); no startup hook needed
usb:
	$(MAKE) BUILD=build-usb SRCS="src/usb_hooks.c src/usb_stream.S"

# Remote-debug variant: memrw + LCD screenshot read-back (BT cmd 0x0B sub 3/4).
# Separate image so the plain memrw build stays byte-identical. All new code
# runs only from the BT command handler; no new always-executed code.
dbg:
	$(MAKE) BUILD=build-dbg SRCS="src/startup.c src/stockdbg.c"
	cp build-dbg/dm40_patched.bin build-dbg/dm40_dbg.bin
	@echo "built build-dbg/dm40_dbg.bin"

$(BUILD)/%.o: src/%.c hooks.h fw_v2_6.h | $(BUILD)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/%.o: src/%.S hooks.h | $(BUILD)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/fw_v2_6.o: fw_v2_6.S | $(BUILD)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD)/patch.elf: $(OBJS) dm40.ld fw_v2_6.ld
	$(CC) $(CFLAGS) $(LDFLAGS) $(OBJS) -o $@
	$(OBJDUMP) -d $@ > $(BUILD)/patch.lst

$(BUILD)/dm40_patched.bin: $(BUILD)/patch.elf patcher.py ../dm40fw.py
	$(PYTHON) patcher.py $(ORIG) $< $@

$(BUILD):
	mkdir -p $@

clean:
	rm -rf build build-demo build-dbg

.PHONY: all clean demo usb dbg
