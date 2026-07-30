# Local compatibility patch

This directory vendors `block` 0.1.6 unchanged except for compatibility
updates in `src/lib.rs`.

The published crate used an empty enum for that marker. Rust's
`uninhabited_static` lint is turning extern statics of empty-enum types into a
hard error. The replacement is a zero-sized, C-compatible opaque struct; the
crate only takes the extern symbol's address and never constructs or reads the
type.

The crate also predates explicit ABI syntax, so its bare `extern` declarations
are written as `extern "C"`. C was already Rust's default for these declarations;
the change makes the existing ABI explicit and removes the corresponding
deprecation warning.

Remove this patch when the `metal`/RISC Zero dependency chain no longer uses
`block` 0.1.6.
