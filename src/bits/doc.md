//! Package `bits` provides bit and byte reading and writing, including Golomb codes and EBSP as used by MPEG video standards.
//!
//! All readers and writers accumulate errors in the sense that they will stop reading or writing at the first error.
//! The first error, if any, can be retrieved with an `acc_error()` method.
//!
//! EBSP (Encapsulated Byte Sequence Packets) uses insertion of start-code emulation prevention bytes 0x03 and is
//! used in MPEG video standards from AVC (H.264) and forward. The main types are:
//!
//! - [`Reader`]: reads bits and bytes from an underlying [`std::io::Read`] with accumulated error.
//! - [`Writer`]: writes bits and bytes to an underlying [`std::io::Write`] with accumulated error.
//! - [`EBSPReader`]: reads EBSP from an underlying [`std::io::Read`] with accumulated error.
//! - [`EBSPWriter`]: writes EBSP to an underlying [`std::io::Write`] with accumulated error.
//! - [`ByteWriter`]: writes byte-based structures to an underlying [`std::io::Write`] with accumulated error.
//! - [`FixedSliceReader`]: reads various byte-based structures from a fixed slice with accumulated error.
//! - [`FixedSliceWriter`]: writes various byte-based structures to a fixed slice with accumulated error.
//!
//! [`Reader`]: crate::Reader
//! [`Writer`]: crate::Writer
//! [`EBSPReader`]: crate::EBSPReader
//! [`EBSPWriter`]: crate::EBSPWriter
//! [`ByteWriter`]: crate::ByteWriter
//! [`FixedSliceReader`]: crate::FixedSliceReader
//! [`FixedSliceWriter`]: crate::FixedSliceWriter

// This file would typically be lib.rs or part of a module's doc comment.
// For this example, we assume it's the main crate documentation.
