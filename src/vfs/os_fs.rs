//! TODO

use std::{
	fs::{self, File, FileType},
	io::{self, ErrorKind},
	path::Path
};

use tokio::io::BufReader;
use walkdir::{DirEntry, WalkDir};

use crate::{RelativePath, VfsPackFile, VfsPackFileIterEntry, VfsPackFileMetadata};

use super::{DirectoryTraversalOptions, VirtualFileSystem};

/// TODO
pub struct OsFilesystem;

impl VirtualFileSystem for OsFilesystem {
	type FileRead = BufReader<tokio::fs::File>;
	type FileIter = impl Iterator<Item = Result<VfsPackFileIterEntry, io::Error>>;

	fn file_iterator(
		&self,
		root_path: &Path,
		directory_traversal_options: DirectoryTraversalOptions
	) -> Self::FileIter {
		let entry_iter = WalkDir::new(&root_path)
			.follow_links(true)
			.into_iter()
			.filter_entry(move |entry| {
				!directory_traversal_options.ignore_system_and_hidden_files
					|| !is_system_or_hidden_file(entry)
			});

		entry_iter.filter_map(|entry_result| {
			entry_result.map_or_else(
				|err| Some(Err(io::Error::new(ErrorKind::Other, err))),
				|entry| {
					// Do not yield directories. We are interested in the directory tree leaves only
					(!entry.file_type().is_dir()).then(|| {
						// Use the entry depth to efficiently get a root path without doing
						// additional allocations or moving ownership of the method parameter
						let entry_depth = entry.depth();
						let file_path = entry.into_path();
						let root_path = file_path.ancestors().take(1 + entry_depth).last().unwrap();

						Ok(VfsPackFileIterEntry {
							relative_path: RelativePath::new(root_path, &file_path)?.into_owned(),
							file_path
						})
					})
				}
			)
		})
	}

	fn open<P: AsRef<Path>>(&self, path: P) -> Result<VfsPackFile<Self::FileRead>, io::Error> {
		// This matches what walkdir would do on a DirEntry, because we follow symlinks
		let metadata = fs::metadata(&path)?;

		Ok(VfsPackFile {
			file_read: BufReader::new(tokio::fs::File::from_std(File::open(path)?)),
			metadata: VfsPackFileMetadata {
				creation_time: metadata.created().ok(),
				modification_time: metadata.modified().ok()
			},
			file_size: metadata.len()
		})
	}

	fn file_type<P: AsRef<Path>>(&self, path: P) -> Result<FileType, io::Error> {
		fs::metadata(path).map(|metadata| metadata.file_type())
	}
}

/// Checks whether a [DirEntry] is a system or hidden file. This operation does no syscalls.
fn is_system_or_hidden_file(entry: &DirEntry) -> bool {
	let file_name = entry.file_name().to_string_lossy();

	// List based on https://www.toptal.com/developers/gitignore/api/git,windows,linux,macos
	file_name.starts_with('.')
		|| if entry.file_type().is_file() {
			file_name == "desktop.ini"
				|| file_name == "Desktop.ini"
				|| file_name == "Thumbs.db"
				|| file_name == "ehthumbs.db"
				|| file_name == "ehthumbs_vista.db"
				|| file_name.ends_with(".lnk")
				|| file_name.ends_with(".orig")
				|| file_name.ends_with(".bak")
		} else {
			file_name == "Network Trash Folder"
				|| file_name == "Temporary Items"
				|| file_name == "$RECYCLE.BIN"
		}
}

#[cfg(test)]
mod tests {
	use super::*;

	use std::fs::File;

	use tempfile::Builder;

	#[test]
	fn os_filesystem_vfs_works() {
		let root_dir = Builder::new()
			.prefix("ps-osfs-test")
			.tempdir()
			.expect("I/O operations are assumed not to fail during tests");

		let mut file1_path = root_dir.path().join("hello");
		file1_path.push("world.txt");
		{
			fs::create_dir_all(file1_path.parent().unwrap())
				.expect("I/O operations are assumed not to fail during tests");

			File::create(&file1_path).expect("I/O operations are assumed not to fail during tests");
		}

		let mut file2_path = root_dir.path().join("bye");
		file2_path.push("bye");
		file2_path.push("now.txt");
		{
			fs::create_dir_all(file2_path.parent().unwrap())
				.expect("I/O operations are assumed not to fail during tests");

			File::create(&file2_path).expect("I/O operations are assumed not to fail during tests");
		}

		let mut file3_path = root_dir.path().join("bye");
		file3_path.push(".dont_come_back.txt");
		{
			File::create(&file3_path).expect("I/O operations are assumed not to fail during tests");
		}

		let file_iter = OsFilesystem.file_iterator(
			root_dir.path(),
			DirectoryTraversalOptions {
				ignore_system_and_hidden_files: true
			}
		);

		const RELATIVE_PATHS: &[&str] = &["hello/world.txt", "bye/bye/now.txt"];

		let mut file_count = 0;
		for file in file_iter {
			assert!(RELATIVE_PATHS.contains(
				&file
					.expect("I/O operations are assumed not to fail during tests")
					.relative_path
					.as_ref()
			));

			file_count += 1;
		}

		assert_eq!(
			file_count,
			RELATIVE_PATHS.len(),
			"Unexpected number of files yielded"
		);
	}
}
