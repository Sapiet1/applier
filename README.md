# applier

## Overview

`applier` is a simple command-line utility written with `clap` to concurrently call a given command to all child directories within a parent directory. It utilizes asynchronous Rust, using the `tokio` and `futures` crates.

## Usage

### Installation

Rust should first be installed on the system. Then, the utility can be installed with the following commands:

```bash
# Clone the repository
git clone https://github.com/Sapiet/applier.git

# Change into the project directory
cd applier

# Install the utility
cargo install --path .
```

### Running `applier`

Executing `applier` should give:

```
A CLI for applying a command to directories within a directory

Usage: applier [OPTIONS] <COMMAND>

Options:
      --path=<PATH>    A path to specify for the parent directory
  -i, --ignore=<PATH>  A path of the children directories to ignore
  -h, --help           Print help
  -V, --version        Print version
```

Take note that `COMMAND` should always be listed after the flags. Also, `COMMAND` cannot be a shell command, and therefore `cd`, `echo`, etc. will not run as expected.

If the path is not specified, it will use the current working directory. To ignore multiple child directories, the `-i=<PATH>` can be repeated, which is relative the specified parent directory. Below are example usages:
```bash
applier --path="./full_of_old_rust_projects" -i="./important_rust_project" cargo clean
```
