MEMORY
{
  FLASH (rx) : ORIGIN = 0x10000000, LENGTH = 2M
  RAM   (rwx): ORIGIN = 0x20000000, LENGTH = 264K
}

__stack_top = ORIGIN(RAM) + LENGTH(RAM);
__stack_limit = __stack_top - 24K;
__stack_reserved_bytes = __stack_top - __stack_limit;

ENTRY(Reset);

SECTIONS
{
  .vector_table ORIGIN(FLASH) :
  {
    KEEP(*(.vector_table.reset_vector))
  } > FLASH

  .text :
  {
    *(.text*)
    *(.rodata*)
  } > FLASH

  .data : ALIGN(4)
  {
    *(.data*)
  } > RAM AT > FLASH

  .bss (NOLOAD) : ALIGN(4)
  {
    *(.bss*)
    *(COMMON)
  } > RAM

  . = ALIGN(8);
  __static_ram_end = .;
}

ASSERT(__static_ram_end <= __stack_limit, "pico smoke static RAM exceeds the reserved 24 KiB kernel stack budget")
