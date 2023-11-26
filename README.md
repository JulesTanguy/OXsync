# oxsync
Sync changes from a directory to another

```
Usage: oxsync.exe [OPTIONS] <SOURCE_DIR> <TARGET_DIR>

Arguments:
  <SOURCE_DIR>  Path of the directory to watch changes from
  <TARGET_DIR>  Path of the directory to write changes to

Options:
  -e, --exclude <EXCLUDE>               Exclude file or dir from the <SOURCE_DIR>
      --exclude-temporary-editor-files  Exclude filenames ending with a tilde `~` [aliases: exclude-tmp]
      --ide-mode                        Exclude `.git` and `.idea` dirs + enable the `exclude-temporary-editor-files` option [aliases: ide]
      --statistics                      Get how much time is needed to copy a file [aliases: stats]
      --trace                           Set the log level to trace
  -h, --help                            Print help
  -V, --version                         Print version
```

## Purpose
oxsync is geared towards enabling fast, local reads with a remote filesystem.
The conventional setup advices to initiate with a copy of local files and directories on the remote machine,
while the tool monitors and synchronizes the modifications performed when the program is running.

## Features
- Real-time "watch for changes" functionality for near immediate synchronization.
- CLI interface with intuitive commands.
- Local copy of remote directories for quick reads.
- Handle big and small files
- An "exclude" argument
- Tested and fully functional on Windows

## Installation
```sh
# Beforehand make sure to have a functional Rust compiler and 
# the Cargo package manager installed
cargo install oxsync
```

## Acknowledgements
As always, feel free to look at the `dependencies` of the `Cargo.toml` file at the root of the repository. It provides a comprehensive list 
of all the libraries that play a crucial part in the development of this project.
