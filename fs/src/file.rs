/// Copying files on disk.
pub mod copier;
/// Deleting files from disk.
pub mod deleter;
/// Reading files and directory entries from disk.
pub mod reader;
/// Renaming/moving files on disk.
pub mod renamer;
/// Writing files to disk.
pub mod writer;

/// Filesystem-backed implementation of the file operation traits
/// ([`FileReader`](reader::FileReader), [`FileWriter`](writer::FileWriter),
/// [`FileRenamer`](renamer::FileRenamer), [`FileDeleter`](deleter::FileDeleter),
/// [`FileCopier`](copier::FileCopier)).
#[derive(Debug)]
pub struct LocalFile;
