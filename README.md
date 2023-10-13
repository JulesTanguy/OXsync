# oxsync - File Synchronization Tool in Rust

Welcome to oxsync ! This project is shaping up to redefine file synchronization using Rust, primarily designed with a strong focus on the "watch for changes" feature.

## Purpose

oxsync is geared towards enabling fast, local reads with a remote filesystem. The conventional setup advices to initiate with a copy of remote files and directories on one's local machine, while the tool monitors and synchronizes only the modifications performed when the program is running. Please note that this tool is not designed to capture changes implemented from other sources or means, as any updates from other means may lead to overwriting during the next local change. It's essential to note that oxsync is not intended to serve as a Version Control System (VCS).

## Key Features (Planned)

- Real-time "watch for changes" functionality for swift synchronization.
- User-friendly interface with intuitive commands.
- Local mirroring of remote file system for quick reads.
- Integrity checks with BLAKE3
- Initial copy builtin into the program
- Display diffs on each change
- Good handling of big and small files
- An "exclude" argument with glob pattern support

## Installation

As this is an ongoing project, **DO NOT USE FOR A PRODUCTION SETUP**.

## Acknowledgements

This project would not have been possible without the incredible work of the open-source community. I would like to express our sincere gratitude towards the developers of the amazing libraries that have been essential in the construction of oxsync. Their impressive and generous contributions to the open-source ecosystem have didn't just make this project feasible, but also nurtured a platform for developers to learn, build, and grow.

Feel free to look at the `Cargo.toml` file at the root of the repository. It provides a comprehensive list of all the libraries that play a crucial part in the development of this project.
