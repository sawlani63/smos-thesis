PHDRS
{
	headers PT_PHDR PHDRS;
	text PT_LOAD FILEHDR PHDRS;
	ro_data PT_LOAD;
	data PT_LOAD;
	tls PT_TLS;
	bss PT_LOAD;
}


ENTRY(_start)

SECTIONS {
	. = 0x10000000;

	. = SIZEOF_HEADERS;
	. = ALIGN(0x1000);
	.text : {
        *(.text)
        *(.text.*)
	} : text

	. = ALIGN(0x1000);
	.ro_data : {
        *(.rodata)
        *(.rodata.*)
	} : ro_data

	. = ALIGN(0x1000);
	.data : {
        *(.data)
        *(.data.*)
	} : data

	. = ALIGN(0x1000);
	.bss : {
        *(.bss)
        *(.bss.*)
	} : bss

	. = ALIGN(0x1000);
	.tbss : {
		*(.tbss)
		*(.tbss.*)
	} : tls
}