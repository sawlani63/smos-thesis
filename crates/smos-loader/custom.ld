PHDRS
{
	headers PT_PHDR PHDRS;
	data PT_LOAD;
	text PT_LOAD FILEHDR PHDRS;
	ro_data PT_LOAD;
	tls PT_TLS;
	synthetic_builtins PT_LOAD;
}


ENTRY(_start)

SECTIONS {
	. = 0x10000000;

	. = SIZEOF_HEADERS;
	. = ALIGN(0x1000);
	.data : {
		*(.data)
	} : data

	. = ALIGN(0x1000);
	.text : {
		*(.text)
	} : text

	. = ALIGN(0x1000);
	.ro_data : {
		*(.ro_data)
	} : ro_data

	.tbss : {
		*(.tbss)
		*(.tbss.*)      /* Handle tbss sections from external crates */
		*(.gnu.linkonce.tb.*)
	} : tls

	. = ALIGN(0x1000);
	.bss : {
		*(.bss)
	} : bss

	. = ALIGN(0x1000);
	.synthetic_builtins : {
		*(.synthetic_builtins)
	} : synthetic_builtins
}